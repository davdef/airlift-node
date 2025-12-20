use axum::{extract::Query, extract::State, response::Json};
use log::warn;
use serde::{Deserialize, Serialize};

use crate::web::WebState;
use crate::web::influx_service::HistoryPoint;

#[derive(Serialize)]
pub struct PeaksMeta {
    ok: bool,
    start: u64,
    end: u64,
}

#[derive(Deserialize)]
pub struct HistoryQuery {
    from: u64,
    to: u64,
}

pub async fn get_peaks(State(state): State<WebState>) -> Json<PeaksMeta> {
    let buf = match state.peak_store.get_buffer_read_lock() {
        Ok(b) => b,
        Err(_) => {
            return Json(PeaksMeta {
                ok: false,
                start: 0,
                end: 0,
            });
        }
    };

    if let (Some(start), Some(end)) = (buf.front(), buf.back()) {
        Json(PeaksMeta {
            ok: true,
            start: start.timestamp,
            end: end.timestamp,
        })
    } else {
        Json(PeaksMeta {
            ok: false,
            start: 0,
            end: 0,
        })
    }
}

pub async fn get_history(
    State(state): State<WebState>,
    Query(params): Query<HistoryQuery>,
) -> Json<Vec<HistoryPoint>> {
    let Some(history_service) = &state.history_service else {
        return Json(Vec::new());
    };

    match history_service.get_history(params.from, params.to) {
        Ok(points) => Json(points),
        Err(err) => {
            warn!("[influx] history query failed: {}", err);
            Json(Vec::new())
        }
    }
}
