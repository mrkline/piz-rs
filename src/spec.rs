//! Code specific to the ZIP file format specification.
//!
//! We try to keep the nitty gritty here,
//! and higher-level stuff in the [`read`] module.
//! (This pattern, like several others, was inspired by the Zip crate.)
//!
//! Most comments quote the ZIP spec, [`APPNOTE.TXT`].
//!
//! [_Zip Files: History, Explanation and Implementation_]
//! is also a fantastic resource and a great read.
//!
//! [`read`]: ../read/index.html
//! [`APPNOTE.TXT`]: https://pkware.cachefly.net/webdocs/APPNOTE/APPNOTE-6.3.6.TXT
//! [_Zip Files: History, Explanation and Implementation_]: https://www.hanshq.net/zip.html

use std::borrow::Cow;
use std::convert::TryInto;
use std::path::Path;

use chrono::{NaiveDate, NaiveDateTime};
use codepage_437::*;
use memchr::memmem;

use crate::arch::usize;
use crate::read::{CompressionMethod, FileMetadata};
use crate::result::*;

// Magic numbers denoting various sections of a ZIP archive

/// End of central directory magic number
const EOCDR_MAGIC: [u8; 4] = [b'P', b'K', 5, 6];
/// Zip64 end of central directory magic number
const ZIP64_EOCDR_MAGIC: [u8; 4] = [b'P', b'K', 6, 6];
/// Zip64 end of central directory locator magic number
const ZIP64_EOCDR_LOCATOR_MAGIC: [u8; 4] = [b'P', b'K', 6, 7];
/// Central directory magic number
const CENTRAL_DIRECTORY_MAGIC: [u8; 4] = [b'P', b'K', 1, 2];
/// Local file header magic number
const LOCAL_FILE_HEADER_MAGIC: [u8; 4] = [b'P', b'K', 3, 4];

impl CompressionMethod {
    fn from_u16(u: u16) -> Self {
        match u {
            0 => CompressionMethod::None,
            8 => CompressionMethod::Deflate,
            // 12 => CompressionMethod::Bzip2,
            v => CompressionMethod::Unsupported(v),
        }
    }
}

/// The OS a file in the archive was compressed with.
/// Used to decode additional metadata like permissions
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum System {
    Dos,
    Unix,
    Unknown,
}

#[allow(dead_code)]
impl System {
    fn from_source_version(source_version: u16) -> Self {
        // 4.4.2.1 The upper byte indicates the compatibility of the file
        // attribute information.  If the external file attributes
        // are compatible with MS-DOS and can be read by PKZIP for
        // DOS version 2.04g then this value will be zero.  If these
        // attributes are not compatible, then this value will
        // identify the host system on which the attributes are
        // compatible.  Software can use this information to determine
        // the line record format for text files etc.
        //
        // 4.4.2.2 The current mappings are:
        //
        //  0 - MS-DOS and OS/2 (FAT / VFAT / FAT32 file systems)
        //  1 - Amiga                     2 - OpenVMS
        //  3 - UNIX                      4 - VM/CMS
        //  5 - Atari ST                  6 - OS/2 H.P.F.S.
        //  7 - Macintosh                 8 - Z-System
        //  9 - CP/M                     10 - Windows NTFS
        // 11 - MVS (OS/390 - Z/OS)      12 - VSE
        // 13 - Acorn Risc               14 - VFAT
        // 15 - alternate MVS            16 - BeOS
        // 17 - Tandem                   18 - OS/400
        // 19 - OS X (Darwin)            20 thru 255 - unused
        match source_version >> 8 {
            0 => System::Dos,
            3 => System::Unix,
            _ => System::Unknown,
        }
    }
}

// Straight from the Rust docs:

/// Reads a little-endian u64 from the front of the provided slice, shrinking it.
fn read_u64(input: &mut &[u8]) -> u64 {
    let (int_bytes, rest) = input.split_at(std::mem::size_of::<u64>());
    *input = rest;
    u64::from_le_bytes(int_bytes.try_into().expect("less than eight bytes for u64"))
}

