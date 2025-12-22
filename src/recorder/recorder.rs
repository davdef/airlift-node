// src/recorder/recorder.rs
use std::fs::{create_dir_all, File};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::time::Instant;

use crate::codecs::{CodecInfo, ContainerKind};
use crate::control::ModuleState;

use super::{EncodedFrameSource, EncodedRead, RecorderConfig, RetentionPolicy};

pub fn run_recorder(
    mut reader: impl EncodedFrameSource,
    cfg: RecorderConfig,
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
        match reader.poll()? {
            EncodedRead::Frame { frame, utc_ns } => {
                let hour = utc_ns / 1_000_000_000 / 3600;
                if frame.info.container != codec_info.container {
                    anyhow::bail!(
                        "recorder received container mismatch (codec_id '{}', expected {:?}, got {:?})",
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
            EncodedRead::Gap { missed } => {
                state.mark_drop(missed);
                ring_state.mark_drop(missed);
            }
            EncodedRead::Empty => {
                std::thread::sleep(cfg.idle_sleep);
            }
        }

        if last_retention.elapsed() >= cfg.retention_interval {
            if let Some(h) = current_hour {
                for r in retentions.iter_mut() {
                    if let Err(e) = r.run(h) {
                        eprintln!("[recorder] retention error: {}", e);
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
            "recorder does not support RTP container (codec_id '{}')",
            codec_id
        )),
    }
}
