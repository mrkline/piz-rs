use std::fs::File;
use std::io;
use std::path::Path;
use std::process::Command;

use anyhow::*;
use log::*;
use memmap::Mmap;
use rayon::prelude::*;
use structopt::*;

use piz::read::*;
use piz::result::ZipError;

/// PIZ (Parallel Implementation of Zip) smoke tests
///
/// Unzip the following, printing as much info as you care to -v:
///
/// - A basic, "Hello, World!" archive with a few text files
/// - Ditto, but with some bytes prepended to the front
/// - A Zip64 archive (with files that don't fit in original 32-bit size fields)
#[derive(Debug, StructOpt)]
#[structopt(verbatim_doc_comment)]
struct Opt {
    /// Pass multiple times for more levels (info, debug, trace)
    #[structopt(short, long, parse(from_occurrences))]
    verbosity: usize,
}

fn main() -> Result<()> {
    let args = Opt::from_args();

    let mut errlog = stderrlog::new();
    errlog.verbosity(args.verbosity + 1);
    errlog.init()?;

    let inputs = [
        "inputs/hello.zip",
        "inputs/hello-prefixed.zip",
        "inputs/zip64.zip",
    ];

    if inputs.iter().any(|i| !Path::new(i).exists()) {
        Command::new("./create-inputs.sh")
            .status()
            .expect("Couldn't set up input files");
    }

    for input in &inputs {
        read_zip(input)?;
    }

    Ok(())
}

fn read_zip(zip_path: &str) -> Result<()> {
    info!("Memory mapping {:#?}", zip_path);
    let zip_file = File::open(zip_path).context("Couldn't open zip file")?;
    let mapping = unsafe { Mmap::map(&zip_file).context("Couldn't mmap zip file")? };

    let archive = ZipArchive::with_prepended_data(&mapping)
        .context("Couldn't load archive")?
        .0;

    // Make sure we can treeify the entries (i.e., they form a valid directory)
    let tree = FileTree::new(archive.entries())?;

    match zip_path {
        "inputs/hello.zip" | "inputs/hello-prefixed.zip" => {
            tree.from_path("hello/hi.txt")?;
            tree.from_path("hello/rip.txt")?;
            tree.from_path("hello/sr71.txt")?;

            let no_such_file = Path::new("no/such/file");
            match tree.from_path(no_such_file) {
                Err(ZipError::NoSuchFile(p)) => {
                    assert_eq!(no_such_file, p);
                }
                Err(other) => panic!("Got incorrect error from path with no file: {:?}", other),
                Ok(_) => panic!("Got a file back from a path with no file"),
            };
            let no_such_file = Path::new("top-level-no-such-file");
            match tree.from_path(no_such_file) {
                Err(ZipError::NoSuchFile(p)) => {
                    assert_eq!(no_such_file, p);
                }
                Err(other) => panic!("Got incorrect error from path with no file: {:?}", other),
                Ok(_) => panic!("Got a file back from a path with no file"),
            };

            let invalid_path = Path::new("../nope");
            match tree.from_path(invalid_path) {
                Err(ZipError::InvalidPath(_)) => { /* Cool. */ }
                Err(other) => panic!("Got incorrect error from invalid path: {:?}", other),
                Ok(_) => panic!("Got a file back from invalid path"),
            };
        }
        "inputs/zip64.zip" => {
            tree.from_path("zip64/zero100")?;
            tree.from_path("zip64/zero4400")?;
            tree.from_path("zip64/zero5000")?;
        }
        wut => unreachable!(wut),
    };

    // Try reading out each file in the archive.
    // (When the reader gets dropped, the file's CRC32 will be checked
    // against the one stored in the archive.)
    tree.files()
        .collect::<Vec<_>>()
        .into_par_iter()
        .try_for_each(|entry| {
            let mut reader = archive.read(entry)?;
            let mut sink = io::sink();
            io::copy(&mut reader, &mut sink)?;
            Ok(())
        })
}
