use std::time::{Duration, Instant};

use airlift_node::core::{AirliftNode, Flow};
use airlift_node::core::processor::basic::PassThrough;
use airlift_node::testing::mocks::{MockProducer, MockConsumer};
use airlift_node::PcmFrame;

#[test]
fn flow_with_processor_and_consumer_runs() -> anyhow::Result<()> {
    let frames = vec![
        PcmFrame {
            utc_ns: 1,
            samples: vec![1, 2, 3, 4],
            sample_rate: 48_000,
            channels: 2,
        },
    ];

    let producer = MockProducer::new("test", frames);
    let (consumer, received) = MockConsumer::new_with_shared("out");

    let mut flow = Flow::new("flow");
    flow.add_processor(Box::new(PassThrough::new("pass")));
    flow.add_consumer(Box::new(consumer));

    let mut node = AirliftNode::new();
    node.add_flow(flow);
    node.add_producer(Box::new(producer))?;
    node.connect_flow_input(0, "producer:test")?;

    node.start()?;

    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline {
        if !received.lock().unwrap().is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    node.stop()?;

    assert!(!received.lock().unwrap().is_empty());
    Ok(())
}
