use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct DeviceInfo {
    pub id: String,
    pub label: String,
}

#[derive(Serialize)]
pub struct DevicesResponse {
    pub available: bool,
    pub devices: Vec<DeviceInfo>,
}

pub async fn get_devices() -> Json<DevicesResponse> {
    let devices = detect_alsa_devices();
    Json(DevicesResponse {
        available: cfg!(feature = "alsa"),
        devices,
    })
}

fn detect_alsa_devices() -> Vec<DeviceInfo> {
    #[cfg(feature = "alsa")]
    {
        // TODO: implement ALSA device enumeration.
        Vec::new()
    }
    #[cfg(not(feature = "alsa"))]
    {
        Vec::new()
    }
}
