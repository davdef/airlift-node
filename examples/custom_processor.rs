use std::thread;
use std::time::Duration;

use airlift_node::{AirliftNode, AudioRingBuffer, Flow};
use airlift_node::core::consumer::file_writer::FileConsumer;
use airlift_node::core::processor::{Processor, ProcessorStatus};
use airlift_node::producers::sine::SineProducer;

struct ScaleProcessor {
    name: String,
    scale: f32,
}

impl ScaleProcessor {
    fn new(name: &str, scale: f32) -> Self {
        Self {
            name: name.to_string(),
            scale,
        }
    }
}

impl Processor for ScaleProcessor {
    fn name(&self) -> &str {
        &self.name
    }

    fn process(&mut self, input_buffer: &AudioRingBuffer, output_buffer: &AudioRingBuffer) -> anyhow::Result<()> {
        while let Some(mut frame) = input_buffer.pop() {
            for sample in frame.samples.iter_mut() {
                *sample = (*sample as f32 * self.scale)
                    .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
            }
            output_buffer.push(frame);
        }
        Ok(())
    }

    fn status(&self) -> ProcessorStatus {
        ProcessorStatus {
            running: true,
            processing_rate_hz: 0.0,
            latency_ms: 0.0,
            errors: 0,
        }
    }

    fn update_config(&mut self, config: serde_json::Value) -> anyhow::Result<()> {
        if let Some(scale) = config.get("scale").and_then(|value| value.as_f64()) {
            self.scale = scale as f32;
        }
        Ok(())
    }
}

fn main() -> anyhow::Result<()> {
    let _ = env_logger::builder().is_test(false).try_init();

    let mut node = AirliftNode::new();

    node.add_producer(Box::new(SineProducer::new("custom_sine", 330.0, 48_000)))?;

    let mut flow = Flow::new("custom_processor_flow");
    flow.add_processor(Box::new(ScaleProcessor::new("scale", 0.5)));
    flow.add_consumer(Box::new(FileConsumer::new("custom_output", "scaled.wav")));

    node.add_flow(flow);
    node.connect_flow_input(0, "producer:custom_sine")?;

    node.start()?;
    thread::sleep(Duration::from_secs(3));
    node.stop()?;

    Ok(())
}
