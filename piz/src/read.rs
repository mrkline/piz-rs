//! Tools for reading a ZIP archive.
//!
//! Current versions of this library don't do any writing,
//! but it was arranged to resemble the structure of the [Zip crate]
//! and make room for potential future writing tools.
//!
//! [Zip crate]: https://crates.io/crates/zip

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::io;
use std::path::{Component, Path};

use flate2::read::DeflateDecoder;
use log::*;

use crate::arch::usize;
use crate::crc_reader::Crc32Reader;
use crate::result::*;
use crate::spec;

// Move types into some submodule if we have a handful?

/// The compression method used to store a file
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum CompressionMethod {
    /// The file is uncompressed
    None,
    /// The file is [DEFLATE](https://en.wikipedia.org/wiki/DEFLATE)d.
    /// This is the most common format used by ZIP archives.
    Deflate,
    /// The file is compressed with a yet-unsupported format.
    /// (The u16 indicates the internal format code.)
    Unsupported(u16),
}

/// Metadata for a file or directory in the archive,
/// retrieved from its central directory
#[derive(Debug, PartialEq, Eq)]
pub struct FileMetadata<'a> {
    /// Uncompressed size of the file in bytes
    pub size: usize,
    /// Compressed size of the file in bytes
    pub compressed_size: usize,
    /// Compression algorithm used to store the file
    pub compression_method: CompressionMethod,
    /// The CRC-32 of the decompressed file
    pub crc32: u32,
    /// True if the file is encrypted (decryption is unsupported)
    pub encrypted: bool,
    /// The provided path of the file.
    pub file_name: Cow<'a, Path>,
    /// The offset to the local file header in the archive
    pub(crate) header_offset: usize,
    // TODO: Add other fields the user might want to know about:
    // time, etc.
}

impl<'a> FileMetadata<'a> {
    /// Returns true if the given entry is a directory
    pub fn is_dir(&self) -> bool {
        // Path::ends_with() doesn't consider separators,
        // so we need a different approach.
        // to_str().unwrap() is safe since the provided string was UTF-8,
        // or was decoded from CP437.
        let filename_str = self.file_name.to_str().unwrap();
        self.size == 0 && filename_str.ends_with('/')
    }

    /// Returns true if the given entry is a file
    pub fn is_file(&self) -> bool {
        !self.is_dir()
    }
}

/// A ZIP archive to be read
pub struct ZipArchive<'a> {
    /// The contents of the ZIP archive, as a byte slice.
    mapping: &'a [u8],
    /// A list of entries from the ZIP's central directory
    entries: Vec<FileMetadata<'a>>,
}

impl<'a> ZipArchive<'a> {
    /// Reads a ZIP archive from a byte slice.
    /// Smaller files can be read into a buffer.
    ///
    ///     let bytes = fs::read("foo.zip")?;
    ///     let archive = ZipArchive::new(&bytes)?;
    ///
    /// For larger ones, memory map!
    ///
    ///     let zip_file = File::open("foo.zip")?;
    ///     let mapping = unsafe { Mmap::map(&zip_file)? };
    ///     let archive = ZipArchive::new(&mapping)?;
    pub fn new(mapping: &'a [u8]) -> ZipResult<Self> {
        let (new_archive, archive_offset) = Self::with_prepended_data(mapping)?;
        if archive_offset != 0 {
            return Err(ZipError::PrependedWithUnknownBytes(archive_offset));
        }
        Ok(new_archive)
    }

