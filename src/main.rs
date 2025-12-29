use airlift_node::{
    api,
    app,
    config,
    core,
    producers,
    monitoring,
};

use airlift_node::app::init::build_plugin_registry;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    log::info!("=== Airlift Node v0.3.0 ===");

    let args: Vec<String> = std::env::args().collect();

    if args.len() > 1 {
        match args[1].as_str() {
            "--discover" => return run_discovery(),
            "--test-device" => {
                if args.len() > 2 {
                    return test_device(&args[2]);
                } else {
                    log::error!("Please specify device ID: cargo run -- --test-device <device_id>");
                    return Ok(());
                }
            }
            _ => {}
        }
    }

    run_normal_mode()
}

fn run_discovery() -> anyhow::Result<()> {
    log::info!("Starting ALSA device discovery...");

    use airlift_node::core::device_scanner::DeviceScanner;
    let scanner = producers::alsa::AlsaDeviceScanner;

    match scanner.scan_devices() {
        Ok(devices) => {
            log::info!("Found {} audio devices", devices.len());

            let json = serde_json::to_string_pretty(&devices)?;
            println!("{}", json);

            for device in &devices {
                log::info!(
                    "[{}] {} - {} (max channels: {}, rates: {:?})",
                    device.id,
                    device.name,
                    match device.device_type {
                        crate::core::device_scanner::DeviceType::Input => "Input",
                        crate::core::device_scanner::DeviceType::Output => "Output",
                        crate::core::device_scanner::DeviceType::Duplex => "Duplex",
                    },
                    device.max_channels,
                    device.supported_rates
                );
            }
        }
        Err(e) => {
            log::error!("Failed to scan devices: {}", e);
            anyhow::bail!("Discovery failed: {}", e);
        }
    }

    Ok(())
}

fn test_device(device_id: &str) -> anyhow::Result<()> {
    log::info!("Testing device: {}", device_id);

    use crate::core::device_scanner::DeviceScanner;
    let scanner = producers::alsa::AlsaDeviceScanner;

    match scanner.test_device(device_id, 3000) {
        Ok(result) => {
            log::info!("Test completed for device: {}", device_id);
            log::info!("Passed: {}", result.test_passed);

            if let Some(ref format) = result.detected_format {
                log::info!(
                    "Detected format: {}-bit {} @ {}Hz, {} channel{}",
                    format.bit_depth,
                    match format.sample_type {
                        crate::core::device_scanner::SampleType::SignedInteger => "SInt",
                        crate::core::device_scanner::SampleType::Float => "Float",
                    },
                    format.sample_rate,
                    format.channels,
                    if format.channels > 1 { "s" } else { "" }
                );
            }

            if !result.warnings.is_empty() {
                log::warn!("Warnings:");
                for warning in &result.warnings {
                    log::warn!("  - {}", warning);
                }
            }

            if !result.errors.is_empty() {
                log::error!("Errors:");
                for error in &result.errors {
                    log::error!("  - {}", error);
                }
            }

            let json = serde_json::to_string_pretty(&result)?;
            println!("{}", json);
        }
        Err(e) => {
            log::error!("Device test failed: {}", e);
            anyhow::bail!("Test failed: {}", e);
        }
    }

    Ok(())
}

