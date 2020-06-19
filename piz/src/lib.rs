//! piz is a Zip archive reader designed to decompress any number of files
//! concurrently using a simple API:
//!
//! ```rust
//! // For smaller files,
//! //
//! //     let bytes = fs::read("foo.zip")
//! //     let archive = ZipArchive::new(&bytes)?;
//! //
//! // works just fine. For larger ones, memory map!
//! let zip_file = File::open("foo.zip")?;
//! let mapping = unsafe { Mmap::map(&zip_file)? };
//! let archive = ZipArchive::new(&mapping)?;
//!
//! // Look, ma, reading files in parallel! Using Rayon:
//! archive.entries().into_par_iter().try_for_each(|entry| {
//!     let mut reader = archive.read(entry)?;
//!     // reader implements Read, so read away!
//! });
//!
//! // If you don't care about parallelism, a simple loop will do:
//! //     for entry in archive.entries() {
//! //         let mut reader = archive.read(entry)?;
//! //         // Read away!
//! //     }
//! ```
//!
//! Zip is an interesting archive format: unlike compressed tarballs often seen
//! in Linux land (`*.tar.gz`, `*.tar.zst`, ...),
//! each file in a Zip archive is compressed independently,
//! with a central directory  telling us where to find each file.
//! This allows us to extract multiple files simultaneously so long as we can
//! read from multiple places at once.
//!
//! Users can either read the entire archive into memory, or, for larger archives,
//! [memory-map](https://docs.rs/memmap/0.7.0/memmap/struct.Mmap.html) the file.
//! (On 64-bit systems, this allows us to treat archives as a contiguous byte range
//! even if the file is _much_ larger than physical RAM. 32-bit systems are limited
//! by address space to archives under 4 GB, but piz _should_ be well-behaved
//! if the archive is small enough.)

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
