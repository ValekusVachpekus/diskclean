//! Content validation for reviewed duplicate copies and their keepers.

use crate::{CleanupItem, FileSnapshot};
use std::{
    io::{self, Read},
    path::Path,
};

pub(crate) fn verify(item: &CleanupItem, cancelled: &dyn Fn() -> bool) -> io::Result<()> {
    let Some(evidence) = &item.duplicate else {
        if item.category == diskclean_rules::Category::Duplicates {
            return Err(io::Error::other("duplicate evidence missing"));
        }
        return Ok(());
    };
    for (path, snapshot) in [
        (&item.path, &item.snapshot),
        (&evidence.keeper, &evidence.keeper_snapshot),
    ] {
        if hash(path, snapshot, cancelled)?.to_hex().as_str() != evidence.fingerprint {
            return Err(io::Error::other(
                "duplicate content no longer matches the reviewed group",
            ));
        }
    }
    Ok(())
}

fn hash(
    path: &Path,
    snapshot: &FileSnapshot,
    cancelled: &dyn Fn() -> bool,
) -> io::Result<blake3::Hash> {
    let mut file = diskclean_platform::open_regular_file(path)?;
    let validate = |file: &std::fs::File| -> io::Result<()> {
        let meta = file.metadata()?;
        if diskclean_platform::opened_identity(file)? != (snapshot.dev, snapshot.ino)
            || meta.len() != snapshot.logical_size
            || meta.modified()? != snapshot.modified
        {
            return Err(io::Error::other("duplicate or keeper changed since scan"));
        }
        Ok(())
    };
    validate(&file)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut remaining = snapshot.logical_size;
    while remaining > 0 {
        if cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "cleanup cancelled",
            ));
        }
        let limit = remaining.min(buffer.len() as u64) as usize;
        let read = file.read(&mut buffer[..limit])?;
        if read == 0 {
            return Err(io::Error::other("duplicate became shorter"));
        }
        remaining -= read as u64;
        hasher.update(&buffer[..read]);
    }
    validate(&file)?;
    Ok(hasher.finalize())
}
