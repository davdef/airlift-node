use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crossbeam_channel::{Receiver, Sender, unbounded};
use serde::Serialize;
use tiny_http::{Header, Method, Request, Response, StatusCode};

use crate::consumers::ws::WsConsumer;
use crate::core::lock::lock_mutex;
use crate::core::{AirliftNode, Flow, PcmFrame};
use crate::producers::ws::{WsHandle, WsProducer};

static RECORDER_COUNTER: AtomicU64 = AtomicU64::new(1);

struct RecordingSession {
    producer_id: String,
    producer_handle: WsHandle,
    echo_sender: Option<Sender<PcmFrame>>, // Sender für Echo-Daten
}

static RECORDER_SESSIONS: OnceLock<Mutex<HashMap<String, RecordingSession>>> = OnceLock::new();

fn session_registry() -> &'static Mutex<HashMap<String, RecordingSession>> {
    RECORDER_SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Serialize)]
struct RecorderStartResponse {
    producer_id: String,
}

pub fn handle_recorder_start(
    req: Request,
    node: Arc<Mutex<AirliftNode>>,
) {
    let response = if req.method() != &Method::Post {
        Response::from_string("").with_status_code(StatusCode(405))
    } else {
        let producer_id = format!(
            "recorder-{}",
            RECORDER_COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let (producer, handle) = WsProducer::new(&producer_id);

        match node.lock() {
            Ok(mut guard) => match guard.add_producer(Box::new(producer)) {
                Ok(()) => {
                    let buffer_name = format!("producer:{}", producer_id);
                    let flow_name = producer_id.clone();

                    // Flow erstellen (falls nicht existiert)
                    if guard.flow_index_by_name(&flow_name).is_none() {
                        guard.add_flow(Flow::new(&flow_name));
                    }

                    if let Some(flow_index) = guard.flow_index_by_name(&flow_name) {
                        // Producer mit Flow verbinden
                        if let Err(err) = guard.connect_flow_input(flow_index, &buffer_name) {
                            log::warn!(
                                "Failed to connect recorder '{}' to flow '{}': {}",
                                producer_id,
                                flow_name,
                                err
                            );
                        }

                        // Echo-Consumer erstellen und hinzufügen
                        let (echo_consumer, echo_receiver) = WsConsumer::new(&format!("echo-{}", producer_id));
                        
                        if let Err(err) = guard.add_consumer_to_flow(flow_index, Box::new(echo_consumer)) {
                            log::warn!("Failed to add echo consumer to flow {}: {}", flow_index, err);
                        } else {
                            log::info!("Added echo consumer 'echo-{}' to flow {}", producer_id, flow_index);
                        }

                        // Channel für Echo-Daten erstellen
                        let (echo_sender, client_receiver) = unbounded();
                        
                        // Thread starten, der Daten vom Consumer zum Echo-Channel forwardet
                        let session_id = producer_id.clone();
                        let echo_sender_clone = echo_sender.clone();
                        std::thread::spawn(move || {
                            log::info!("Starting echo forwarder for session: {}", session_id);
                            let mut frame_count = 0;
                            
                            for frame in echo_receiver.iter() {
                                frame_count += 1;
                                
                                if frame_count % 100 == 0 {
                                    log::debug!("Echo forwarder '{}': forwarded {} frames", session_id, frame_count);
                                }
                                
                                // Forward frame zum Echo-Channel
                                if echo_sender_clone.send(frame).is_err() {
                                    log::info!("Echo forwarder '{}': client disconnected", session_id);
                                    break;
                                }
                            }
                            
                            log::info!("Echo forwarder stopped for session: {}", session_id);
                        });

                        // Session in Registry speichern
                        let mut sessions = lock_mutex(session_registry(), "api.recorder.register_session");
                        sessions.insert(
                            producer_id.clone(),
                            RecordingSession {
                                producer_id: producer_id.clone(),
                                producer_handle: handle,
                                echo_sender: Some(echo_sender), // Store the SENDER, not receiver
                            },
                        );

                        // Flow starten (startet automatisch alle Consumer)
                        if let Err(err) = guard.start_flow_by_name(&flow_name) {
                            log::warn!(
                                "Failed to start recorder flow '{}': {}",
                                flow_name,
                                err
                            );
                        } else {
                            log::info!("Started recorder flow '{}'", flow_name);
                        }
                    } else {
                        log::warn!(
                            "Recorder flow '{}' not found after creation",
                            flow_name
                        );
                    }

                    let payload = serde_json::to_string(&RecorderStartResponse {
                        producer_id: producer_id.clone(),
                    })
                    .unwrap_or_else(|_| {
                        "{\"producer_id\":\"serialization_error\"}".to_string()
                    });
                    Response::from_string(payload)
                        .with_status_code(StatusCode(200))
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        )
                }
                Err(err) => Response::from_string(err.to_string())
                    .with_status_code(StatusCode(500)),
            },
            Err(_) => Response::from_string("node lock poisoned")
                .with_status_code(StatusCode(500)),
        }
    };

    let _ = req.respond(response);
}

