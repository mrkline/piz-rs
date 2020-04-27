use std::fs::File;

use log::*;
use memmap::Mmap;

use crate::arch::usize;
use crate::result::*;
use crate::spec;

pub struct ZipArchive {
    mapping: Mmap,
    archive_offset: usize,
}

impl ZipArchive {
    pub fn new(file: &File) -> ZipResult<Self> {
        let mapping = unsafe { Mmap::map(file)? };

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

        // Zip files can be prepended by arbitrary junk,
        // so all the given positions might be off.
        // Calculate the offset.
        let archive_offset;

        let zip64_eocdr_locator_posit = eocdr_posit - 20;
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
            let zip64_eocdr_search_end = eocdr_posit - zip64_eocdr_locator.size_in_file();
            let zip64_eocdr_search_space =
                &mapping[zip64_eocdr_search_start..zip64_eocdr_search_end];

            let zip64_eocdr_posit = spec::find_zip64_eocdr(zip64_eocdr_search_space)?;
            // Since we're searching starting at the provided offset,
            // the returned position is the archive offset.
            archive_offset = zip64_eocdr_posit;
            let zip64_eocdr = spec::Zip64EndOfCentralDirectory::parse(
                &zip64_eocdr_search_space[zip64_eocdr_posit..],
            );
            trace!("{:?}", zip64_eocdr);
        } else {
            // The offset is the actual position versus the stored one.
            let actual_cdr_posit = eocdr_posit.checked_sub(usize(eocdr.central_directory_size)?);
            let nominal_offset = usize(eocdr.central_directory_offset)?;
            archive_offset = actual_cdr_posit
                .and_then(|off| off.checked_sub(nominal_offset))
                .ok_or(ZipError::InvalidArchive(
                    "Invalid central directory size or offset",
                ))?;
        }

        trace!("Archive prepended with {} bytes of junk", archive_offset);

        Ok(ZipArchive {
            mapping,
            archive_offset,
        })
    }
}
