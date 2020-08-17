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
// works just fine. Memory map larger files!
let zip_file = File::open("foo.zip")?;
let mapping = unsafe { Mmap::map(&zip_file)? };
let archive = ZipArchive::new(&mapping)?;

// We can iterate through the entries in the archive directly...
//
//     for entry in archive.entries() {
//         let mut reader = archive.read(entry)?;
//         // Read away!
//     }
//
// ...but ZIP doesn't guarantee that entries are in any particular order,
// that there aren't duplicates, that an entry has a valid file path, etc.
// Let's do some validation and organize them into a tree of files and folders.
let tree = FileTree::new(archive.entries())?;

// With that done, we can get a file (or directory)'s metadata from its path.
let metadata = tree.get("some/specific/file")?;
// And read the file out, if we'd like:
let mut reader = archive.read(metadata)?;
let mut save_to = File::create(&metadata.file_name)?;
io::copy(&mut reader, &mut save_to)?;

// Readers are `Send`, so we can read out as many as we'd like in parallel.
// Here we'll use Rayon to read out the whole archive with all cores:
tree.files()
    .collect::<Vec<_>>() // `.into_par_iter()` works on a collection.
    .into_par_iter()
    .try_for_each(|entry| {
        if let Some(parent) = entry.file_name.parent() {
            // Create parent directories as needed.
            fs::create_dir_all(parent)?;
        }
        let mut reader = archive.read(entry)?;
        let mut sink = File::create(&entry.file_name)?;
        io::copy(&mut reader, &mut sink)?;
        Ok(())
    })?;
```

Zip is an interesting archive format: unlike compressed tarballs often seen
in Linux land (`*.tar.gz`, `*.tar.zst`, ...),
each file in a Zip archive is compressed independently,
with a central directory telling us where to find each file.
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
