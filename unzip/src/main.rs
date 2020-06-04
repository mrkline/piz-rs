use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use anyhow::*;
use log::*;
use memmap::Mmap;
use structopt::*;

use piz::read::ZipArchive;

#[derive(Debug, StructOpt)]
#[structopt(name = "unzip", about = "Dumps a .zip file into the current directory")]
struct Opt {
    /// Pass multiple times for additional verbosity (info, debug, trace)
    #[structopt(short, long, parse(from_occurrences))]
    verbosity: usize,

    /// Change to the given directory before perfoming any operations.
    #[structopt(short = "C", long)]
    directory: Option<PathBuf>,

    #[structopt(name("ZIP file"))]
    zip_path: PathBuf,
}

fn main() -> Result<()> {
    let args = Opt::from_args();

    let mut errlog = stderrlog::new();
    errlog.verbosity(args.verbosity + 1);
    errlog.init()?;

    if let Some(chto) = args.directory {
        std::env::set_current_dir(&chto)
            .with_context(|| format!("Couldn't set working directory to {}", chto.display()))?;
    }

    read_zip(&args.zip_path)
}

fn read_zip(zip_path: &Path) -> Result<()> {
    info!("Memory mapping {:#?}", zip_path);
    let zip_file = File::open(zip_path).context("Couldn't open zip file")?;
    let mapping = unsafe { Mmap::map(&zip_file).context("Couldn't mmap zip file")? };

    let archive = ZipArchive::with_prepended_data(&mapping)
        .context("Couldn't load archive")?
        .0;
    for entry in archive.entries() {
        if entry.is_dir() {
            fs::create_dir_all(&entry.file_name).with_context(|| {
                format!("Couldn't create directory {}", entry.file_name.display())
            })?;
        } else {
            let mut reader = archive.read(entry)?;
            let mut sink = File::create(&entry.file_name)
                .with_context(|| format!("Couldn't create file {}", entry.file_name.display()))?;
            io::copy(&mut reader, &mut sink)?;
        }
    }
    Ok(())
}
