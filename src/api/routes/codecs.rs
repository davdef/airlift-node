use axum::extract::State;
use axum::Json;

use crate::api::ApiState;
use crate::codecs::registry::CodecInstanceSnapshot;

pub async fn get_codecs(State(state): State<ApiState>) -> Json<Vec<CodecInstanceSnapshot>> {
    Json(state.codec_registry.snapshots())
}
