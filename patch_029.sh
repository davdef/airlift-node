#!/bin/bash
# test_simple_flow.sh

# Erstelle eine Test-Config ohne Mixer
cat > test_config.toml << 'EOF'
node_name = "test-node"

[producers.test_source]
type = "file"
enabled = true
path = "background.wav"
loop_audio = true
channels = 2
sample_rate = 48000

[processors.simple_pass]
type = "passthrough"
enabled = true

[consumers.test_recorder]
type = "file"
enabled = true
path = "test_simple.wav"

[flows.test_flow]
enabled = true
inputs = ["test_source"]
processors = ["simple_pass"]
outputs = ["test_recorder"]
EOF

# Erstelle ein einfaches Test-Programm
cat > src/test_simple.rs << 'EOF'
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug"))
        .format_timestamp_millis()
        .init();
    
    log::info!("=== Simple Flow Test ===");
    
    let config = airlift_node::config::Config::load("test_config.toml")
        .unwrap_or_else(|e| {
            log::warn!("Config error: {}, using defaults", e);
            airlift_node::config::Config::default()
        });
    
    let mut node = airlift_node::core::AirliftNode::new();
    
    // Producer hinzufügen
    for (name, producer_cfg) in &config.producers {
        if !producer_cfg.enabled { continue; }
        
        match producer_cfg.producer_type.as_str() {
            "file" => {
                let producer = airlift_node::producers::file::FileProducer::new(name, producer_cfg);
                node.add_producer(Box::new(producer));
                log::info!("Added producer: {}", name);
            }
            _ => {}
        }
    }
    
    // Flow erstellen
    for (flow_name, flow_cfg) in &config.flows {
        if !flow_cfg.enabled { continue; }
        
        let mut flow = airlift_node::core::Flow::new(flow_name);
        
        // Processors hinzufügen
        for processor_name in &flow_cfg.processors {
            if let Some(processor_cfg) = config.processors.get(processor_name) {
                if !processor_cfg.enabled { continue; }
                
                match processor_cfg.processor_type.as_str() {
                    "passthrough" => {
                        let processor = airlift_node::core::processor::basic::PassThrough::new(processor_name);
                        flow.add_processor(Box::new(processor));
                        log::info!("Added processor: {}", processor_name);
                    }
                    _ => {}
                }
            }
        }
        
        // Consumer hinzufügen
        for output_name in &flow_cfg.outputs {
            if let Some(consumer_cfg) = config.consumers.get(output_name) {
                if !consumer_cfg.enabled { continue; }
                
                match consumer_cfg.consumer_type.as_str() {
                    "file" => {
                        if let Some(path) = &consumer_cfg.path {
                            let consumer = Box::new(airlift_node::core::consumer::file_writer::FileConsumer::new(
                                output_name, path
                            ));
                            flow.add_consumer(consumer);
                            log::info!("Added consumer: {}", output_name);
                        }
                    }
                    _ => {}
                }
            }
        }
        
        node.flows.push(flow);
        log::info!("Added flow: {}", flow_name);
    }
    
    // Verbinde Producer mit Flow
    for (flow_name, flow_cfg) in &config.flows {
        if !flow_cfg.enabled { continue; }
        
        for (flow_idx, flow) in node.flows.iter().enumerate() {
            if flow.name == *flow_name {
                for input_name in &flow_cfg.inputs {
                    for (prod_idx, producer) in node.producers().iter().enumerate() {
                        if producer.name() == input_name {
                            if let Err(e) = node.connect_producer_to_flow(prod_idx, flow_idx) {
                                log::error!("Connection error: {}", e);
                            }
                            break;
                        }
                    }
                }
                break;
            }
        }
    }
    
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();
    
    ctrlc::set_handler(move || {
        log::info!("\nShutdown requested");
        shutdown_clone.store(true, Ordering::SeqCst);
    })?;
    
    node.start()?;
    log::info!("Node running for 10 seconds...");
    
    let mut tick = 0;
    while !shutdown.load(Ordering::Relaxed) && tick < 20 {
        std::thread::sleep(Duration::from_millis(500));
        tick += 1;
        
        let status = node.status();
        log::info!("Tick {}: producers={}, flows={}, output_buffer={}", 
            tick, status.producers, status.flows,
            status.flow_status.get(0).map(|f| f.output_buffer_level).unwrap_or(0));
    }
    
    node.stop()?;
    log::info!("Test completed. Check test_simple.wav");
    
    Ok(())
}
EOF

echo "Kompiliere und führe einfachen Test aus..."
cargo build
rustc --extern airlift_node=target/debug/libairlift_node.rlib \
      --extern anyhow=target/debug/deps/libanyhow-*.rlib \
      --extern log=target/debug/deps/liblog-*.rlib \
      --extern env_logger=target/debug/deps/libenv_logger-*.rlib \
      --edition=2021 src/test_simple.rs -o test_simple_program

# Wenn das nicht funktioniert, lass mich stattdessen die main.rs direkt testen
echo "Führe einfachen Test direkt aus..."
cp test_config.toml config.toml.backup
cp test_config.toml config.toml

# Erstelle eine minimal main.rs für den Test
cat > src/main_simple.rs << 'EOF'
mod core;
mod config;
mod producers;
mod processors;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug"))
        .format_timestamp_millis()
        .init();
    
    log::info!("=== Simple Direct Test ===");
    
    // Direkter Test ohne Config
    let mut node = core::AirliftNode::new();
    
    // FileProducer
    let producer_cfg = config::ProducerConfig {
        producer_type: "file".to_string(),
        enabled: true,
        device: None,
        path: Some("background.wav".to_string()),
        channels: Some(2),
        sample_rate: Some(48000),
        loop_audio: Some(true),
    };
    let producer = producers::file::FileProducer::new("test_source", &producer_cfg);
    node.add_producer(Box::new(producer));
    log::info!("Added producer");
    
    // Einfacher Flow
    let mut flow = core::Flow::new("test_flow");
    flow.add_processor(Box::new(core::processor::basic::PassThrough::new("pass")));
    
    // FileConsumer
    let consumer = Box::new(core::consumer::file_writer::FileConsumer::new(
        "recorder", "direct_output.wav"
    ));
    flow.add_consumer(consumer);
    
    node.flows.push(flow);
    
    // Verbinde Producer mit Flow
    if let Err(e) = node.connect_producer_to_flow(0, 0) {
        log::error!("Connection error: {}", e);
    }
    
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();
    
    ctrlc::set_handler(move || {
        log::info!("\nShutdown requested");
        shutdown_clone.store(true, Ordering::SeqCst);
    })?;
    
    node.start()?;
    log::info!("Running for 5 seconds...");
    
    std::thread::sleep(Duration::from_secs(5));
    
    node.stop()?;
    log::info!("Test done. Check direct_output.wav");
    
    Ok(())
}
EOF

# Teste mit einfachem Programm
echo "Teste mit einfachem direkten Setup..."
mv src/main.rs src/main.backup.rs
cp src/main_simple.rs src/main.rs
cargo run
