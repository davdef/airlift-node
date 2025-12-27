// src/main.rs - Vereinfacht

mod core;
mod config;
mod producers;
mod processors;
mod services;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Result;
use log::{info, error, warn};

fn main() -> Result<()> {
    // Logger initialisieren
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();
    
    info!("Starting Airlift Node v0.2.0");
    
    // Config laden
    let config = match config::Config::load("config.toml") {
        Ok(cfg) => {
            info!("Configuration loaded");
            cfg
        },
        Err(e) => {
            warn!("Config error: {}. Using defaults.", e);
            config::Config::default()
        }
    };
    
    info!("Node: {} ({:?})", config.node.name, config.node.role);
    
    // Graceful Shutdown
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        info!("\nShutdown requested");
        r.store(false, Ordering::SeqCst);
    })?;
    
    // Einfachen Node bauen (nur mit Producers und Processors)
    let mut node_builder = core::NodeBuilder::new();
    
    // Producers hinzufügen
    for (name, producer_cfg) in &config.producers {
        if producer_cfg.enabled {
            info!("Adding producer: {}", name);
            let producer = producers::create(producer_cfg)?;
            node_builder.add_producer(name.clone(), producer)?;
        }
    }
    
    // Processors hinzufügen  
    for (name, processor_cfg) in &config.processors {
        if processor_cfg.enabled {
            info!("Adding processor: {}", name);
            let processor = processors::create(processor_cfg)?;
            node_builder.add_processor(
                name.clone(), 
                processor, 
                &processor_cfg.source
            )?;
        }
    }
    
    // Services hinzufügen
    for (name, service_cfg) in &config.services {
        if service_cfg.enabled {
            info!("Adding service: {}", name);
            let service = services::create(service_cfg)?;
            node_builder.add_service(name.clone(), service)?;
        }
    }
    
    // Node bauen und starten
    let mut node = node_builder.build()?;
    node.start()?;
    
    info!("Node started successfully. Press Ctrl+C to stop.");
    
    // Hauptschleife
    while running.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(100));
        
        // Einfache Health-Check
        match node.health_check() {
            Ok(_) => {},
            Err(e) => error!("Health check: {}", e),
        }
    }
    
    // Shutdown
    info!("Shutting down...");
    node.stop()?;
    
    info!("Shutdown complete");
    Ok(())
}
