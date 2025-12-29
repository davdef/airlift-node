use std::time::{Duration, Instant};

use airlift_node::core::processor::basic::PassThrough;
use airlift_node::core::{AirliftNode, Flow};
use airlift_node::testing::mocks::{MockConsumer, MockProducer};
use airlift_node::PcmFrame;

#[test]
fn e2e_producer_processor_consumer_flow() -> anyhow::Result<()> {
    let frames = vec![
        PcmFrame {
            utc_ns: 1,
            samples: vec![1, 2, 3, 4],
            sample_rate: 48_000,
            channels: 2,
        },
        PcmFrame {
            utc_ns: 2,
            samples: vec![5, 6, 7, 8],
            sample_rate: 48_000,
            channels: 2,
        },
    ];

    let producer = MockProducer::new("mock_producer", frames.clone());
    let (consumer, received_frames) = MockConsumer::new_with_shared("mock_consumer");

    let mut flow = Flow::new("e2e_flow");
    flow.add_processor(Box::new(PassThrough::new("passthrough")));
    flow.add_consumer(Box::new(consumer));

    let mut node = AirliftNode::new();
    node.add_flow(flow);
    node.add_producer(Box::new(producer))?;
    node.connect_registered_buffer_to_flow("producer:mock_producer", 0)?;

    node.start()?;

    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if received_frames.lock().expect("lock frames").len() >= frames.len() {
            break;
        }
        if Instant::now() > deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    node.stop()?;

    let collected = received_frames.lock().expect("lock frames").clone();
    assert!(
        collected.len() >= frames.len(),
        "expected at least {} frames, got {}",
        frames.len(),
        collected.len()
    );
    assert_eq!(collected[0].samples, frames[0].samples);

    Ok(())
}
