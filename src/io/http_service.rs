// src/io/http_service.rs - HTTP audio service (singleton)
use std::sync::Arc;
use std::thread;

use log::{info, warn};
use tiny_http::{Method, Request, Response, Server, StatusCode};

pub trait HttpAudioOutput: Send + Sync {
    fn matches(&self, url: &str) -> bool;
    fn handle(&self, req: Request);
}

pub struct HttpAudioService {
    bind: String,
    outputs: Vec<Arc<dyn HttpAudioOutput>>,
}

impl HttpAudioService {
    pub fn new(bind: impl Into<String>) -> anyhow::Result<Self> {
        Ok(Self {
            bind: bind.into(),
            outputs: Vec::new(),
        })
    }

    pub fn register_output(&mut self, output: Arc<dyn HttpAudioOutput>) {
        self.outputs.push(output);
    }

    pub fn start(self) -> anyhow::Result<()> {
        let server = Server::http(&self.bind).map_err(|e| anyhow::anyhow!(e))?;
        let outputs = Arc::new(self.outputs);

        info!("[audio] HTTP server on {}", self.bind);

        thread::spawn(move || {
            for req in server.incoming_requests() {
                if req.method() != &Method::Get {
                    let _ = req.respond(Response::empty(StatusCode(405)));
                    continue;
                }

                let url = req.url().to_string();
                let mut req = Some(req);
                let mut handled = false;

                for output in outputs.iter() {
                    if output.matches(&url) {
                        if let Some(req) = req.take() {
                            output.handle(req);
                        }
                        handled = true;
                        break;
                    }
                }

                if let Some(req) = req {
                    if !handled {
                        warn!("[audio] no route for {}", url);
                        let _ = req.respond(Response::empty(StatusCode(404)));
                    }
                }
            }
        });

        Ok(())
    }
}