fn run_normal_mode() -> anyhow::Result<()> {
    let config = config::Config::load("config.toml").unwrap_or_else(|e| {
        log::warn!("Config error: {}, using defaults", e);
        config::Config::default()
    });

    let config = Arc::new(Mutex::new(config));

    let api_bind =
        std::env::var("AIRLIFT_CONFIG_API_BIND").unwrap_or_else(|_| "127.0.0.1:8080".to_string());
    api::config::start_config_api(&api_bind, config.clone())?;

    let config_snapshot = config
        .lock()
        .map_err(|_| anyhow::anyhow!("config lock poisoned"))?
        .clone();

    log::info!("Node: {}", config_snapshot.node_name);


use airlift_node::app::init::{
    PluginRegistry,
    build_plugin_registry,
};

    let node = Arc::new(Mutex::new(core::AirliftNode::new()));

    let mut processor_registry = PluginRegistry::new();

    build_plugin_registry();

    {
        let mut node = node
            .lock()
            .map_err(|_| anyhow::anyhow!("node lock poisoned"))?;
        // Producer aus Config laden
        for (name, producer_cfg) in &config_snapshot.producers {
            if !producer_cfg.enabled {
                continue;
            }

            match producer_cfg.producer_type.as_str() {
                "file" => {
                    let producer = producers::file::FileProducer::new(name, producer_cfg);
                    if let Err(e) = node.add_producer(Box::new(producer)) {
                        log::error!("Failed to add file producer {}: {}", name, e);
                    } else {
                        log::info!("Added file producer: {}", name);
                    }
                }
                "alsa_input" => match producers::alsa::AlsaProducer::new(name, producer_cfg) {
                    Ok(producer) => {
                        if let Err(e) = node.add_producer(Box::new(producer)) {
                            log::error!("Failed to add ALSA input producer {}: {}", name, e);
                        } else {
                            log::info!("Added ALSA input producer: {}", name);
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to create ALSA producer {}: {}", name, e);
                    }
                },
                "alsa_output" => {
                    match producers::alsa::AlsaOutputCapture::new(name, producer_cfg) {
                        Ok(producer) => {
                            if let Err(e) = node.add_producer(Box::new(producer)) {
                                log::error!("Failed to add ALSA output capture {}: {}", name, e);
                            } else {
                                log::info!("Added ALSA output capture: {}", name);
                            }
                        }
                        Err(e) => {
                            log::error!("Failed to create output capture {}: {}", name, e);
                        }
                    }
                }

                "sine" => {
                    let freq: f32 = producer_cfg
                        .config
                        .get("frequency")
                        .and_then(|v: &serde_json::Value| v.as_f64())
                        .map(|f| f as f32)
                        .unwrap_or(440.0);
                    let rate = producer_cfg.sample_rate.unwrap_or(48000);
                    let producer = producers::sine::SineProducer::new(name, freq, rate);
                    if let Err(e) = node.add_producer(Box::new(producer)) {
                        log::error!("Failed to add sine producer {}: {}", name, e);
                    } else {
                        log::info!("Added sine producer: {} ({} Hz)", name, freq);
                    }
                }

                _ => log::error!("Unknown producer type: {}", producer_cfg.producer_type),
            }
        }

        let plugin_registry = build_plugin_registry();

        // Flows aus Config erstellen und Processors hinzufügen
        for (flow_name, flow_cfg) in &config_snapshot.flows {
            if !flow_cfg.enabled {
                continue;
            }

            let mut flow = core::Flow::new(flow_name);

            // Processors zum Flow hinzufügen
            for processor_name in &flow_cfg.processors {
                if let Some(processor_cfg) = config_snapshot.processors.get(processor_name) {
                    if !processor_cfg.enabled {
                        continue;
                    }

                    match plugin_registry.create_processor(processor_name, processor_cfg) {
                        Ok(processor) => {
                            flow.add_processor(processor);
                            log::info!(
                                "Added processor '{}' (type: {}) to flow '{}'",
                                processor_name,
                                processor_cfg.processor_type,
                                flow_name
                            );
                        }
                        Err(e) => {
                            log::error!(
                                "Failed to create processor '{}' (type: {}): {}",
                                processor_name,
                                processor_cfg.processor_type,
                                e
                            );
                        }
                    }
                }
            }

            node.flows.push(flow);
            log::info!("Added flow: {}", flow_name);
        }

        // Consumer zu Flows hinzufügen (basierend auf Flow outputs)
        for (flow_name, flow_cfg) in &config_snapshot.flows {
            if !flow_cfg.enabled {
                continue;
            }

            for flow in node.flows.iter_mut() {
                if flow.name == *flow_name {
                    for output_name in &flow_cfg.outputs {
                        // Finde Consumer mit diesem Namen
                        if let Some(consumer_cfg) = config_snapshot.consumers.get(output_name) {
                            if !consumer_cfg.enabled {
                                continue;
                            }

                            match consumer_cfg.consumer_type.as_str() {
                                "file" => {
                                    if let Some(path) = &consumer_cfg.path {
                                        let consumer = Box::new(
                                            core::consumer::file_writer::FileConsumer::new(
                                                output_name,
                                                path,
                                            ),
                                        );
                                        flow.add_consumer(consumer);
                                        log::info!(
                                            "Added FileConsumer '{}' to flow '{}' (output: {})",
                                            output_name,
                                            flow_name,
                                            path
                                        );
                                    }
                                }
                                _ => log::error!(
                                    "Unknown consumer type for '{}': {}",
                                    output_name,
                                    consumer_cfg.consumer_type
                                ),
                            }
                        }
                    }
                    break;
                }
            }
        }

        // Producer mit Flows verbinden (basierend auf Flow inputs)
        // UND: Mixer-Inputs verbinden (special case)
        for (flow_name, flow_cfg) in &config_snapshot.flows {
            if !flow_cfg.enabled {
                continue;
            }

            for (flow_index, flow) in node.flows.iter().enumerate() {
                if flow.name == *flow_name {
                    // Normale Producer-Verbindungen
                    for input_name in &flow_cfg.inputs {
                        let mut connected = false;
                        // Finde Producer mit diesem Namen
                        for producer in node.producers().iter() {
                            if producer.name() == input_name {
                                let buffer_name = format!("producer:{}", input_name);

let flow_index = node.flows.len() - 1;

if let Err(e) = node.connect_flow_input(flow_index, &buffer_name) {

                                    log::error!(
                                        "Failed to connect {} to flow {}: {}",
                                        input_name,
                                        flow_name,
                                        e
                                    );
                                }
                                connected = true;
                                break;
                            }
                        }
                        if !connected {
                            if let Err(e) = node.connect_flow_input(flow_index, input_name) {
                                log::error!(
                                    "Failed to connect buffer {} to flow {}: {}",
                                    input_name,
                                    flow_name,
                                    e
                                );
                            }
                        }
                    }

                    // Mixer-Inputs konfigurieren
                    for processor_name in &flow_cfg.processors {
                        if let Some(processor_cfg) = config_snapshot.processors.get(processor_name)
                        {
                            if processor_cfg.processor_type == "mixer" {
                                // Finde den Mixer in diesem Flow
                                let processor_index = flow_cfg
                                    .processors
                                    .iter()
                                    .position(|p| p == processor_name)
                                    .unwrap_or(0);

                                // Jetzt müssen wir die Mixer-Inputs verbinden
                                // Dafür brauchen wir den Mixer aus dem Flow
                                // Das ist tricky, weil wir mutable access brauchen...
                                // Einfacher: Mixer-Inputs direkt beim Erstellen verbinden
                                // ODER: Eine neue Methode in Flow hinzufügen
                                let _ = processor_index;
                            }
                        }
                    }
                    break;
                }
            }
        }

        // Einfacher Test: Direkter Producer->Consumer ohne Processing
        log::info!("Setting up direct test connection for FileConsumer...");

        // Test: Erstelle einen simplen Test-Flow mit nur einem Passthrough
        // und verbinde einen Producer direkt
        if let Some(first_producer) = node.producers().first() {
            log::info!(
                "Found producer: {}, setting up test...",
                first_producer.name()
            );
            let buffer_name = format!("producer:{}", first_producer.name());

            // Erstelle einfachen Test-Flow
            let mut test_flow = core::Flow::new("test_recording");
            test_flow.add_processor(Box::new(core::processor::basic::PassThrough::new(
                "test_passthrough",
            )));

            // FileConsumer hinzufügen
            let file_consumer = Box::new(core::consumer::file_writer::FileConsumer::new(
                "test_recorder",
                "test_output.wav",
            ));
            test_flow.add_consumer(file_consumer);

            node.flows.push(test_flow);

            // Verbinde ersten Producer zum Test-Flow

let flow_index = node.flows.len() - 1;

if let Err(e) = node.connect_flow_input(flow_index, &buffer_name) {

                log::error!("Failed to connect test: {}", e);
            }
        }

        // Falls nichts konfiguriert: Demo-Setup
        if node.producers().is_empty() {
            log::info!("No producers configured, adding demo");
            let demo_cfg = config::ProducerConfig {
                producer_type: "file".to_string(),
                enabled: true,
                device: None,
                path: Some("demo.wav".to_string()),
                channels: Some(2),
                sample_rate: Some(48000),
                loop_audio: Some(true),
                config: std::collections::HashMap::new(),
            };
            let demo_producer = producers::file::FileProducer::new("demo", &demo_cfg);
            if let Err(e) = node.add_producer(Box::new(demo_producer)) {
                log::error!("Failed to add demo producer: {}", e);
            }

            // Demo-Flow mit FileConsumer
            let mut demo_flow = core::Flow::new("demo_flow");
            demo_flow.add_processor(Box::new(core::processor::basic::PassThrough::new(
                "demo_passthrough",
            )));

            // FileConsumer für Demo hinzufügen
            let file_consumer = Box::new(core::consumer::file_writer::FileConsumer::new(
                "demo_recorder",
                "output.wav",
            ));
            demo_flow.add_consumer(file_consumer);

            node.flows.push(demo_flow);
            if let Err(e) = node.connect_flow_input(0, "producer:demo") {
                log::error!("Failed to connect demo: {}", e);
            }
        }
    }

    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();

    ctrlc::set_handler(move || {
        log::info!("\nShutdown requested (Ctrl+C)");
        shutdown_clone.store(true, Ordering::SeqCst);
    })?;

    let monitoring_bind = format!("0.0.0.0:{}", config_snapshot.monitoring.http_port);
    monitoring::start_monitoring_server(&monitoring_bind, node.clone())?;

    {
        let mut node = node
            .lock()
            .map_err(|_| anyhow::anyhow!("node lock poisoned"))?;
        node.start().map_err(|e| anyhow::anyhow!(e))?;
    }
    log::info!("Node started. Press Ctrl+C to stop.");

    let mut tick = 0;
    while !shutdown.load(Ordering::Relaxed)
        && node
            .lock()
            .map_err(|_| anyhow::anyhow!("node lock poisoned"))?
            .is_running()
    {
        std::thread::sleep(Duration::from_millis(500));

        tick += 1;
        if tick % 10 == 0 {
            let (status, flow_names) = {
                let node_guard = node
                    .lock()
                    .map_err(|_| anyhow::anyhow!("node lock poisoned"))?;
                let status = node_guard.status();
                let flow_names = node_guard
                    .flows
                    .iter()
                    .map(|flow| flow.name.clone())
                    .collect::<Vec<_>>();
                (status, flow_names)
            };
            log::info!("=== Node Status ===");
            log::info!(
                "Uptime: {}s, Producers: {}, Flows: {}",
                status.uptime_seconds,
                status.producers,
                status.flows
            );

            for (i, p_status) in status.producer_status.iter().enumerate() {
                log::info!("  Producer {}:", i);
                log::info!(
                    "    running={}, connected={}, samples={}",
                    p_status.running,
                    p_status.connected,
                    p_status.samples_processed
                );
            }

            for (i, f_status) in status.flow_status.iter().enumerate() {
                let flow_name = flow_names
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string());
                if !flow_name.is_empty() {
                    log::info!("  Flow {} ('{}'): running={}, input_buffers={}, processor_buffers={}, output={}",
                        i, flow_name, f_status.running,
                        f_status.input_buffer_levels.len(),
                        f_status.processor_buffer_levels.len(),
                        f_status.output_buffer_level);
                }
            }
        }
    }

    {
        let mut node = node
            .lock()
            .map_err(|_| anyhow::anyhow!("node lock poisoned"))?;
        node.stop().map_err(|e| anyhow::anyhow!(e))?;
    }
    log::info!("Node stopped");

    Ok(())
}
