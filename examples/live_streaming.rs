use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use airlift_node::{AirliftNode, Flow};
use airlift_node::core::consumer::encoded_output::EncodedOutputConsumer;
use airlift_node::encoders::{PcmCodec, PCM_CHANNELS, PCM_I16_SAMPLES, PCM_SAMPLE_RATE};
use airlift_node::producers::sine::SineProducer;
use airlift_node::ring::{EncodedRing, EncodedRingRead};
use airlift_node::types::{CodecInfo, CodecKind, ContainerKind, EncodedFrame};

fn main() -> anyhow::Result<()> {
    let _ = env_logger::builder().is_test(false).try_init();

    let mut node = AirliftNode::new();

    node.add_producer(Box::new(SineProducer::new("live_sine", 220.0, 48_000)))?;

    let codec_info = CodecInfo {
        kind: CodecKind::Pcm,
        sample_rate: PCM_SAMPLE_RATE,
        channels: PCM_CHANNELS,
        container: ContainerKind::Raw,
    };
    let default_frame = EncodedFrame {
        payload: vec![0; PCM_I16_SAMPLES * 2],
        info: codec_info,
    };
    let ring = EncodedRing::new(128, default_frame);

    let mut flow = Flow::new("live_stream");
    let consumer = EncodedOutputConsumer::new(
        "encoded_output",
        Box::new(PcmCodec::new()),
        Arc::new(ring.clone()),
    );
    flow.add_consumer(Box::new(consumer));

    node.add_flow(flow);
    node.connect_flow_input(0, "producer:live_sine")?;

    node.start()?;

    let mut reader = ring.subscribe();
    let start = Instant::now();
    let mut frames = 0u64;
    while start.elapsed() < Duration::from_secs(2) {
        match reader.poll() {
            EncodedRingRead::Frame { .. } => frames += 1,
            EncodedRingRead::Gap { missed } => frames += missed,
            EncodedRingRead::Empty => thread::sleep(Duration::from_millis(10)),
        }
    }

    log::info!("streamed {} encoded frames", frames);

    node.stop()?;

    Ok(())
}
