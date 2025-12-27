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
