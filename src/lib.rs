use std::io;
use std::fs::{File, metadata, remove_file, rename};
use std::io::Write;
use std::path::PathBuf;

use chrono::NaiveDate;
use rand::distributions::{Alphanumeric, DistString};

pub mod github;
pub mod db;
pub mod chart;

// Traffic types
pub enum StatType {
    Clones,
    Views,
}

// Create a temporary file and move it to a target file
fn make_temp_file(target: PathBuf, b: &[u8]) -> io::Result<()> {
    let random_str = Alphanumeric.sample_string(&mut rand::thread_rng(), 16);

    let tmpname = PathBuf::from(
        format!("cache/.tmp.{}.{}",
                random_str,
                target.extension().unwrap().to_str().expect("extension?")
        )
    );

    let mut f = File::create(&tmpname)?;
    f.write_all(b)?;
    f.flush()?;
    drop(f);

    rename(&tmpname, target)?;

    Ok(())
}

pub struct Repo {
    pub owner: String,
    pub name: String,
}

pub struct Stats {
    pub count: u64,
    pub uniques: u64,
}

pub struct RepoStats {
    pub date: NaiveDate,
    pub views: Stats,
    pub clones: Stats,
}
