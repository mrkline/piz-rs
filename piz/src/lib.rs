pub mod read;
pub mod result;

pub use read::CompressionMethod;
pub use read::ZipArchive;

mod arch;
mod crc_reader;
mod spec;

#[cfg(test)]
mod tests {
    #[test]
    fn there_are_four_lights() {
        assert_ne!(2 + 2, 5);
    }
}
