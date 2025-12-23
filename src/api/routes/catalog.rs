use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct CatalogConfigField {
    pub key: String,
    pub required: bool,
    pub example: String,
}

#[derive(Serialize)]
pub struct CatalogItem {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub item_type: Option<String>,
    #[serde(rename = "backendType", skip_serializing_if = "Option::is_none")]
    pub backend_type: Option<String>,
    pub label: String,
    pub supported: bool,
    #[serde(rename = "configFields")]
    pub config_fields: Vec<CatalogConfigField>,
}

#[derive(Serialize)]
pub struct CatalogGroup {
    pub title: String,
    pub kind: String,
    pub items: Vec<CatalogItem>,
}

fn config_field(key: &str, required: bool, example: &str) -> CatalogConfigField {
    CatalogConfigField {
        key: key.to_string(),
        required,
        example: example.to_string(),
    }
}

fn catalog_data() -> Vec<CatalogGroup> {
    vec![
        CatalogGroup {
            title: "Inputs".to_string(),
            kind: "input".to_string(),
            items: vec![
                CatalogItem {
                    item_type: Some("srt".to_string()),
                    backend_type: Some("srt".to_string()),
                    label: "SRT Input".to_string(),
                    supported: true,
                    config_fields: vec![
                        config_field("listen", true, "0.0.0.0:9000"),
                        config_field("latency_ms", true, "200"),
                        config_field("streamid", false, "airlift"),
                    ],
                },
                CatalogItem {
                    item_type: Some("icecast".to_string()),
                    backend_type: Some("icecast".to_string()),
                    label: "Icecast Input".to_string(),
                    supported: true,
                    config_fields: vec![config_field(
                        "url",
                        true,
                        "https://example.com/stream.ogg",
                    )],
                },
                CatalogItem {
                    item_type: Some("http_stream".to_string()),
                    backend_type: Some("http_stream".to_string()),
                    label: "HTTP Stream Input".to_string(),
                    supported: true,
                    config_fields: vec![config_field(
                        "url",
                        true,
                        "https://example.com/stream.ogg",
                    )],
                },
                CatalogItem {
                    item_type: Some("alsa".to_string()),
                    backend_type: Some("alsa".to_string()),
                    label: "ALSA Input".to_string(),
                    supported: true,
                    config_fields: vec![config_field("device", true, "hw:0,0")],
                },
                CatalogItem {
                    item_type: Some("file_in".to_string()),
                    backend_type: Some("file_in".to_string()),
                    label: "File Input".to_string(),
                    supported: true,
                    config_fields: vec![config_field("path", true, "/path/to/audio.wav")],
                },
            ],
        },
        CatalogGroup {
            title: "Buffers".to_string(),
            kind: "buffer".to_string(),
            items: vec![CatalogItem {
                item_type: None,
                backend_type: None,
                label: "Ringbuffer".to_string(),
                supported: true,
                config_fields: vec![
                    config_field("slots", true, "6000"),
                    config_field("prealloc_samples", true, "9600"),
                ],
            }],
        },
        CatalogGroup {
            title: "Codecs".to_string(),
            kind: "processing".to_string(),
            items: vec![
                CatalogItem {
                    item_type: Some("pcm".to_string()),
                    backend_type: Some("pcm".to_string()),
                    label: "PCM".to_string(),
                    supported: true,
                    config_fields: vec![
                        config_field("sample_rate", false, "48000"),
                        config_field("channels", false, "2"),
                    ],
                },
                CatalogItem {
                    item_type: Some("opus_ogg".to_string()),
                    backend_type: Some("opus_ogg".to_string()),
                    label: "Opus OGG".to_string(),
                    supported: true,
                    config_fields: vec![
                        config_field("sample_rate", false, "48000"),
                        config_field("channels", false, "2"),
                        config_field("frame_size_ms", false, "20"),
                        config_field("bitrate", false, "128000"),
                        config_field("application", false, "audio"),
                    ],
                },
                CatalogItem {
                    item_type: Some("opus_webrtc".to_string()),
                    backend_type: Some("opus_webrtc".to_string()),
                    label: "Opus WebRTC".to_string(),
                    supported: true,
                    config_fields: vec![
                        config_field("sample_rate", false, "48000"),
                        config_field("channels", false, "2"),
                        config_field("bitrate", false, "128000"),
                        config_field("application", false, "audio"),
                    ],
                },
                CatalogItem {
                    item_type: Some("mp3".to_string()),
                    backend_type: Some("mp3".to_string()),
                    label: "MP3".to_string(),
                    supported: true,
                    config_fields: vec![
                        config_field("sample_rate", false, "48000"),
                        config_field("channels", false, "2"),
                        config_field("bitrate", false, "128000"),
                    ],
                },
                CatalogItem {
                    item_type: Some("vorbis".to_string()),
                    backend_type: Some("vorbis".to_string()),
                    label: "Vorbis".to_string(),
                    supported: true,
                    config_fields: vec![
                        config_field("sample_rate", false, "48000"),
                        config_field("channels", false, "2"),
                        config_field("quality", false, "0.4"),
                    ],
                },
                CatalogItem {
                    item_type: Some("aac_lc".to_string()),
                    backend_type: Some("aac_lc".to_string()),
                    label: "AAC-LC".to_string(),
                    supported: false,
                    config_fields: Vec::new(),
                },
                CatalogItem {
                    item_type: Some("flac".to_string()),
                    backend_type: Some("flac".to_string()),
                    label: "FLAC".to_string(),
                    supported: false,
                    config_fields: Vec::new(),
                },
            ],
        },
        CatalogGroup {
            title: "Services".to_string(),
            kind: "service".to_string(),
            items: vec![
                CatalogItem {
                    item_type: Some("audio_http".to_string()),
                    backend_type: Some("audio_http".to_string()),
                    label: "Audio HTTP".to_string(),
                    supported: true,
                    config_fields: vec![
                        config_field("buffer", true, "main"),
                        config_field("codec_id", true, "codec_opus_ogg"),
                    ],
                },
                CatalogItem {
                    item_type: Some("monitor".to_string()),
                    backend_type: Some("monitor".to_string()),
                    label: "Monitor".to_string(),
                    supported: true,
                    config_fields: Vec::new(),
                },
                CatalogItem {
                    item_type: Some("monitoring".to_string()),
                    backend_type: Some("monitoring".to_string()),
                    label: "Monitoring".to_string(),
                    supported: true,
                    config_fields: Vec::new(),
                },
                CatalogItem {
                    item_type: Some("peak_analyzer".to_string()),
                    backend_type: Some("peak_analyzer".to_string()),
                    label: "Peak Analyzer".to_string(),
                    supported: true,
                    config_fields: vec![
                        config_field("buffer", true, "main"),
                        config_field("interval_ms", true, "30000"),
                    ],
                },
                CatalogItem {
                    item_type: Some("influx_out".to_string()),
                    backend_type: Some("influx_out".to_string()),
                    label: "InfluxDB".to_string(),
                    supported: true,
                    config_fields: vec![
                        config_field("url", true, "https://influx.example"),
                        config_field("db", true, "airlift"),
                        config_field("interval_ms", true, "30000"),
                    ],
                },
                CatalogItem {
                    item_type: Some("broadcast_http".to_string()),
                    backend_type: Some("broadcast_http".to_string()),
                    label: "Broadcast HTTP".to_string(),
                    supported: true,
                    config_fields: vec![
                        config_field("url", true, "https://example.com/notify"),
                        config_field("interval_ms", true, "30000"),
                    ],
                },
                CatalogItem {
                    item_type: Some("file_out".to_string()),
                    backend_type: Some("file_out".to_string()),
                    label: "File-Out Service".to_string(),
                    supported: true,
                    config_fields: vec![
                        config_field("codec_id", true, "codec_opus_ogg"),
                        config_field("wav_dir", true, "/opt/rfm/airlift-node/aircheck/wav"),
                        config_field("retention_days", true, "7"),
                    ],
                },
            ],
        },
        CatalogGroup {
            title: "Outputs".to_string(),
            kind: "output".to_string(),
            items: vec![
                CatalogItem {
                    item_type: Some("srt_out".to_string()),
                    backend_type: Some("srt_out".to_string()),
                    label: "SRT Output".to_string(),
                    supported: true,
                    config_fields: vec![
                        config_field("target", true, "example.com:9000"),
                        config_field("latency_ms", true, "200"),
                        config_field("codec_id", true, "codec_opus_ogg"),
                    ],
                },
                CatalogItem {
                    item_type: Some("icecast_out".to_string()),
                    backend_type: Some("icecast_out".to_string()),
                    label: "Icecast Output".to_string(),
                    supported: true,
                    config_fields: vec![
                        config_field("host", true, "icecast.local"),
                        config_field("port", true, "8000"),
                        config_field("mount", true, "/airlift"),
                        config_field("user", true, "source"),
                        config_field("password", true, "hackme"),
                        config_field("bitrate", true, "128000"),
                        config_field("name", true, "Airlift Node"),
                        config_field("description", true, "Live-Stream"),
                        config_field("genre", true, "news"),
                        config_field("public", true, "false"),
                        config_field("codec_id", true, "codec_opus_ogg"),
                    ],
                },
                CatalogItem {
                    item_type: Some("udp_out".to_string()),
                    backend_type: Some("udp_out".to_string()),
                    label: "UDP Output".to_string(),
                    supported: true,
                    config_fields: vec![
                        config_field("target", true, "239.0.0.1:1234"),
                        config_field("codec_id", true, "codec_opus_webrtc"),
                    ],
                },
                CatalogItem {
                    item_type: Some("file_out".to_string()),
                    backend_type: Some("file_out".to_string()),
                    label: "File Output".to_string(),
                    supported: true,
                    config_fields: vec![
                        config_field("wav_dir", true, "/opt/rfm/airlift-node/aircheck/wav"),
                        config_field("retention_days", true, "7"),
                        config_field("codec_id", true, "codec_opus_ogg"),
                    ],
                },
            ],
        },
    ]
}

pub async fn get_catalog() -> Json<Vec<CatalogGroup>> {
    Json(catalog_data())
}
