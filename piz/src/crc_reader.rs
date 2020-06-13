//! Helper module to compute a CRC32 checksum
//!
//! Borrowed from zip-rs:
//! <https://github.com/mvdnes/zip-rs/commit/b3c836d9c32efa120cdd5366280f940d3c3b985c>

use std::io;
use std::io::prelude::*;

use crc32fast::Hasher;

/// Reader that validates the CRC32 when it reaches the EOF.
pub struct Crc32Reader<R> {
    inner: R,
    hasher: Hasher,
    provided_checksum: u32,
}

impl<R> Crc32Reader<R> {
    pub fn new(inner: R, provided_checksum: u32) -> Crc32Reader<R> {
        Crc32Reader {
            inner,
            hasher: Hasher::new(),
            provided_checksum,
        }
    }

    /// Returns true if the final checksum matches the one provided by `new()`
    fn check_matches(&self) -> bool {
        self.provided_checksum == self.hasher.clone().finalize()
    }
}

impl<R: Read> Read for Crc32Reader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let count = match self.inner.read(buf) {
            Ok(0) if !buf.is_empty() && !self.check_matches() => {
                return Err(io::Error::new(io::ErrorKind::Other, "Invalid checksum"))
            }
            Ok(n) => n,
            Err(e) => return Err(e),
        };
        self.hasher.update(&buf[0..count]);
        Ok(count)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::io::Read;

    #[test]
    fn test_empty_reader() {
        let data: &[u8] = b"";
        let mut buf = [0; 1];

        let mut reader = Crc32Reader::new(data, 0);
        assert_eq!(reader.read(&mut buf).unwrap(), 0);

        let mut reader = Crc32Reader::new(data, 1);
        assert!(reader
            .read(&mut buf)
            .unwrap_err()
            .to_string()
            .contains("Invalid checksum"));
    }

    #[test]
    fn test_byte_by_byte() {
        let data: &[u8] = b"1234";
        let mut buf = [0; 1];

        let mut reader = Crc32Reader::new(data, 0x9be3e0a3);
        assert_eq!(reader.read(&mut buf).unwrap(), 1);
        assert_eq!(reader.read(&mut buf).unwrap(), 1);
        assert_eq!(reader.read(&mut buf).unwrap(), 1);
        assert_eq!(reader.read(&mut buf).unwrap(), 1);
        assert_eq!(reader.read(&mut buf).unwrap(), 0);
        // Can keep reading 0 bytes after the end
        assert_eq!(reader.read(&mut buf).unwrap(), 0);
    }

    #[test]
    fn test_zero_read() {
        let data: &[u8] = b"1234";
        let mut buf = [0; 5];

        let mut reader = Crc32Reader::new(data, 0x9be3e0a3);
        assert_eq!(reader.read(&mut buf[..0]).unwrap(), 0);
        assert_eq!(reader.read(&mut buf).unwrap(), 4);
    }
}
