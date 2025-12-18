// src/web/player.rs

use axum::{
    extract::State,
    response::{Html, IntoResponse},
};
use std::sync::Arc;

use crate::web::peaks::PeakStorage;

pub async fn index(State(_peak_store): State<Arc<PeakStorage>>) -> impl IntoResponse {
    Html(include_str!("../../public/index.html"))
}
