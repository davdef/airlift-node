use airlift_node::{
    api,
    config,
    core,
    producers,
};

use airlift_node::app::init::{build_plugin_registry, PluginRegistry};

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
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
                    log::error!("Usage: cargo run -- --test-device <device_id>");
                    return Ok(());
                }
            }
            _ => {}
        }
    }

    run_normal_mode()
}

fn run_discovery() -> anyhow::Result<()> {
    use airlift_node::core::device_scanner::DeviceScanner;
    let scanner = producers::alsa::AlsaDeviceScanner;

    log::info!("Starting ALSA device discovery…");
    let devices = scanner.scan_devices()?;
    println!("{}", serde_json::to_string_pretty(&devices)?);
    Ok(())
}

fn test_device(device_id: &str) -> anyhow::Result<()> {
    use airlift_node::core::device_scanner::DeviceScanner;
    let scanner = producers::alsa::AlsaDeviceScanner;

    log::info!("Testing device {}", device_id);
    let result = scanner.test_device(device_id, 3000)?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

fn run_normal_mode() -> anyhow::Result<()> {
    let cfg = config::Config::load("config.toml")
        .unwrap_or_else(|e| {
            log::warn!("Config error: {}, using defaults", e);
            config::Config::default()
        });

    let cfg = Arc::new(Mutex::new(cfg));
    let node = Arc::new(Mutex::new(core::AirliftNode::new()));

    let snapshot = cfg.lock().unwrap().clone();
    log::info!("Node: {}", snapshot.node_name);

    let api_bind = format!("0.0.0.0:{}", snapshot.monitoring.http_port);
    api::start_api_server(&api_bind, cfg.clone(), node.clone())?;

    let plugin_registry: PluginRegistry = build_plugin_registry();

    {
        let mut node = node.lock().unwrap();

        /* ---------------- Producers ---------------- */
        for (name, p_cfg) in &snapshot.producers {
            if !p_cfg.enabled {
                continue;
            }

            match p_cfg.producer_type.as_str() {
                "sine" => {
                    let freq = p_cfg.config
                        .get("frequency")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(440.0) as f32;
                    let rate = p_cfg.sample_rate.unwrap_or(48_000);

                    node.add_producer(Box::new(
                        producers::sine::SineProducer::new(name, freq, rate),
                    ))?;

                    log::info!("Added sine producer '{}' ({} Hz)", name, freq);
                }
                other => {
                    log::error!("Unsupported producer type '{}'", other);
                }
            }
        }

        /* ---------------- Flows ---------------- */
        for (flow_name, flow_cfg) in &snapshot.flows {
            if !flow_cfg.enabled {
                continue;
            }

            let mut flow = core::Flow::new(flow_name);

            // Processors
            for proc_name in &flow_cfg.processors {
                if let Some(proc_cfg) = snapshot.processors.get(proc_name) {
                    if proc_cfg.enabled {
                        let p = plugin_registry.create_processor(proc_name, proc_cfg)?;
                        flow.add_processor(p);
                        log::info!(
                            "Added processor '{}' to flow '{}'",
                            proc_name, flow_name
                        );
                    }
                }
            }

            // Consumers
            for out_name in &flow_cfg.outputs {
                if let Some(c_cfg) = snapshot.consumers.get(out_name) {
                    if !c_cfg.enabled {
                        continue;
                    }

                    match c_cfg.consumer_type.as_str() {
                        "file" => {
                            let path = c_cfg.path.as_ref().unwrap();
                            flow.add_consumer(Box::new(
                                core::consumer::file_writer::FileConsumer::new(out_name, path),
                            ));
                            log::info!(
                                "Added FileConsumer '{}' to flow '{}' ({})",
                                out_name, flow_name, path
                            );
                        }
                        other => {
                            log::error!("Unsupported consumer type '{}'", other);
                        }
                    }
                }
            }

            node.add_flow(flow);
            log::info!("Added flow '{}'", flow_name);
        }

        /* ---------------- Connections (BORROW-SAFE) ---------------- */
        let flow_count = node.flows.len();

        for flow_index in 0..flow_count {
            let flow_name = node.flows[flow_index].name.clone();

            if let Some(flow_cfg) = snapshot.flows.get(&flow_name) {
                for input in &flow_cfg.inputs {
                    let buffer_name = format!("producer:{}", input);
                    node.connect_flow_input(flow_index, &buffer_name)?;
                }
            }
        }

        node.start()?;
    }

    log::info!("Node started. Press Ctrl+C to stop.");

    let shutdown = Arc::new(AtomicBool::new(false));
    let s = shutdown.clone();
    ctrlc::set_handler(move || {
        log::info!("Shutdown requested");
        s.store(true, Ordering::SeqCst);
    })?;

    while !shutdown.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(500));
    }

    node.lock().unwrap().stop()?;
    log::info!("Node stopped");
    Ok(())
}

