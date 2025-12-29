use airlift_node::core::ProducerStatus;

#[test]
fn test_producer_status() {
    let status = ProducerStatus {
        running: true,
        connected: true,
        samples_processed: 1000,
        errors: 0,
        buffer_stats: None,
    };

    assert!(status.running);
    assert!(status.connected);
    assert_eq!(status.samples_processed, 1000);
}
