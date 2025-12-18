use axum::{extract::State, response::Json};
use std::sync::Arc;

use crate::web::peaks::{PeakPoint, PeakStorage};

pub async fn get_peaks(
    State(peak_store): State<Arc<PeakStorage>>,
) -> Json<Vec<PeakPoint>> {
    let buf = match peak_store.get_buffer_read_lock() {
        Ok(b) => b,
        Err(_) => return Json(Vec::new()),
    };

    Json(buf.iter().rev().take(1000).cloned().collect())
}
