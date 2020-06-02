use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::*;
use log::*;
use memmap::Mmap;
use structopt::*;

use piz::read::ZipArchive;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "zip_probe",
    about = "Examines a .zip file and reads its contents into the void"
)]
struct Opt {
    /// Pass multiple times for additional verbosity (info, debug, trace)
    #[structopt(short, long, parse(from_occurrences))]
    verbosity: usize,

    #[structopt(name("ZIP file"))]
    zip_path: PathBuf,
}

fn main() -> Result<()> {
    let args = Opt::from_args();

    let mut errlog = stderrlog::new();
    errlog.verbosity(args.verbosity + 1);
    errlog.init()?;

    read_zip(&args.zip_path)
}

fn read_zip(zip_path: &Path) -> Result<()> {
    info!("Memory mapping {:#?}", zip_path);
    let zip_file = File::open(zip_path).context("Couldn't open zip file")?;
    let mapping = unsafe { Mmap::map(&zip_file).context("Couldn't mmap zip file")? };

    let archive = ZipArchive::with_prepended_data(&mapping).context("Couldn't load archive")?.0;
    for entry in archive.entries() {
        let mut reader = archive.read(entry)?;
        let mut sink = io::sink();
        io::copy(&mut reader, &mut sink)?;
    }
    Ok(())
}
