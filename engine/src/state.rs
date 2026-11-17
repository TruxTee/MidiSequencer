use crate::pattern::Pattern;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    pub pattern: Pattern,
    pub bpm_milli: u32,
}

impl Default for State {
    fn default() -> Self {
        State { pattern: Pattern::default(), bpm_milli: 120_000 }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum LoadOutcome {
    Loaded,
    Missing,
    /// The unreadable file was moved aside to this path.
    Corrupt(PathBuf),
}

/// Writes to a temp file, flushes it to disk, then renames it over the old file.
/// A power cut at any point leaves either the complete old file or the complete new one.
pub fn save_atomic(path: &Path, state: &State) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    let data = serde_json::to_vec_pretty(state).map_err(io::Error::other)?;
    {
        let mut f = File::create(&tmp)?;
        f.write_all(&data)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    // The rename itself must reach the disk too; Linux needs the directory synced for that.
    #[cfg(unix)]
    if let Some(dir) = path.parent() {
        File::open(dir)?.sync_all()?;
    }
    Ok(())
}

pub fn load(path: &Path) -> (State, LoadOutcome) {
    let data = match fs::read(path) {
        Ok(d) => d,
        Err(_) => return (State::default(), LoadOutcome::Missing),
    };
    match serde_json::from_slice(&data) {
        Ok(s) => (s, LoadOutcome::Loaded),
        Err(_) => {
            let bad = path.with_extension("bad");
            let _ = fs::rename(path, &bad);
            (State::default(), LoadOutcome::Corrupt(bad))
        }
    }
}

/// Decides when to save: shortly after the last edit, but never later than `max_wait`
/// after the first unsaved edit, so constant tweaking still gets saved.
pub struct Autosave {
    quiet_ns: u64,
    max_wait_ns: u64,
    first_dirty: Option<u64>,
    last_dirty: u64,
}

impl Autosave {
    pub fn new(quiet_ns: u64, max_wait_ns: u64) -> Self {
        Autosave { quiet_ns, max_wait_ns, first_dirty: None, last_dirty: 0 }
    }

    pub fn mark_dirty(&mut self, now_ns: u64) {
        self.first_dirty.get_or_insert(now_ns);
        self.last_dirty = now_ns;
    }

    pub fn due(&self, now_ns: u64) -> bool {
        match self.first_dirty {
            None => false,
            Some(first) => {
                now_ns.saturating_sub(self.last_dirty) >= self.quiet_ns
                    || now_ns.saturating_sub(first) >= self.max_wait_ns
            }
        }
    }

    pub fn saved(&mut self) {
        self.first_dirty = None;
    }
}
