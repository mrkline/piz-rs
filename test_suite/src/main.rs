use std::env;
use std::fs::File;

use anyhow::*;
use memmap::Mmap;
use rayon::prelude::*;

use piz::read::ZipArchive;

fn main() -> Result<()> {
    let args: Vec<_> = env::args().collect();

    if args.len() != 2 {
        bail!("Usage: test_suite <zip file>");
    }

    let mut errlog = stderrlog::new();
    errlog.verbosity(4);
    errlog.init()?;

    let zip_path = &args[1];
    println!("{}", zip_path);

    let zip_file = File::open(zip_path).context("Couldn't open zip file")?;
    let mapping = unsafe { Mmap::map(&zip_file).context("Couldn't mmap zip file")? };

    let archive = ZipArchive::new(&mapping).context("Couldn't load archive")?;
    let readers: Vec<_> = archive
        .entries()
        .iter()
        .map(|e| archive.read(e).unwrap())
        .collect();
    readers
        .into_par_iter()
        .try_for_each::<_, Result<()>>(|mut reader| {
            let mut file_contents = Vec::new();
            reader.read_to_end(&mut file_contents)?;
            if !file_contents.is_empty() {
                println!("{}", std::str::from_utf8(&file_contents).unwrap());
            }
            Ok(())
        })
}
