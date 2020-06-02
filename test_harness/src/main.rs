use std::fs::File;
use std::io;
use std::path::Path;
use std::process::Command;

use anyhow::*;
use log::*;
use memmap::Mmap;
use rayon::prelude::*;
use structopt::*;

use piz::read::ZipArchive;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "piz_testbench",
    about = "PIZ (Parallel Implementation of Zip) tests"
)]
struct Opt {
    /// Pass multiple times for additional verbosity (info, debug, trace)
    #[structopt(short, long, parse(from_occurrences))]
    verbosity: usize,
}

fn main() -> Result<()> {
    let args = Opt::from_args();

    let mut errlog = stderrlog::new();
    errlog.verbosity(args.verbosity + 1);
    errlog.init()?;

    let inputs: Vec<&Path> = [
        "inputs/hello.zip",
        "inputs/hello-prefixed.zip",
        "inputs/zip64.zip",
    ]
    .iter()
    .map(|p| Path::new(p))
    .collect();

    if inputs.iter().any(|i| !i.exists()) {
        Command::new("./create-inputs.sh")
            .status()
            .expect("Couldn't set up input files");
    }

    for input in inputs {
        read_zip(input)?;
    }

    Ok(())
}

fn read_zip(zip_path: &Path) -> Result<()> {
    info!("Memory mapping {:#?}", zip_path);
    let zip_file = File::open(zip_path).context("Couldn't open zip file")?;
    let mapping = unsafe { Mmap::map(&zip_file).context("Couldn't mmap zip file")? };

    let archive = ZipArchive::with_prepended_data(&mapping).context("Couldn't load archive")?.0;
    let readers: Vec<_> = archive
        .entries()
        .iter()
        .map(|e| archive.read(e).unwrap())
        .collect();
    // Look, ma, readers are Send so we can read several in parallel!
    readers
        .into_par_iter()
        .try_for_each::<_, Result<()>>(|mut reader| {
            let mut sink = io::sink();
            io::copy(&mut reader, &mut sink)?;
            Ok(())
        })
}