/// Reads a little-endian u32 from the front of the provided slice, shrinking it.
fn read_u32(input: &mut &[u8]) -> u32 {
    let (int_bytes, rest) = input.split_at(std::mem::size_of::<u32>());
    *input = rest;
    u32::from_le_bytes(int_bytes.try_into().expect("less than four bytes for u32"))
}

/// Reads a little-endian u16 from the front of the provided slice, shrinking it.
fn read_u16(input: &mut &[u8]) -> u16 {
    let (int_bytes, rest) = input.split_at(std::mem::size_of::<u16>());
    *input = rest;
    u16::from_le_bytes(int_bytes.try_into().expect("less than two bytes for u16"))
}

/// Data from the End of central directory record
///
/// Found at the back of the ZIP archive and provides offsets for finding
/// its central directory, along with lots of stuff that stopped being relevant
/// when we stopped breaking ZIP archives onto multiple floppies.
#[derive(Debug)]
pub struct EndOfCentralDirectory<'a> {
    pub disk_number: u16,
    pub disk_with_central_directory: u16,
    pub entries_on_this_disk: u16,
    pub entries: u16,
    pub central_directory_size: u32,
    pub central_directory_offset: u32,
    pub file_comment: &'a [u8],
}

impl<'a> EndOfCentralDirectory<'a> {
    pub fn parse(mut eocdr: &'a [u8]) -> ZipResult<Self> {
        // 4.3.16  End of central directory record:
        //
        // end of central dir signature    4 bytes  (0x06054b50)
        // number of this disk             2 bytes
        // number of the disk with the
        // start of the central directory  2 bytes
        // total number of entries in
        // the central dir on this disk    2 bytes
        // total number of entries in
        // the central dir                 2 bytes
        // size of the central directory   4 bytes
        // offset of start of central
        // directory with respect to
        // the starting disk number        4 bytes
        // zipfile comment length          2 bytes

        // Assert the magic instead of checking for it
        // because the search should have found it.
        assert_eq!(eocdr[..4], EOCDR_MAGIC);
        eocdr = &eocdr[4..];
        let disk_number = read_u16(&mut eocdr);
        let disk_with_central_directory = read_u16(&mut eocdr);
        let entries_on_this_disk = read_u16(&mut eocdr);
        let entries = read_u16(&mut eocdr);
        let central_directory_size = read_u32(&mut eocdr);
        let central_directory_offset = read_u32(&mut eocdr);
        let comment_length = read_u16(&mut eocdr);
        let file_comment = &eocdr[..usize(comment_length)?];

        Ok(Self {
            disk_number,
            disk_with_central_directory,
            entries_on_this_disk,
            entries,
            central_directory_size,
            central_directory_offset,
            file_comment,
        })
    }
}

/// Searches backward through `mapping` to find the
/// End of central directory record.
///
/// It should be right at the end of the file,
/// but its variable size means we can't jump to a known offset.
pub fn find_eocdr(mapping: &[u8]) -> ZipResult<usize> {
    memmem::rfind(mapping, &EOCDR_MAGIC).ok_or(ZipError::InvalidArchive(
        "Couldn't find End Of Central Directory Record",
    ))
}

/// Data from the Zip64 end of central directory locator
///
/// This should immediately precede the End of central directory record
/// on Zip64 files and tell us where to find the Zip64 end of central directory record.
#[derive(Debug)]
pub struct Zip64EndOfCentralDirectoryLocator {
    pub disk_with_central_directory: u32,
    pub zip64_eocdr_offset: u64,
    pub disks: u32,
}

impl Zip64EndOfCentralDirectoryLocator {
    pub fn parse(mut mapping: &[u8]) -> Option<Self> {
        // 4.3.15 Zip64 end of central directory locator
        //
        // zip64 end of central dir locator
        // signature                       4 bytes  (0x07064b50)
        // number of the disk with the
        // start of the zip64 end of
        // central directory               4 bytes
        // relative offset of the zip64
        // end of central directory record 8 bytes
        // total number of disks           4 bytes
        if mapping[..4] != ZIP64_EOCDR_LOCATOR_MAGIC {
            return None;
        }
        mapping = &mapping[4..];
        let disk_with_central_directory = read_u32(&mut mapping);
        let zip64_eocdr_offset = read_u64(&mut mapping);
        let disks = read_u32(&mut mapping);

        Some(Self {
            disk_with_central_directory,
            zip64_eocdr_offset,
            disks,
        })
    }

