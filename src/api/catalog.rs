use serde::Serialize;
use std::sync::{Arc, Mutex};

use tiny_http::{Header, Method, Request, Response, StatusCode};

use crate::app::configurator::{
    supported_consumer_type_list, supported_producer_type_list, supported_processor_type_list,
};
use crate::core::AirliftNode;

#[derive(Serialize)]
pub struct CatalogResponse {
    pub inputs: Vec<CatalogItem>,
    pub buffers: Vec<CatalogItem>,
    pub processing: Vec<CatalogItem>,
    pub services: Vec<CatalogItem>,
    pub outputs: Vec<CatalogItem>,
}

#[derive(Serialize)]
pub struct CatalogItem {
    pub name: String,
    #[serde(rename = "type")]
    pub item_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flow: Option<String>,
}

pub fn handle_catalog_request(req: Request, node: Arc<Mutex<AirliftNode>>) {
    if req.method() != &Method::Get {
        let _ = req.respond(Response::empty(StatusCode(405)));
        return;
    }

    let response = match node.lock() {
        Ok(guard) => {
            let catalog = build_catalog(&guard);
            let body = serde_json::to_string(&catalog).unwrap_or_else(|_| "{}".to_string());
            Response::from_string(body)
                .with_status_code(StatusCode(200))
                .with_header(
                    Header::from_bytes("Content-Type", "application/json").unwrap(),
                )
        }
        Err(_) => Response::from_string("node lock poisoned")
            .with_status_code(StatusCode(500)),
    };

    let _ = req.respond(response);
}

fn build_catalog(node: &AirliftNode) -> CatalogResponse {
    let inputs = supported_producer_type_list()
        .iter()
        .map(|producer_type| CatalogItem {
            name: (*producer_type).to_string(),
            item_type: "producer".to_string(),
            flow: None,
        })
        .collect::<Vec<_>>();

    let registry = node.buffer_registry();
    let buffers = registry
        .list()
        .into_iter()
        .map(|name| CatalogItem {
            name,
            item_type: "buffer".to_string(),
            flow: None,
        })
        .collect::<Vec<_>>();

    let mut processing = Vec::new();
    let mut outputs = Vec::new();
    let mut services = Vec::new();

    services.push(CatalogItem {
        name: "flow".to_string(),
        item_type: "flow".to_string(),
        flow: None,
    });

    for processor_type in supported_processor_type_list() {
        processing.push(CatalogItem {
            name: (*processor_type).to_string(),
            item_type: "processor".to_string(),
            flow: None,
        });
    }

    for consumer_type in supported_consumer_type_list() {
        outputs.push(CatalogItem {
            name: (*consumer_type).to_string(),
            item_type: "consumer".to_string(),
            flow: None,
        });
    }

    CatalogResponse {
        inputs,
        buffers,
        processing,
        services,
        outputs,
    }
}