pub fn handle_recorder_stop(
    req: Request,
    node: Arc<Mutex<AirliftNode>>,
) {
    // Extrahiere producer_id aus der URL: /api/recorder/stop/:producer_id
    let url = req.url();
    let producer_id = if url.starts_with("/api/recorder/stop/") {
        url.trim_start_matches("/api/recorder/stop/")
    } else {
        ""
    };

    if producer_id.is_empty() {
        let _ = req.respond(
            Response::from_string("Missing producer_id").with_status_code(StatusCode(400)),
        );
        return;
    }

    let response = match node.lock() {
        Ok(mut guard) => {
            // Entferne die Recording-Session aus dem Node
            match guard.remove_recording_session(producer_id) {
                Ok(()) => {
                    // Entferne auch aus der Session-Registry
                    let mut sessions = lock_mutex(session_registry(), "api.recorder.unregister_session");
                    sessions.remove(producer_id);

                    Response::from_string("").with_status_code(StatusCode(200))
                }
                Err(err) => {
                    Response::from_string(err.to_string()).with_status_code(StatusCode(404))
                }
            }
        }
        Err(_) => Response::from_string("node lock poisoned").with_status_code(StatusCode(500)),
    };

    let _ = req.respond(response);
}

pub fn get_recorder_handle(producer_id: &str) -> Option<WsHandle> {
    let sessions = lock_mutex(session_registry(), "api.recorder.lookup_handle");
    sessions
        .get(producer_id)
        .map(|session| session.producer_handle.clone())
}

// Add the missing get_echo_sender function
pub fn get_echo_sender(session_id: &str) -> Option<Sender<PcmFrame>> {
    let sessions = lock_mutex(session_registry(), "api.recorder.get_echo_sender");
    sessions.get(session_id).and_then(|session| {
        session.echo_sender.clone()
    })
}

pub fn get_echo_receiver(session_id: &str) -> Option<Receiver<PcmFrame>> {
    let sessions = lock_mutex(session_registry(), "api.recorder.get_echo_receiver");
    sessions.get(session_id).and_then(|session| {
        if let Some(sender) = &session.echo_sender {
            // Erstelle einen neuen Channel für diesen Client
            let (client_sender, client_receiver) = unbounded();
            
            // Starte einen Thread, der Frames an den Session-Sender forwardet
            let forward_sender = sender.clone();
            let session_id = session_id.to_string();
            
            std::thread::spawn(move || {
                log::info!("Starting echo client forwarder for session: {}", session_id);
                
                for frame in client_receiver.iter() {
                    if forward_sender.send(frame).is_err() {
                        log::info!("Echo client forwarder '{}': session closed", session_id);
                        break;
                    }
                }
                
                log::info!("Echo client forwarder stopped for session: {}", session_id);
            });
            
            // Return the RECEIVER, not the sender
            // Wait, we need to think about this differently...
            // Actually, we need to return a receiver that will get frames from the session
            // Let me reconsider the logic
        } else {
            None
        }
    })
}
