use std::collections::{HashMap, HashSet};

use anyhow::{bail, Context};
use serde_json::Value;

use crate::app::init::build_plugin_registry;
use crate::codecs::supported_codecs;
use crate::config::Config;
use crate::core::consumer::file_writer::FileConsumer;
use crate::core::{AirliftNode, Flow};
use crate::producers;

pub fn apply_config(node: &mut AirliftNode, config: &Config) -> anyhow::Result<()> {
    config
        .validate()
        .context("config validation failed before apply")?;
    validate_config_capabilities(config)?;

    let was_running = node.is_running();
    if was_running {
        node.stop()
            .map_err(|e| anyhow::anyhow!("failed to stop node: {}", e))?;
    }

    node.reset_modules();

    let plugin_registry = build_plugin_registry();

    for (name, producer_cfg) in &config.producers {
        if !producer_cfg.enabled {
            continue;
        }

        match producer_cfg.producer_type.as_str() {
            "file" => {
                let producer = producers::file::FileProducer::new(name, producer_cfg);
                node.add_producer(Box::new(producer))
                    .context("failed to add file producer")?;
            }
            "alsa_input" => {
                let producer = producers::alsa::AlsaProducer::new(name, producer_cfg)
                    .context("failed to create ALSA input producer")?;
                node.add_producer(Box::new(producer))
                    .context("failed to add ALSA input producer")?;
            }
            "alsa_output" => {
                let producer = producers::alsa::AlsaOutputCapture::new(name, producer_cfg)
                    .context("failed to create ALSA output capture producer")?;
                node.add_producer(Box::new(producer))
                    .context("failed to add ALSA output capture producer")?;
            }
            "sine" => {
                let freq: f32 = producer_cfg
                    .config
                    .get("frequency")
                    .and_then(|v| v.as_f64())
                    .map(|f| f as f32)
                    .unwrap_or(440.0);
                let rate = producer_cfg.sample_rate.unwrap_or(48000);
                let producer = producers::sine::SineProducer::new(name, freq, rate);
                node.add_producer(Box::new(producer))
                    .context("failed to add sine producer")?;
            }
            other => bail!("producer '{}' uses unsupported type '{}'", name, other),
        }
    }

    for (flow_name, flow_cfg) in &config.flows {
        if !flow_cfg.enabled {
            continue;
        }

        let mut flow = Flow::new(flow_name);

        for processor_name in &flow_cfg.processors {
            let processor_cfg = config.processors.get(processor_name).with_context(|| {
                format!(
                    "processor '{}' referenced in flow '{}' is missing",
                    processor_name, flow_name
                )
            })?;

            if !processor_cfg.enabled {
                continue;
            }

            let processor = plugin_registry
                .create_processor(processor_name, processor_cfg)
                .with_context(|| {
                    format!(
                        "failed to create processor '{}' (type: {})",
                        processor_name, processor_cfg.processor_type
                    )
                })?;
            flow.add_processor(processor);
        }

        node.add_flow(flow);
    }

    for (flow_name, flow_cfg) in &config.flows {
        if !flow_cfg.enabled {
            continue;
        }

        let flow_index = node
            .flow_index_by_name(flow_name)
            .with_context(|| format!("flow '{}' missing after configuration", flow_name))?;

        for output_name in &flow_cfg.outputs {
            let consumer_cfg = config.consumers.get(output_name).with_context(|| {
                format!(
                    "consumer '{}' referenced in flow '{}' is missing",
                    output_name, flow_name
                )
            })?;

            if !consumer_cfg.enabled {
                continue;
            }

            match consumer_cfg.consumer_type.as_str() {
                "file" => {
                    let path = consumer_cfg.path.as_ref().with_context(|| {
                        format!(
                            "consumer '{}' in flow '{}' missing output path",
                            output_name, flow_name
                        )
                    })?;
                    let consumer = Box::new(FileConsumer::new(output_name, path));
                    node.add_consumer_to_flow(flow_index, consumer)
                        .context("failed to add consumer to flow")?;
                }
                other => bail!(
                    "consumer '{}' uses unsupported type '{}'",
                    output_name,
                    other
                ),
            }
        }
    }

    for (flow_name, flow_cfg) in &config.flows {
        if !flow_cfg.enabled {
            continue;
        }

        let flow_index = node
            .flow_index_by_name(flow_name)
            .with_context(|| format!("flow '{}' missing after configuration", flow_name))?;

        for input_name in &flow_cfg.inputs {
            let buffer_name = if config.producers.contains_key(input_name) {
                format!("producer:{}", input_name)
            } else {
                input_name.to_string()
            };
            node.connect_flow_input(flow_index, &buffer_name)
                .with_context(|| {
                    format!(
                        "failed to connect input '{}' to flow '{}'",
                        input_name, flow_name
                    )
                })?;
        }
    }

    if was_running {
        let event_bus = node.event_bus();
        let mut event_bus = event_bus
            .lock()
            .map_err(|_| anyhow::anyhow!("event bus lock poisoned"))?;
        event_bus.start().context("failed to restart event bus")?;
        node.start()
            .map_err(|e| anyhow::anyhow!("failed to start node: {}", e))?;
    }

    Ok(())
}

