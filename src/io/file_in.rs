use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::control::ModuleState;
use crate::ring::AudioRing;

pub struct FileInConfig {
    pub enabled: bool,
    pub path: PathBuf,
}

pub fn run_file_in(
    _ring: AudioRing,
    _cfg: FileInConfig,
    _running: Arc<AtomicBool>,
    state: Arc<ModuleState>,
    _ring_state: Arc<ModuleState>,
) -> anyhow::Result<()> {
    state.set_running(true);
    state.set_connected(false);
    state.set_running(false);
    state.mark_error(1);
    anyhow::bail!("file_in is not implemented yet")
}
