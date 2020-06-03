use std::borrow::Cow;
use std::io;
use std::path::Path;

use flate2::read::DeflateDecoder;
use log::*;

use crate::arch::usize;
use crate::crc_reader::Crc32Reader;
use crate::result::*;
use crate::spec;

// Move types into some submodule if we have a handful?
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum CompressionMethod {
    None,
    Deflate,
    Unsupported(u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum System {
    Dos,
    Unix,
    Unknown,
}

/// Information from a file's CentralDirectoryEntry,
/// distilled down to stuff the rest of the library will use.
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
    pub(crate) header_offset: usize,
    // TODO: Add other fields the user might want to know about:
    // time, etc.
}

impl<'a> FileMetadata<'a> {
    pub fn is_dir(&self) -> bool {
        // Path::ends_with() doesn't consider separators,
        // so we need a different approach.
        // to_str().unwrap() is safe since the provided string was UTF-8,
        // or was decoded from CP437.
        let filename_str = self.file_name.to_str().unwrap();
        self.size == 0 && filename_str.ends_with('/')
    }

    pub fn is_file(&self) -> bool {
        !self.is_dir()
    }
}

pub struct ZipArchive<'a> {
    mapping: &'a [u8],
    entries: Vec<FileMetadata<'a>>,
}

impl<'a> ZipArchive<'a> {
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

    pub fn new(mapping: &'a [u8]) -> ZipResult<Self> {
        let (new_archive, archive_offset) = Self::with_prepended_data(mapping)?;
        if archive_offset != 0 {
            return Err(ZipError::PrependedWithUnknownBytes(archive_offset));
        }
        Ok(new_archive)
    }

    pub fn entries(&self) -> &[FileMetadata] {
        &self.entries
    }

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