    pub fn size_in_file() -> usize {
        20
    }
}

/// Data from the Zip64 end of central directory record
///
/// This should immediately precede the "End of central directory" record
/// on Zip64 files and tell us where to find the Zip64 end of central directory record.
#[derive(Debug)]
pub struct Zip64EndOfCentralDirectory<'a> {
    pub source_version: u16,
    pub minimum_extract_version: u16,
    pub disk_number: u32,
    pub disk_with_central_directory: u32,
    pub entries_on_this_disk: u64,
    pub entries: u64,
    pub central_directory_size: u64,
    pub central_directory_offset: u64,
    pub extensible_data: &'a [u8],
}

impl<'a> Zip64EndOfCentralDirectory<'a> {
    pub fn parse(mut eocdr: &'a [u8]) -> ZipResult<Self> {
        // 4.3.14  Zip64 end of central directory record
        //
        // zip64 end of central dir
        // signature                       4 bytes  (0x06064b50)
        // size of zip64 end of central
        // directory record                8 bytes
        // version made by                 2 bytes
        // version needed to extract       2 bytes
        // number of this disk             4 bytes
        // number of the disk with the
        // start of the central directory  4 bytes
        // total number of entries in the
        // central directory on this disk  8 bytes
        // total number of entries in the
        // central directory               8 bytes
        // size of the central directory   8 bytes
        // offset of start of central
        // directory with respect to
        // the starting disk number        8 bytes
        // zip64 extensible data sector    (variable size)

        // Assert the magic instead of checking for it
        // because the search should have found it.
        assert_eq!(eocdr[..4], ZIP64_EOCDR_MAGIC);
        eocdr = &eocdr[4..];
        let eocdr_size = read_u64(&mut eocdr);
        let source_version = read_u16(&mut eocdr);
        let minimum_extract_version = read_u16(&mut eocdr);
        let disk_number = read_u32(&mut eocdr);
        let disk_with_central_directory = read_u32(&mut eocdr);
        let entries_on_this_disk = read_u64(&mut eocdr);
        let entries = read_u64(&mut eocdr);
        let central_directory_size = read_u64(&mut eocdr);
        let central_directory_offset = read_u64(&mut eocdr);

        // 4.3.14.1 The value stored into the "size of zip64 end of central
        // directory record" SHOULD be the size of the remaining
        // record and SHOULD NOT include the leading 12 bytes.
        //
        // Size = SizeOfFixedFields + SizeOfVariableData - 12.
        // (SizeOfVariableData = Size - SizeOfFixedFields + 12)

        // Check for underflow:
        let eocdr_size = usize(eocdr_size)?;
        if (eocdr_size + 12) < Self::fixed_size_in_file() {
            return Err(ZipError::InvalidArchive(
                "Invalid extensible data length in Zip64 End Of Central Directory Record",
            ));
        }
        // We should be left with just the extensible data:
        let extensible_data_length = eocdr_size + 12 - Self::fixed_size_in_file();
        if eocdr.len() != extensible_data_length {
            return Err(ZipError::InvalidArchive(
                "Invalid extensible data length in Zip64 End Of Central Directory Record",
            ));
        }
        let extensible_data = eocdr;

        Ok(Self {
            source_version,
            minimum_extract_version,
            disk_number,
            disk_with_central_directory,
            entries,
            entries_on_this_disk,
            central_directory_size,
            central_directory_offset,
            extensible_data,
        })
    }

    fn fixed_size_in_file() -> usize {
        56
    }
}

/// Finds the Zip64 end of central directory record in the given slice.
///
/// The slice should start at the Zip64 EOCDR's nominal location,
/// but we might have to do some searching since ZIP archives can have
/// arbitrary junk up front.
pub fn find_zip64_eocdr(mapping: &[u8]) -> ZipResult<usize> {
    memmem::find(mapping, &ZIP64_EOCDR_MAGIC).ok_or(ZipError::InvalidArchive(
        "Couldn't find zip64 End Of Central Directory Record",
    ))
}

