//! Release-machine commands for the fork's update key (`docs/releasing-fork.md`),
//! hidden from `hex --help`. They never talk to a running Hex.
use std::path::{Path, PathBuf};

pub(super) fn keygen(key: &Path) -> u8 {
    match crate::updater::release_keygen(key) {
        Ok(public) => {
            println!("{public}");
            0
        }
        Err(error) => {
            eprintln!("hex: {error:#}");
            super::EXIT_REFUSED
        }
    }
}

pub(super) fn sign(key: &Path, files: &[PathBuf]) -> u8 {
    match crate::updater::release_sign(key, files) {
        Ok(sums) => {
            for (sha256, file) in sums.iter().zip(files) {
                println!("{sha256}  {}", file.display());
            }
            0
        }
        Err(error) => {
            eprintln!("hex: {error:#}");
            super::EXIT_REFUSED
        }
    }
}
