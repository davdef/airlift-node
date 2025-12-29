use std::thread;
use std::time::Duration;

use airlift_node::core::consumer::file_writer::FileConsumer;
use airlift_node::processors::{MixerConfig, MixerInputConfig};
use airlift_node::producers::sine::SineProducer;
use airlift_node::{AirliftNode, Flow};

fn main() -> anyhow::Result<()> {
    let _ = env_logger::builder().is_test(false).try_init();

    let mut node = AirliftNode::new();

    node.add_producer(Box::new(SineProducer::new("sine_left", 440.0, 48_000)))?;
    node.add_producer(Box::new(SineProducer::new("sine_right", 660.0, 48_000)))?;

    let mut flow = Flow::new("mix_flow");
    flow.add_consumer(Box::new(FileConsumer::new("mix_output", "mix.wav")));
    node.add_flow(flow);

    let mixer_config = MixerConfig {
        inputs: vec![
            MixerInputConfig {
                name: "tone_left".to_string(),
                source: "producer:sine_left".to_string(),
                gain: 0.6,
                enabled: Some(true),
            },
            MixerInputConfig {
                name: "tone_right".to_string(),
                source: "producer:sine_right".to_string(),
                gain: 0.4,
                enabled: Some(true),
            },
        ],
        output_sample_rate: Some(48_000),
        output_channels: Some(2),
        master_gain: Some(0.9),
        auto_connect: Some(true),
    };

    node.create_and_add_mixer(0, "main_mixer", mixer_config)?;
    node.connect_flow_input(0, "producer:sine_left")?;
    node.connect_flow_input(0, "producer:sine_right")?;

    node.start()?;
    thread::sleep(Duration::from_secs(3));
    node.stop()?;

    Ok(())
}
