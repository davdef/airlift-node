// src/recorder/retention_fs.rs
use crate::recorder::RetentionPolicy;
use std::path::{PathBuf};
use std::fs;

pub struct FsRetention {
    base_dir: PathBuf,
    retention_hours: u64,
}

impl FsRetention {
    pub fn new(base_dir: PathBuf, retention_days: u64) -> Self {
        Self {
            base_dir,
            retention_hours: retention_days * 24,
        }
    }

    fn parse_hour_from_filename(name: &str) -> Option<u64> {
        // erwartet z.B. "490561.wav" oder "490561.mp3"
        let stem = name.split('.').next()?;
        stem.parse::<u64>().ok()
    }
}

impl RetentionPolicy for FsRetention {
    fn run(&mut self, now_hour: u64) -> anyhow::Result<()> {
        let cutoff = now_hour.saturating_sub(self.retention_hours);

        let entries = match fs::read_dir(&self.base_dir) {
            Ok(e) => e,
            Err(_) => return Ok(()), // stillschweigend
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };

            let hour = match Self::parse_hour_from_filename(name) {
                Some(h) => h,
                None => continue,
            };

            if hour < cutoff {
                match fs::remove_file(&path) {
                    Ok(_) => {
                        eprintln!("[retention] removed {:?}", path);
                    }
                    Err(err) => {
                        eprintln!("[retention] failed {:?}: {}", path, err);
                    }
                }
            }
        }

        Ok(())
    }
}
