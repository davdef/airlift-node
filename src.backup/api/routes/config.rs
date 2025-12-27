use axum::{Json, extract::State};

use crate::api::ApiState;
use crate::config::Config;

pub async fn get_config(State(state): State<ApiState>) -> Json<Config> {
    Json(state.config.as_ref().clone())
}