/// Data from a central directory entry
///
/// Each of these records contians information about a file or folder
/// stored in the ZIP archive.
#[derive(Debug)]
pub struct CentralDirectoryEntry<'a> {
    pub source_version: u16,
    pub minimum_extract_version: u16,
    pub flags: u16,
    pub compression_method: u16,
    pub last_modified_time: u16,
    pub last_modified_date: u16,
    pub crc32: u32,
    pub compressed_size: u32,
    pub uncompressed_size: u32,
    pub disk_number: u16,
    pub internal_file_attributes: u16,
    pub external_file_attributes: u32,
    pub header_offset: u32,
    pub path: &'a [u8],
    pub extra_field: &'a [u8],
    pub file_comment: &'a [u8],
}

impl<'a> CentralDirectoryEntry<'a> {
    pub fn parse_and_consume(entry: &mut &'a [u8]) -> ZipResult<Self> {
        // 4.3.12  Central directory structure:
        //
        // [central directory header 1]
        // .
        // .
        // .
        // [central directory header n]
        // [digital signature]
        //
        // File header:
        //
        //   central file header signature   4 bytes  (0x02014b50)
        //   version made by                 2 bytes
        //   version needed to extract       2 bytes
        //   general purpose bit flag        2 bytes
        //   compression method              2 bytes
        //   last mod file time              2 bytes
        //   last mod file date              2 bytes
        //   crc-32                          4 bytes
        //   compressed size                 4 bytes
        //   uncompressed size               4 bytes
        //   file name length                2 bytes
        //   extra field length              2 bytes
        //   file comment length             2 bytes
        //   disk number start               2 bytes
        //   internal file attributes        2 bytes
        //   external file attributes        4 bytes
        //   relative offset of local header 4 bytes
        //
        //   file name (variable size)
        //   extra field (variable size)
        //   file comment (variable size)
        if entry[..4] != CENTRAL_DIRECTORY_MAGIC {
            return Err(ZipError::InvalidArchive("Invalid central directory entry"));
        }
        *entry = &entry[4..];
        let source_version = read_u16(entry);
        let minimum_extract_version = read_u16(entry);
        let flags = read_u16(entry);
        let compression_method = read_u16(entry);
        let last_modified_time = read_u16(entry);
        let last_modified_date = read_u16(entry);
        let crc32 = read_u32(entry);
        let compressed_size = read_u32(entry);
        let uncompressed_size = read_u32(entry);
        let path_length = usize(read_u16(entry))?;
        let extra_field_length = usize(read_u16(entry))?;
        let file_comment_length = usize(read_u16(entry))?;
        let disk_number = read_u16(entry);
        let internal_file_attributes = read_u16(entry);
        let external_file_attributes = read_u32(entry);
        let header_offset = read_u32(entry);
        let (path, remaining) = entry.split_at(path_length);
        let (extra_field, remaining) = remaining.split_at(extra_field_length);
        let (file_comment, remaining) = remaining.split_at(file_comment_length);
        *entry = remaining;

        Ok(Self {
            source_version,
            minimum_extract_version,
            flags,
            compression_method,
            last_modified_time,
            last_modified_date,
            crc32,
            compressed_size,
            uncompressed_size,
            disk_number,
            internal_file_attributes,
            external_file_attributes,
            header_offset,
            path,
            extra_field,
            file_comment,
        })
    }
}

/// Extracts the "is this text UTF-8?" bit from the 16-bit flags field.
///
/// If false, text is assumped to be CP437.
fn is_utf8(flags: u16) -> bool {
    // Bit 11: Language encoding flag (EFS).  If this bit is set,
    //         the filename and comment fields for this file
    //         MUST be encoded using UTF-8. (see APPENDIX D)
    flags & (1 << 11) != 0
}

