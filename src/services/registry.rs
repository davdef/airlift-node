use crate::api::{Registry, ServiceDescriptor, ServiceEndpoint};

pub fn register_services(
    registry: &Registry,
    api_bind: &str,
    audio_bind: &str,
    monitoring_port: u16,
) {
    registry.register_service(ServiceDescriptor {
        id: "api".to_string(),
        service_type: "api".to_string(),
        endpoints: vec![ServiceEndpoint {
            name: "http".to_string(),
            url: format!("http://{}", api_bind),
        }],
    });

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
