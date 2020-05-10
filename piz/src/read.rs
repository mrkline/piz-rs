use log::*;

use crate::arch::usize;
use crate::result::*;
use crate::spec;

// Move types into some submodule if we have a handful?
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum CompressionMethod {
    None,
    Deflate,
    Unsupported(u16)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum System
{
    Dos,
    Unix,
    Unknown,
}

impl System {
    pub fn from_u8(system: u8) -> System
    {
        use self::System::*;

        match system {
            0 => Dos,
            3 => Unix,
            _ => Unknown,
        }
    }
}

pub struct ZipArchive<'a> {
    mapping: &'a [u8],
    entries: Vec<spec::FileMetadata<'a>>,
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

            let file_metadata = spec::FileMetadata::from_cde(&dir_entry)?;
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
}