/// Extracts the "is this file encrypted?" bit from the 16-bit flags field.
fn is_encrypted(flags: u16) -> bool {
    // Bit 0: If set, indicates that the file is encrypted
    flags & 1 != 0
}

impl<'a> FileMetadata<'a> {
    /// Extracts `FileMetadata` from a central directory entry
    pub(crate) fn from_cde(cde: &CentralDirectoryEntry<'a>) -> ZipResult<Self> {
        let is_utf8 = is_utf8(cde.flags);

        let path: Cow<Path> = if is_utf8 {
            let utf8 = std::str::from_utf8(cde.path).map_err(ZipError::Encoding)?;
            Cow::Borrowed(Path::new(utf8))
        } else {
            let str_cow: Cow<str> = Cow::borrow_from_cp437(cde.path, &CP437_CONTROL);
            // Annoying: doesn't seem to be any Cow<str> -> Cow<Path>
            match str_cow {
                Cow::Borrowed(s) => Cow::Borrowed(Path::new(s)),
                Cow::Owned(s) => Cow::Owned(s.into()),
            }
        };

        if cde.disk_number != 0 {
            return Err(ZipError::UnsupportedArchive(format!(
                "No support for multi-disk archives: file {} claims to be on disk {}",
                path.display(),
                cde.disk_number,
            )));
        }

        let encrypted = is_encrypted(cde.flags);
        /* When we try to read; don't bomb if the archive has _any_ encrypted file
        if encrypted {
            return Err(ZipError::UnsupportedArchive(format!(
                "No support for encrypted files, as {} claims to be",
                path
            )));
        }
        */

        let compression_method = CompressionMethod::from_u16(cde.compression_method);

        let mut metadata = Self {
            size: usize(cde.uncompressed_size)?,
            compressed_size: usize(cde.compressed_size)?,
            compression_method,
            crc32: cde.crc32,
            encrypted,
            path,
            last_modified: parse_msdos(cde.last_modified_time, cde.last_modified_date),
            header_offset: usize(cde.header_offset)?,
        };

        parse_extra_field(&mut metadata, cde.extra_field)?;

        Ok(metadata)
    }

    /// Extract metadata from a local file header.
    ///
    /// Since the local header doesn't contain the offset
    /// (we're at it already if we're reading the thing),
    /// take the CDE-provided offset as an argument.
    pub(crate) fn from_local_header(
        local: &LocalFileHeader<'a>,
        header_offset: usize,
    ) -> ZipResult<Self> {
        let is_utf8 = is_utf8(local.flags);

        let path: Cow<Path> = if is_utf8 {
            let utf8 = std::str::from_utf8(local.path).map_err(ZipError::Encoding)?;
            Cow::Borrowed(Path::new(utf8))
        } else {
            let str_cow: Cow<str> = Cow::borrow_from_cp437(local.path, &CP437_CONTROL);
            // Annoying: doesn't seem to be any Cow<str> -> Cow<Path>
            match str_cow {
                Cow::Borrowed(s) => Cow::Borrowed(Path::new(s)),
                Cow::Owned(s) => Cow::Owned(s.into()),
            }
        };

        let encrypted = is_encrypted(local.flags);

        let compression_method = CompressionMethod::from_u16(local.compression_method);

        let mut metadata = Self {
            size: usize(local.uncompressed_size)?,
            compressed_size: usize(local.compressed_size)?,
            compression_method,
            crc32: local.crc32,
            encrypted,
            path,
            last_modified: parse_msdos(local.last_modified_time, local.last_modified_date),
            header_offset,
        };

        parse_extra_field(&mut metadata, local.extra_field)?;

        Ok(metadata)
    }
}

fn parse_msdos(time: u16, date: u16) -> NaiveDateTime {
    let seconds = (0b0000_0000_0001_1111 & time) as u32 * 2; // MSDOS uses 2-second precision
    let minutes = (0b0000_0111_1110_0000 & time) as u32 >> 5;
    let hours = (0b1111_1000_0000_0000 & time) as u32 >> 11;

    let days = (0b0000_0000_0001_1111 & date) as u32;
    let months = (0b0000_0001_1110_0000 & date) as u32 >> 5;
    // MSDOS uses years since 1980; Always interpreted as a positive value
    let years = ((0b1111_1110_0000_0000 & date) >> 9) as i32 + 1980;

    NaiveDate::from_ymd(years, months, days).and_hms(hours, minutes, seconds)
}

