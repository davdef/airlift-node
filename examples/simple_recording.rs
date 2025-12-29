use std::thread;
use std::time::Duration;

use airlift_node::{AirliftNode, Flow};
use airlift_node::core::consumer::file_writer::FileConsumer;
use airlift_node::producers::sine::SineProducer;

fn main() -> anyhow::Result<()> {
    let _ = env_logger::builder().is_test(false).try_init();

    let mut node = AirliftNode::new();

    node.add_producer(Box::new(SineProducer::new("sine_440", 440.0, 48_000)))?;

    let mut flow = Flow::new("simple_recording");
    flow.add_consumer(Box::new(FileConsumer::new("wav_writer", "recording.wav")));

    node.add_flow(flow);
    node.connect_flow_input(0, "producer:sine_440")?;

    node.start()?;
    thread::sleep(Duration::from_secs(3));
    node.stop()?;

    Ok(())
}
