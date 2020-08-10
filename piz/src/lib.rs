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
//! // We can iterate through the entries in the archive directly...
//! //
//! //     for entry in archive.entries() {
//! //         let mut reader = archive.read(entry)?;
//! //         // Read away!
//! //     }
//! //
//! // ...but ZIP doesn't guarantee that entries are in any particular order,
//! // that there aren't duplicates, that an entry's has a valid file path, etc.
//! // Let's arrange them into a tree of directories and files, which performs
//! // some validation and makes files easier to look up.
//! let tree = FileTree::new(archive.entries())?;
//!
//! // With that done, we can get a file (or directory)'s metadata from its path.
//! let metadata = tree.from_path("some/specific/file", &tree)?;
//! // And read it out, if we'd like:
//! let mut reader = archive.read(metadata)?;
//! let mut save_to = File::create(&metadata.file_name)?;
//! io::copy(&mut reader, &mut sink)?;
//!
//! // Readers are `Send`, so we can read out as many as we'd like.
//! // Here we'll use Rayon to read out the whole archive:
//! // TODO
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
