use std::fs::{create_dir_all, File, read_dir, remove_file};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::codecs::{CodecInfo, ContainerKind};
use crate::control::ModuleState;
use crate::ring::{EncodedRingRead, EncodedSource};

pub struct FileOutConfig {
    pub idle_sleep: Duration,
    pub retention_interval: Duration,
}

pub trait RetentionPolicy: Send + Sync {
    fn run(&mut self, current_hour: u64) -> anyhow::Result<()>;
}

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
        let stem = name.split('.').next()?;
        stem.parse::<u64>().ok()
    }
}

impl RetentionPolicy for FsRetention {
    fn run(&mut self, now_hour: u64) -> anyhow::Result<()> {
        let cutoff = now_hour.saturating_sub(self.retention_hours);

        let entries = match read_dir(&self.base_dir) {
            Ok(e) => e,
            Err(_) => return Ok(()),
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
                match remove_file(&path) {
                    Ok(_) => {
                        eprintln!("[file_out] removed {:?}", path);
                    }
                    Err(err) => {
                        eprintln!("[file_out] failed {:?}: {}", path, err);
                    }
                }
            }
        }

        Ok(())
    }
}

pub fn run_file_out(
    mut reader: impl EncodedSource,
    cfg: FileOutConfig,
    base_dir: PathBuf,
    codec_id: String,
    codec_info: CodecInfo,
    mut retentions: Vec<Box<dyn RetentionPolicy>>,
    state: std::sync::Arc<ModuleState>,
    ring_state: std::sync::Arc<ModuleState>,
) -> anyhow::Result<()> {
    state.set_running(true);
    state.set_connected(true);
    create_dir_all(&base_dir)?;

    let mut current_hour: Option<u64> = None;
    let mut writer: Option<BufWriter<File>> = None;
    let mut last_retention = Instant::now();

    let extension = file_extension(&codec_id, &codec_info)?;

    loop {
        match reader.poll() {
            EncodedRingRead::Frame { frame, utc_ns } => {
                let hour = utc_ns / 1_000_000_000 / 3600;
                if frame.info.container != codec_info.container {
                    anyhow::bail!(
                        "file_out received container mismatch (codec_id '{}', expected {:?}, got {:?})",
                        codec_id,
                        codec_info.container,
                        frame.info.container
                    );
                }

                if current_hour != Some(hour) {
                    let path = base_dir.join(format!("{}.{}", hour, extension));
                    writer = Some(BufWriter::new(File::create(path)?));
                    current_hour = Some(hour);
                }

                if let Some(w) = writer.as_mut() {
                    w.write_all(&frame.payload)?;
                    w.flush()?;
                    state.mark_tx(1);
                    ring_state.mark_tx(1);
                }
            }
            EncodedRingRead::Gap { missed } => {
                state.mark_drop(missed);
                ring_state.mark_drop(missed);
            }
            EncodedRingRead::Empty => {
                std::thread::sleep(cfg.idle_sleep);
            }
        }

        if last_retention.elapsed() >= cfg.retention_interval {
            if let Some(h) = current_hour {
                for r in retentions.iter_mut() {
                    if let Err(e) = r.run(h) {
                        eprintln!("[file_out] retention error: {}", e);
                    }
                }
            }
            last_retention = Instant::now();
        }
    }
}

fn file_extension(codec_id: &str, info: &CodecInfo) -> anyhow::Result<&'static str> {
    match info.container {
        ContainerKind::Ogg => Ok("ogg"),
        ContainerKind::Mpeg => Ok("mp3"),
        ContainerKind::Raw => Ok("raw"),
        ContainerKind::Rtp => Err(anyhow::anyhow!(
            "file_out does not support RTP container (codec_id '{}')",
            codec_id
        )),
    }
}
