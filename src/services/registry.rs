use crate::api::{Registry, ServiceDescriptor, ServiceEndpoint};
use crate::config::ValidatedGraphConfig;

pub fn register_services(
    registry: &Registry,
    api_bind: &str,
    audio_bind: &str,
    monitoring_port: u16,
    api_enabled: bool,
    audio_http_enabled: bool,
    monitoring_enabled: bool,
) {
    if api_enabled {
        registry.register_service(ServiceDescriptor {
            id: "api".to_string(),
            service_type: "api".to_string(),
            endpoints: vec![ServiceEndpoint {
                name: "http".to_string(),
                url: format!("http://{}", api_bind),
            }],
        });
    }

    if audio_http_enabled {
        registry.register_service(ServiceDescriptor {
            id: "audio_http".to_string(),
            service_type: "audio_http".to_string(),
            endpoints: vec![
                ServiceEndpoint {
                    name: "live".to_string(),
                    url: format!("http://{}/audio/live", audio_bind),
                },
                ServiceEndpoint {
                    name: "timeshift".to_string(),
                    url: format!("http://{}/audio/at", audio_bind),
                },
            ],
        });
    }

    if monitoring_enabled {
        registry.register_service(ServiceDescriptor {
            id: "monitoring".to_string(),
            service_type: "monitoring".to_string(),
            endpoints: vec![
                ServiceEndpoint {
                    name: "metrics".to_string(),
                    url: format!("http://0.0.0.0:{}/metrics", monitoring_port),
                },
                ServiceEndpoint {
                    name: "health".to_string(),
                    url: format!("http://0.0.0.0:{}/health", monitoring_port),
                },
            ],
        });
    }
}

pub fn register_graph_services(
    registry: &Registry,
    api_bind: &str,
    audio_bind: &str,
    monitoring_port: u16,
    api_enabled: bool,
    audio_http_enabled: bool,
    monitoring_enabled: bool,
    graph: &ValidatedGraphConfig,
) {
    if api_enabled {
        registry.register_service(ServiceDescriptor {
            id: "api".to_string(),
            service_type: "api".to_string(),
            endpoints: vec![ServiceEndpoint {
                name: "http".to_string(),
                url: format!("http://{}", api_bind),
            }],
        });
    }

    if audio_http_enabled {
        if let Some((id, service)) = graph
        .services
        .iter()
        .find(|(_, svc)| svc.service_type == "audio_http" && svc.enabled)
        {
            registry.register_service(ServiceDescriptor {
                id: id.clone(),
                service_type: service.service_type.clone(),
                endpoints: vec![
                    ServiceEndpoint {
                        name: "live".to_string(),
                        url: format!("http://{}/audio/live", audio_bind),
                    },
                    ServiceEndpoint {
                        name: "timeshift".to_string(),
                        url: format!("http://{}/audio/at", audio_bind),
                    },
                ],
            });
        }
    }

    if monitoring_enabled {
        if let Some((id, service)) = graph
        .services
        .iter()
        .find(|(_, svc)| svc.service_type == "monitoring" && svc.enabled)
        {
            registry.register_service(ServiceDescriptor {
                id: id.clone(),
                service_type: service.service_type.clone(),
                endpoints: vec![
                    ServiceEndpoint {
                        name: "metrics".to_string(),
                        url: format!("http://0.0.0.0:{}/metrics", monitoring_port),
                    },
                    ServiceEndpoint {
                        name: "health".to_string(),
                        url: format!("http://0.0.0.0:{}/health", monitoring_port),
                    },
                ],
            });
        }
    }

    for (id, service) in graph.services.iter() {
        if !service.enabled {
            continue;
        }

        if matches!(
            service.service_type.as_str(),
            "peak_analyzer" | "influx_out" | "broadcast_http" | "file_out"
        ) {
            registry.register_service(ServiceDescriptor {
                id: id.clone(),
                service_type: service.service_type.clone(),
                endpoints: Vec::new(),
            });
        }
    }
}
