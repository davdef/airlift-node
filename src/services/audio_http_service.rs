use log::{error, info};
use std::path::PathBuf;

use crate::audio::http::start_audio_http_server;
use crate::ring::AudioRing;

pub struct AudioHttpService {
    bind: String,
}

impl AudioHttpService {
    pub fn new(bind: impl Into<String>) -> Self {
        Self {
            bind: bind.into(),
        }
    }

    pub fn start(&self, wav_dir: PathBuf, ring: AudioRing) {
        // eine Kopie für Logs / Außenwelt
        let bind = self.bind.clone();
        // eine Kopie für den Worker-Thread
        let bind_for_thread = bind.clone();

        std::thread::spawn(move || {
            if let Err(e) = start_audio_http_server(
                &bind_for_thread,
                wav_dir,
                move || ring.subscribe(),
            ) {
                error!("[audio_http] server failed: {}", e);
            }
        });

        info!("[airlift] audio HTTP enabled (http://{})", bind);
    }
}