pub fn validate_config_capabilities(config: &Config) -> anyhow::Result<()> {
    let producer_types = supported_producer_types();
    let processor_types = supported_processor_types();
    let consumer_types = supported_consumer_types();

    for (name, producer_cfg) in &config.producers {
        if !producer_types.contains(producer_cfg.producer_type.as_str()) {
            bail!(
                "producer '{}' has unsupported type '{}'",
                name,
                producer_cfg.producer_type
            );
        }
        validate_codec_config(&producer_cfg.config, "producer", name)?;
    }

    for (name, processor_cfg) in &config.processors {
        if !processor_types.contains(processor_cfg.processor_type.as_str()) {
            bail!(
                "processor '{}' has unsupported type '{}'",
                name,
                processor_cfg.processor_type
            );
        }
        validate_codec_config(&processor_cfg.config, "processor", name)?;
    }

    for (name, consumer_cfg) in &config.consumers {
        if !consumer_types.contains(consumer_cfg.consumer_type.as_str()) {
            bail!(
                "consumer '{}' has unsupported type '{}'",
                name,
                consumer_cfg.consumer_type
            );
        }
        validate_codec_config(&consumer_cfg.config, "consumer", name)?;
    }

    Ok(())
}

fn supported_producer_types() -> HashSet<&'static str> {
    ["file", "alsa_input", "alsa_output", "sine"]
        .into_iter()
        .collect()
}

fn supported_processor_types() -> HashSet<&'static str> {
    ["passthrough", "gain", "mixer"].into_iter().collect()
}

fn supported_consumer_types() -> HashSet<&'static str> {
    ["file"].into_iter().collect()
}

fn validate_codec_config(
    config: &HashMap<String, Value>,
    module_kind: &str,
    module_name: &str,
) -> anyhow::Result<()> {
    let supported = supported_codec_ids();

    for key in ["codec", "codec_id"] {
        if let Some(value) = config.get(key) {
            let codec_id = value
                .as_str()
                .with_context(|| {
                    format!(
                        "{} '{}' has non-string {} entry",
                        module_kind, module_name, key
                    )
                })?
                .to_lowercase();
            if !supported.contains(&codec_id) {
                bail!(
                    "{} '{}' references unsupported codec '{}'",
                    module_kind,
                    module_name,
                    codec_id
                );
            }
        }
    }

    Ok(())
}

fn supported_codec_ids() -> HashSet<String> {
    supported_codecs()
        .into_iter()
        .map(|info| format!("{:?}", info.kind).to_lowercase())
        .collect()
}
