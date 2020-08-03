# piz: A Parallel Implementation of Zip (in Rust)

![CI status](https://github.com/mrkline/piz-rs/workflows/CI/badge.svg)

piz is a Zip archive reader designed to decompress any number of files
concurrently using a simple API:
```rust
// For smaller files,
//
//     let bytes = fs::read("foo.zip")
//     let archive = ZipArchive::new(&bytes)?;
//
// works just fine. For larger ones, memory map!
let zip_file = File::open("foo.zip")?;
let mapping = unsafe { Mmap::map(&zip_file)? };
let archive = ZipArchive::new(&mapping)?;

// Look, ma, reading files in parallel! Using Rayon:
archive.entries().into_par_iter().try_for_each(|entry| {
    let mut reader = archive.read(entry)?;
    // reader implements Read, so read away!
});

// If you don't care about parallelism, a simple loop will do:
//     for entry in archive.entries() {
//         let mut reader = archive.read(entry)?;
//         // Read away!
//     }

// If you want to look up entries by name,
// arrange them in a tree of directories and files, which:
// - Simplifies lookup
// - Validates the archive, making sure each `FileMetadata` has a valid path,
//   no duplicates, etc.
let tree = treeify(archive.entries())?;
let metadata = metadata_from_path("some/specific/file", &tree)?;
```

Zip is an interesting archive format: unlike compressed tarballs often seen
in Linux land (`*.tar.gz`, `*.tar.zst`, ...),
each file in a Zip archive is compressed independently,
with a central directory  telling us where to find each file.
This allows us to extract multiple files simultaneously so long as we can
read from multiple places at once.

Users can either read the entire archive into memory, or, for larger archives,
[memory-map](https://docs.rs/memmap/0.7.0/memmap/struct.Mmap.html) the file.
(On 64-bit systems, this allows us to treat archives as a contiguous byte range
even if the file is _much_ larger than physical RAM. 32-bit systems are limited
by address space to archives under 4 GB, but piz _should_ be well-behaved
if the archive is small enough.)

## Examples

See `unzip/` for a simple CLI example that unzips a provided file
into the current directory.

## Tests

`test_harness/` contains some smoke tests against a few inputs, e.g.:

- A basic, "Hello, Zip!" archive of a few text files
- The same, but with some junk prepended to it
- A Zip64 archive with files > 2^32 bytes

If it doesn't find these files, it creates them with a shell script
(which assumes a Unix-y environment).


## Future plans

Currently piz provides very limited metadata (file name, size, CRC32, etc.).
Additional data (permissions, last-modified time, etc.) should be added later.
Support for compression algorithms besides DEFLATE (like Bzip2) could also be added.

## Thanks

Many thanks to

- Hans Wennborg for their fantastic article,
  [Zip Files: History, Explanation and Implementation](https://www.hanshq.net/zip.html)

- Mathijs van de Nes's [zip-rs](https://github.com/mvdnes/zip-rs),
  the main inspiration of this project and a great example of a
  Zip decoder in Rust
