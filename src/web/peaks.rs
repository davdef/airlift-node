// src/web/peaks.rs
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::{Arc, RwLock};

use axum::{
    extract::State,
    response::Json,
};

#[derive(Debug, Clone, Serialize)]
pub struct PeakPoint {
    pub timestamp: u64,    // in Nanosekunden!
    pub peak_l: f32,
    pub peak_r: f32,
    pub rms: Option<f32>,
    pub lufs: Option<f32>,
    pub silence: bool,
}

pub struct PeakStorage {
    buffer: RwLock<VecDeque<PeakPoint>>,
}

const BUFFER_CAPACITY: usize = 10000;

impl PeakStorage {
    pub fn new() -> Self {
        Self {
            buffer: RwLock::new(VecDeque::with_capacity(BUFFER_CAPACITY)),
        }
    }

    pub fn add_peak(&self, peak: PeakPoint) {
        let mut buffer = self.buffer.write().unwrap();
        buffer.push_back(peak);
        
        while buffer.len() > BUFFER_CAPACITY {
            buffer.pop_front();
        }
    }

pub async fn get_peaks(
    State(peak_store): State<Arc<PeakStorage>>  // KEIN Tuple mehr!
) -> Json<Vec<PeakPoint>> {
    let (_buffer_start, buffer_end) = peak_store.get_buffer_bounds();
    let peaks = peak_store.get_peaks(buffer_end.saturating_sub(1000), buffer_end);
    Json(peaks)
}

    pub fn get_buffer_bounds(&self) -> (usize, usize) {
        let buffer = self.buffer.read().unwrap();
        (0, buffer.len())
    }

    pub fn get_latest(&self) -> Option<PeakPoint> {
        let buffer = self.buffer.read().unwrap();
        buffer.back().cloned()
    }
    
    // NEUE METHODE für buffer_info_handler
    pub fn get_buffer_read_lock(&self) -> Result<std::sync::RwLockReadGuard<VecDeque<PeakPoint>>, ()> {
        match self.buffer.read() {
            Ok(guard) => Ok(guard),
            Err(_) => Err(()),
        }
    }
}

// Handler für /api/peaks (ANGEPASST für Tuple State)
pub async fn get_peaks(
    State((peak_store, _influx_service)): State<(Arc<PeakStorage>, Arc<crate::web::influx_service::InfluxService>)>
) -> Json<Vec<PeakPoint>> {
    let (_buffer_start, buffer_end) = peak_store.get_buffer_bounds();
    let peaks = peak_store.get_peaks(buffer_end.saturating_sub(1000), buffer_end);
    Json(peaks)
}