    /// Like `ZipArchive::new()`, but allows arbitrary data to prepend the archive.
    /// Returns the ZipArchive and the number of bytes prepended to the archive.
    ///
    /// Since a ZIP archive's metadata sits at the back of the file,
    /// many formats consist of ZIP archives prepended with some other data.
    /// For example, a self-extracting archive is one with an executable in the front.
    pub fn with_prepended_data(mut mapping: &'a [u8]) -> ZipResult<(Self, usize)> {
        let eocdr_posit = spec::find_eocdr(&mapping)?;
        let eocdr = spec::EndOfCentralDirectory::parse(&mapping[eocdr_posit..])?;
        trace!("{:?}", eocdr);

        if eocdr.disk_number != eocdr.disk_with_central_directory {
            return Err(ZipError::UnsupportedArchive(format!(
                "No support for multi-disk archives: disk ({}) != disk with central directory ({})",
                eocdr.disk_number, eocdr.disk_with_central_directory
            )));
        }
        if eocdr.entries != eocdr.entries_on_this_disk {
            return Err(ZipError::UnsupportedArchive(format!(
                "No support for multi-disk archives: entries ({}) != entries this disk ({})",
                eocdr.entries, eocdr.entries_on_this_disk
            )));
        }

        let nominal_central_directory_offset: usize;
        let entry_count: u64;

        // Zip files can be prepended by arbitrary junk,
        // so all the given positions might be off.
        // Calculate the offset.
        let archive_offset;

        let zip64_eocdr_locator_posit = eocdr_posit
            .checked_sub(spec::Zip64EndOfCentralDirectoryLocator::size_in_file())
            .ok_or(ZipError::InvalidArchive(
                "Too small for anything but End Of Central Directory Record",
            ))?;
        if let Some(zip64_eocdr_locator) =
            spec::Zip64EndOfCentralDirectoryLocator::parse(&mapping[zip64_eocdr_locator_posit..])
        {
            trace!("{:?}", zip64_eocdr_locator);

            if eocdr.disk_number as u32 != zip64_eocdr_locator.disk_with_central_directory {
                return Err(ZipError::UnsupportedArchive(format!(
                    "No support for multi-disk archives: disk ({}) != disk with zip64 central directory ({})",
                    eocdr.disk_number, zip64_eocdr_locator.disk_with_central_directory
                )));
            }
            if zip64_eocdr_locator.disks != 1 {
                return Err(ZipError::UnsupportedArchive(format!(
                    "No support for multi-disk archives: Zip64 EOCDR locator reports {} disks",
                    zip64_eocdr_locator.disks
                )));
            }

            // Search for the zip64 EOCDR, from its nominal starting position
            // to the end of where it could be.
            let zip64_eocdr_search_start = usize(zip64_eocdr_locator.zip64_eocdr_offset)?;
            let zip64_eocdr_search_end = eocdr_posit
                .checked_sub(spec::Zip64EndOfCentralDirectoryLocator::size_in_file())
                .ok_or(ZipError::InvalidArchive(
                    "Too small for Zip64 End Of Central Directory Record",
                ))?;
            let zip64_eocdr_search_space =
                &mapping[zip64_eocdr_search_start..zip64_eocdr_search_end];

            let zip64_eocdr_posit = spec::find_zip64_eocdr(zip64_eocdr_search_space)?;
            // Since we're searching starting at the provided offset,
            // the returned position is the archive offset.
            archive_offset = zip64_eocdr_posit;
            let zip64_eocdr = spec::Zip64EndOfCentralDirectory::parse(
                &zip64_eocdr_search_space[zip64_eocdr_posit..],
            )?;

            trace!("{:?}", zip64_eocdr);

            nominal_central_directory_offset = usize(zip64_eocdr.central_directory_offset)?;
            entry_count = zip64_eocdr.entries;
        } else {
            // The offset is the actual position versus the stored one.
            let actual_cdr_posit = eocdr_posit.checked_sub(usize(eocdr.central_directory_size)?);
            let nominal_offset = usize(eocdr.central_directory_offset)?;
            archive_offset = actual_cdr_posit
                .and_then(|off| off.checked_sub(nominal_offset))
                .ok_or(ZipError::InvalidArchive(
                    "Invalid central directory size or offset",
                ))?;
            nominal_central_directory_offset = usize(eocdr.central_directory_offset)?;
            entry_count = eocdr.entries as u64;
        }

        mapping = &mapping[archive_offset..];
        trace!(
            "{} entries at nominal offset {}",
            entry_count,
            nominal_central_directory_offset
        );

        let mut central_directory = &mapping[nominal_central_directory_offset..];

        let mut entries = Vec::new();
        entries.reserve(usize(entry_count)?);

        for _ in 0..entry_count {
            let dir_entry = spec::CentralDirectoryEntry::parse_and_consume(&mut central_directory)?;
            trace!("{:?}", dir_entry);

            let file_metadata = FileMetadata::from_cde(&dir_entry)?;
            debug!("{:?}", file_metadata);
            entries.push(file_metadata);
        }

        Ok((ZipArchive { mapping, entries }, archive_offset))
    }

    /// Returns the entries found in the ZIP archive's central directory.
    ///
    /// No effort is made to deduplicate or otherwise validate these entries.
    /// Future releases might provide helper functions that builds a tree of
    /// these entries.
    pub fn entries(&self) -> &[FileMetadata] {
        &self.entries
    }

    /// Reads the given file from the ZIP archive.
    ///
    /// Since each file in a ZIP archive is compressed independently,
    /// multiple files can be read in parallel.
    pub fn read(&self, metadata: &FileMetadata) -> ZipResult<Box<dyn io::Read + Send + 'a>> {
        let mut file_slice = &self.mapping[metadata.header_offset..];
        let local_header = spec::LocalFileHeader::parse_and_consume(&mut file_slice)?;
        trace!("{:?}", local_header);
        let local_metadata =
            FileMetadata::from_local_header(&local_header, metadata.header_offset)?;
        debug!("Reading {:?}", local_metadata);
        if *metadata != local_metadata {
            return Err(ZipError::InvalidArchive(
                "Central directory entry doesn't match local file header",
            ));
        }

        if metadata.encrypted {
            return Err(ZipError::UnsupportedArchive(format!(
                "Can't read encrypted file {}",
                metadata.file_name.display()
            )));
        }