/// Parses the "extra fields" found in central directory entries
/// and local file headers.
///
/// Currently we just look for Zip64 info (64-bit values for files > 2^32 in size)
fn parse_extra_field(metadata: &mut FileMetadata, mut extra_field: &[u8]) -> ZipResult<()> {
    // 4.5.1 In order to allow different programs and different types
    // of information to be stored in the 'extra' field in .ZIP
    // files, the following structure MUST be used for all
    // programs storing data in this field:

    //     header1+data1 + header2+data2 . . .

    // Each header MUST consist of:

    //     Header ID - 2 bytes
    //     Data Size - 2 bytes
    while !extra_field.is_empty() {
        let kind = read_u16(&mut extra_field);
        let field_len = read_u16(&mut extra_field);

        let mut amount_left = field_len as i16;
        // Zip64 extended information extra field
        if kind == 0x0001 {
            if metadata.size == u32::MAX as usize {
                metadata.size = usize(read_u64(&mut extra_field))?;
                amount_left -= 8;
            }
            if metadata.compressed_size == u32::MAX as usize {
                metadata.compressed_size = usize(read_u64(&mut extra_field))?;
                amount_left -= 8;
            }
            if metadata.header_offset == u32::MAX as usize {
                metadata.header_offset = usize(read_u64(&mut extra_field))?;
                amount_left -= 8;
            }
            // We already checked many times that this isn't a multi-disk archive.
            if amount_left != 0 {
                return Err(ZipError::InvalidArchive(
                    "Extra data field contains disk number",
                ));
            }
        }
        extra_field = &extra_field[amount_left as usize..];
    }
    Ok(())
}

/// Data from a local file header
///
/// Each files' actual contents is preceded by this header.
/// These headers alllow for "streaming" decompression without
/// the use of the central directory,
/// but we don't make use of this feature.
#[derive(Debug)]
pub struct LocalFileHeader<'a> {
    pub minimum_extract_version: u16,
    pub flags: u16,
    pub compression_method: u16,
    pub last_modified_time: u16,
    pub last_modified_date: u16,
    pub crc32: u32,
    pub compressed_size: u32,
    pub uncompressed_size: u32,
    pub path: &'a [u8],
    pub extra_field: &'a [u8],
}

impl<'a> LocalFileHeader<'a> {
    pub fn parse_and_consume(header: &mut &'a [u8]) -> ZipResult<Self> {
        // 4.3.7  Local file header:
        //
        // local file header signature     4 bytes  (0x04034b50)
        // version needed to extract       2 bytes
        // general purpose bit flag        2 bytes
        // compression method              2 bytes
        // last mod file time              2 bytes
        // last mod file date              2 bytes
        // crc-32                          4 bytes
        // compressed size                 4 bytes
        // uncompressed size               4 bytes
        // file name length                2 bytes
        // extra field length              2 bytes
        //
        // file name (variable size)
        // extra field (variable size)
        assert_eq!(header[..4], LOCAL_FILE_HEADER_MAGIC);
        *header = &header[4..];
        let minimum_extract_version = read_u16(header);
        let flags = read_u16(header);
        let compression_method = read_u16(header);
        let last_modified_time = read_u16(header);
        let last_modified_date = read_u16(header);
        let crc32 = read_u32(header);
        let compressed_size = read_u32(header);
        let uncompressed_size = read_u32(header);
        let path_length = usize(read_u16(header))?;
        let extra_field_length = usize(read_u16(header))?;
        let (path, remaining) = header.split_at(path_length);
        let (extra_field, remaining) = remaining.split_at(extra_field_length);
        *header = remaining;

        Ok(Self {
            minimum_extract_version,
            flags,
            compression_method,
            last_modified_time,
            last_modified_date,
            crc32,
            compressed_size,
            uncompressed_size,
            path,
            extra_field,
        })
    }
}
