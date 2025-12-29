use airlift_node::{ComponentLogger, LogContext};

#[test]
fn test_log_context_creation() {
    let ctx = LogContext::new("Producer", "alsa:default");

    assert_eq!(ctx.component, "Producer");
    assert_eq!(ctx.instance_id, "alsa:default");
    assert!(ctx.sequence > 0);
    assert!(ctx.timestamp_ns > 0);
    assert!(ctx.flow_id.is_none());
}

#[test]
fn test_log_context_with_flow() {
    let ctx = LogContext::new("Flow", "main").with_flow("recording_flow");

    assert_eq!(ctx.flow_id, Some("recording_flow".to_string()));
}

#[test]
fn test_log_formatting() {
    let ctx = LogContext::new("Test", "001");
    let formatted = ctx.format("INFO", "Starting up");

    assert!(formatted.contains("[INFO]"));
    assert!(formatted.contains("[Test:001]"));
    assert!(formatted.contains("Starting up"));

    // Mit Flow
    let ctx_with_flow = ctx.with_flow("main_flow");
    let formatted_with_flow = ctx_with_flow.format("DEBUG", "Processing");

    assert!(formatted_with_flow.contains("flow=main_flow"));
}

#[test]
fn test_component_logger_trait() {
    struct MockComponent {
        id: String,
    }

    impl MockComponent {
        fn new(id: &str) -> Self {
            Self { id: id.to_string() }
        }
    }

    impl ComponentLogger for MockComponent {
        fn log_context(&self) -> LogContext {
            LogContext::new("Mock", &self.id)
        }
    }

    let component = MockComponent::new("test_001");
    let ctx = component.log_context();

    assert_eq!(ctx.component, "Mock");
    assert_eq!(ctx.instance_id, "test_001");
}