        make_reader(
            metadata.compression_method,
            metadata.crc32,
            io::Cursor::new(&file_slice[0..metadata.compressed_size]),
        )
    }
}

/// Returns a boxed read trait for a compressed file,
/// given its compression method and expected CRC.
fn make_reader<'a, R: io::Read + Send + 'a>(
    compression_method: CompressionMethod,
    crc32: u32,
    reader: R,
) -> ZipResult<Box<dyn io::Read + Send + 'a>> {
    match compression_method {
        CompressionMethod::None => Ok(Box::new(Crc32Reader::new(reader, crc32))),
        CompressionMethod::Deflate => {
            let deflate_reader = DeflateDecoder::new(reader);
            Ok(Box::new(Crc32Reader::new(deflate_reader, crc32)))
        }
        _ => Err(ZipError::UnsupportedArchive(String::from(
            "Compression method not supported",
        ))),
    }
}

pub type DirectoryContents<'a> = BTreeMap<&'a OsStr, DirectoryEntry<'a>>;

#[derive(Debug)]
pub struct Directory<'a> {
    pub metadata: &'a FileMetadata<'a>,
    pub children: DirectoryContents<'a>,
}

impl<'a> Directory<'a> {
    fn new(metadata: &'a FileMetadata<'a>) -> Self {
        Self {
            metadata,
            children: DirectoryContents::new(),
        }
    }
}

#[derive(Debug)]
pub enum DirectoryEntry<'a> {
    File(&'a FileMetadata<'a>),
    Directory(Directory<'a>),
}

impl<'a> DirectoryEntry<'a> {
    fn metadata(&self) -> &'a FileMetadata<'a> {
        match &self {
            DirectoryEntry::File(metadata) => metadata,
            DirectoryEntry::Directory(dir) => dir.metadata,
        }
    }

    fn name(&self) -> &'a OsStr {
        let path = &self.metadata().file_name;
        path.file_name().expect("Path ended in ..")
    }
}

pub fn treeify<'a>(entries: &'a [FileMetadata<'a>]) -> ZipResult<DirectoryContents<'a>> {
    let mut contents = DirectoryContents::new();

    for entry in entries {
        entree_entry(entry, &mut contents)?;
    }

    Ok(contents)
}

fn entree_entry<'a>(
    entry: &'a FileMetadata<'a>,
    tree: &mut DirectoryContents<'a>,
) -> ZipResult<()> {
    let path = &entry.file_name;

    let parent_dir = if let Some(parent) = path.parent() {
        walk_tree(parent, tree)?
    } else {
        tree
    };

    // Check: Path doesn't end in something weird.
    let _base = path
        .file_name()
        .ok_or_else(|| ZipError::Hierarchy(format!("Path {} ended in ..", path.display())))?;

    let to_insert: DirectoryEntry = if entry.is_dir() {
        DirectoryEntry::Directory(Directory::new(entry))
    } else {
        DirectoryEntry::File(entry)
    };

    if parent_dir.insert(to_insert.name(), to_insert).is_some() {
        return Err(ZipError::Hierarchy(format!(
            "Duplicate entry for {}",
            path.display()
        )));
    }

    Ok(())
}

fn walk_tree<'a, 'b>(
    path: &Path,
    tree: &'b mut DirectoryContents<'a>,
) -> ZipResult<&'b mut DirectoryContents<'a>> {
    let mut current = tree;

    for component in path.components() {
        match component {
            Component::Prefix(prefix) => {
                let prefix = prefix.as_os_str();
                return Err(ZipError::Hierarchy(format!(
                    "Prefix {} found in path {}",
                    prefix.to_string_lossy(),
                    path.display()
                )));
            }
            Component::RootDir => {
                warn!("Root directory found in path {}", path.display());
                // Huh. Keep going.
            }
            Component::CurDir => {
                warn!("Current dir (.) found in path {}", path.display());
                // Huh. Keep going.
            }
            Component::ParentDir => {
                // We could canonicalize it somewhere down the road.
                // Path::canonicalize() doesn't work because it tries
                // to actually resolve the path
                // (and failing if something doesn't exist there).
                // Maybe try https://crates.io/crates/path-clean some time?
                return Err(ZipError::Hierarchy(format!(
                    "Parent dir (..) found in path {}",
                    path.display()
                )));
            }

            Component::Normal(component) => {
                if let Some(child) = current.get_mut(component) {
                    match child {
                        DirectoryEntry::Directory(dir) => {
                            current = &mut dir.children;
                        }
                        _ => {
                            return Err(ZipError::Hierarchy(format!(
                                "{} is a file, expected a directory",
                                path.display()
                            )));
                        }
                    }
                } else {
                    return Err(ZipError::Hierarchy(format!(
                        "{} found before parent directories",
                        path.display()
                    )));
                }
            }
        }
    }
    Ok(current)
}
