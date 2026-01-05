use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use crossbeam_channel::{Receiver, Sender, unbounded};
use serde::Serialize;
use tiny_http::{Header, Method, Request, Response, StatusCode};

use crate::consumers::ws::WsConsumer;
use crate::core::lock::lock_mutex;
use crate::core::{AirliftNode, Flow, PcmFrame};
use crate::producers::ws::{WsHandle, WsProducer};

static RECORDER_COUNTER: AtomicU64 = AtomicU64::new(1);
static ECHO_CLIENT_COUNTER: AtomicU64 = AtomicU64::new(1);

struct RecordingSession {
    producer_id: String,
    producer_handle: WsHandle,
    echo_clients: HashMap<u64, Sender<PcmFrame>>,
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

                        // Echo-Consumer erstellen und konfigurieren
                        let (mut echo_consumer, echo_receiver) = WsConsumer::new(&format!("echo-{}", producer_id));
                        echo_consumer.set_echo_mode(true); // WICHTIG: Echo-Modus aktivieren!
                        
                        if let Err(err) = guard.add_consumer_to_flow(flow_index, Box::new(echo_consumer)) {
                            log::warn!("Failed to add echo consumer to flow {}: {}", flow_index, err);
                        } else {
                            log::info!("Added echo consumer 'echo-{}' to flow {}", producer_id, flow_index);
                        }

                        // Thread starten, der Daten vom Consumer an alle Echo-Clients forwardet
                        let session_id = producer_id.clone();
                        std::thread::spawn(move || {
                            log::info!("Starting echo forwarder for session: {}", session_id);
                            let mut frame_count = 0;
                            let mut last_log = Instant::now();
                            
                            for frame in echo_receiver.iter() {
                                frame_count += 1;
                                
                                // Gelegentlich loggen (nicht zu oft)
                                if last_log.elapsed() >= std::time::Duration::from_secs(2) {
                                    log::debug!("Echo forwarder '{}': forwarded {} frames", session_id, frame_count);
                                    last_log = Instant::now();
                                }
                                
                                let clients = {
                                    let sessions = lock_mutex(session_registry(), "api.recorder.echo_clients_snapshot");
                                    sessions.get(&session_id).map(|session| session.echo_clients.clone())
                                };

                                let Some(clients) = clients else {
                                    log::info!("Echo forwarder '{}' stopped: session removed", session_id);
                                    break;
                                };

                                let mut failed_clients = Vec::new();
                                for (client_id, sender) in clients {
                                    // Frame KLONEN und senden (jeder Client bekommt eigene Kopie)
                                    if sender.send(frame.clone()).is_err() {
                                        failed_clients.push(client_id);
                                    }
                                }

                                if !failed_clients.is_empty() {
                                    let mut sessions = lock_mutex(session_registry(), "api.recorder.echo_clients_prune");
                                    if let Some(session) = sessions.get_mut(&session_id) {
                                        for client_id in failed_clients {
                                            session.echo_clients.remove(&client_id);
                                        }
                                    }
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
                                echo_clients: HashMap::new(),
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

pub fn register_echo_client(session_id: &str) -> Option<(u64, Receiver<PcmFrame>)> {
    let (sender, receiver) = unbounded();
    let mut sessions = lock_mutex(session_registry(), "api.recorder.register_echo_client");
    let session = sessions.get_mut(session_id)?;
    let client_id = ECHO_CLIENT_COUNTER.fetch_add(1, Ordering::Relaxed);
    session.echo_clients.insert(client_id, sender);
    Some((client_id, receiver))
}

pub fn unregister_echo_client(session_id: &str, client_id: u64) {
    let mut sessions = lock_mutex(session_registry(), "api.recorder.unregister_echo_client");
    if let Some(session) = sessions.get_mut(session_id) {
        session.echo_clients.remove(&client_id);
    }
}
