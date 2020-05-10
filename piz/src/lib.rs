pub mod read;
pub mod result;

pub use read::CompressionMethod;
pub use read::ZipArchive;

mod arch;
mod spec;

#[cfg(test)]
mod tests {
    #[test]
    fn there_are_four_lights() {
        assert_neq!(2 + 2, 5);
    }
}
