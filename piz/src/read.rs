use std::fs::File;

use memmap::Mmap;
use twoway::rfind_bytes;

use crate::result::*;

pub struct ZipArchive {
    mapping: Mmap,
}

impl ZipArchive {
    pub fn new(file: &File) -> ZipResult<Self> {
        let mapping = unsafe { Mmap::map(file)? };

        find_eocdr(&mapping)?;

        Ok(ZipArchive { mapping })
    }
}

fn find_eocdr(mapping: &[u8]) -> ZipResult<usize> {
    let magic = &[b'P', b'K', 5, 6];
    rfind_bytes(mapping, magic).ok_or(ZipError::InvalidArchive(
        "Couldn't find End Of Central Directory Record",
    ))
}
