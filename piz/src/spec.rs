use std::convert::TryInto;

use twoway::{find_bytes, rfind_bytes};

use crate::arch::usize;
use crate::result::*;

const EOCDR_MAGIC: [u8; 4] = [b'P', b'K', 5, 6];
const ZIP64_EOCDR_MAGIC: [u8; 4] = [b'P', b'K', 6, 6];
const ZIP64_EOCDR_LOCATOR_MAGIC: [u8; 4] = [b'P', b'K', 6, 7];

// Straight from the Rust docs:
fn read_u64(input: &mut &[u8]) -> u64 {
    let (int_bytes, rest) = input.split_at(std::mem::size_of::<u64>());
    *input = rest;
    u64::from_le_bytes(int_bytes.try_into().expect("less than eight bytes for u64"))
}

fn read_u32(input: &mut &[u8]) -> u32 {
    let (int_bytes, rest) = input.split_at(std::mem::size_of::<u32>());
    *input = rest;
    u32::from_le_bytes(int_bytes.try_into().expect("less than four bytes for u32"))
}

fn read_u16(input: &mut &[u8]) -> u16 {
    let (int_bytes, rest) = input.split_at(std::mem::size_of::<u16>());
    *input = rest;
    u16::from_le_bytes(int_bytes.try_into().expect("less than two bytes for u16"))
}

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

pub fn find_eocdr(mapping: &[u8]) -> ZipResult<usize> {
    rfind_bytes(mapping, &EOCDR_MAGIC).ok_or(ZipError::InvalidArchive(
        "Couldn't find End Of Central Directory Record",
    ))
}

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

pub fn find_zip64_eocdr(mapping: &[u8]) -> ZipResult<usize> {
    find_bytes(mapping, &ZIP64_EOCDR_MAGIC).ok_or(ZipError::InvalidArchive(
        "Couldn't find zip64 End Of Central Directory Record",
    ))
}
