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
