use serde::Serialize;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Default)]
pub struct ModuleState {
    enabled: AtomicBool,
    running: AtomicBool,
    connected: AtomicBool,
    rx: AtomicU64,
    tx: AtomicU64,
    drops: AtomicU64,
    errors: AtomicU64,
    last_activity_ms: AtomicU64,
}

impl ModuleState {
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn set_running(&self, running: bool) {
        self.running.store(running, Ordering::Relaxed);
    }

    pub fn set_connected(&self, connected: bool) {
        self.connected.store(connected, Ordering::Relaxed);
    }

    pub fn swap_connected(&self, connected: bool) -> bool {
        self.connected.swap(connected, Ordering::Relaxed)
    }

    pub fn mark_rx(&self, amount: u64) {
        if amount > 0 {
            self.rx.fetch_add(amount, Ordering::Relaxed);
        }
        self.touch();
    }

    pub fn mark_tx(&self, amount: u64) {
        if amount > 0 {
            self.tx.fetch_add(amount, Ordering::Relaxed);
        }
        self.touch();
    }

    pub fn mark_drop(&self, amount: u64) {
        if amount > 0 {
            self.drops.fetch_add(amount, Ordering::Relaxed);
        }
        self.touch();
    }

    pub fn mark_error(&self, amount: u64) {
        if amount > 0 {
            self.errors.fetch_add(amount, Ordering::Relaxed);
        }
        self.touch();
    }

    pub fn touch(&self) {
        self.last_activity_ms.store(now_ms(), Ordering::Relaxed);
    }

    pub fn reset_counters(&self) {
        self.rx.store(0, Ordering::Relaxed);
        self.tx.store(0, Ordering::Relaxed);
        self.drops.store(0, Ordering::Relaxed);
        self.errors.store(0, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> ModuleSnapshot {
        ModuleSnapshot {
            enabled: self.enabled.load(Ordering::Relaxed),
            running: self.running.load(Ordering::Relaxed),
            connected: self.connected.load(Ordering::Relaxed),
            counters: CountersSnapshot {
                rx: self.rx.load(Ordering::Relaxed),
                tx: self.tx.load(Ordering::Relaxed),
                drops: self.drops.load(Ordering::Relaxed),
                errors: self.errors.load(Ordering::Relaxed),
            },
            last_activity_ms: self.last_activity_ms.load(Ordering::Relaxed),
        }
    }
}

#[derive(Clone, Serialize)]
pub struct CountersSnapshot {
    pub rx: u64,
    pub tx: u64,
    pub drops: u64,
    pub errors: u64,
}

#[derive(Clone, Serialize)]
pub struct ModuleSnapshot {
    pub enabled: bool,
    pub running: bool,
    pub connected: bool,
    pub counters: CountersSnapshot,
    pub last_activity_ms: u64,
}

pub struct SrtInState {
    pub module: ModuleState,
    pub force_disconnect: AtomicBool,
}

impl SrtInState {
    pub fn new() -> Self {
        Self {
            module: ModuleState::default(),
            force_disconnect: AtomicBool::new(false),
        }
    }
}

pub struct SrtOutState {
    pub module: ModuleState,
    pub force_reconnect: AtomicBool,
}

impl SrtOutState {
    pub fn new() -> Self {
        Self {
            module: ModuleState::default(),
            force_reconnect: AtomicBool::new(false),
        }
    }
}

pub struct ControlState {
    pub srt_in: Arc<SrtInState>,
    pub srt_out: Arc<SrtOutState>,
    pub alsa_in: Arc<ModuleState>,
    pub icecast_in: Arc<ModuleState>,
    pub icecast_out: Arc<ModuleState>,
    pub file_in: Arc<ModuleState>,
    pub file_out: Arc<ModuleState>,
    pub ring: Arc<ModuleState>,
}

impl ControlState {
    pub fn new() -> Self {
        Self {
            srt_in: Arc::new(SrtInState::new()),
            srt_out: Arc::new(SrtOutState::new()),
            alsa_in: Arc::new(ModuleState::default()),
            icecast_in: Arc::new(ModuleState::default()),
            icecast_out: Arc::new(ModuleState::default()),
            file_in: Arc::new(ModuleState::default()),
            file_out: Arc::new(ModuleState::default()),
            ring: Arc::new(ModuleState::default()),
        }
    }

    pub fn reset_counters(&self) {
        self.srt_in.module.reset_counters();
        self.srt_out.module.reset_counters();
        self.alsa_in.reset_counters();
        self.icecast_in.reset_counters();
        self.icecast_out.reset_counters();
        self.file_in.reset_counters();
        self.file_out.reset_counters();
        self.ring.reset_counters();
    }
}
