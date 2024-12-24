use std::fs::{self, File};
use std::io;
use std::path::PathBuf;

use anyhow::*;
use log::*;
use memmap2::Mmap;
use rayon::prelude::*;
use structopt::*;

use piz::read::*;

#[derive(Debug, StructOpt)]
#[structopt(name = "unzip", about = "Dumps a .zip file into the current directory")]
struct Opt {
    /// Pass multiple times for additional verbosity (info, debug, trace)
    #[structopt(short, long, parse(from_occurrences))]
    verbosity: usize,

    /// Change to the given directory before perfoming any operations.
    #[structopt(short = "C", long)]
    directory: Option<PathBuf>,

    /// Prints the tree of files in the ZIP archive instead of extracting them.
    #[structopt(short = "n", long)]
    dry_run: bool,

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

    info!("Memory mapping {:#?}", &args.zip_path);
    let zip_file = File::open(&args.zip_path).context("Couldn't open zip file")?;
    let mapping = unsafe { Mmap::map(&zip_file).context("Couldn't mmap zip file")? };

    let archive = ZipArchive::with_prepended_data(&mapping)
        .context("Couldn't load archive")?
        .0;
    let tree = as_tree(archive.entries())?;

    if args.dry_run {
        print_tree(&tree)
    } else {
        read_zip(&tree, &archive)
    }
}

fn print_tree(tree: &DirectoryContents) -> Result<()> {
    for entry in tree.traverse() {
        println!("{}", entry.metadata().path);
    }
    Ok(())
}

fn read_zip(tree: &DirectoryContents, archive: &ZipArchive) -> Result<()> {
    tree.files().par_bridge().try_for_each(|entry| {
        if let Some(parent) = entry.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Couldn't create directory {}", parent))?;
        }
        let mut reader = archive.read(entry)?;
        let mut sink = File::create(&*entry.path)
            .with_context(|| format!("Couldn't create file {}", entry.path))?;
        io::copy(&mut reader, &mut sink)?;
        Ok(())
    })
}
