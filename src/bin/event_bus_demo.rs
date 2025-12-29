// src/bin/event_bus_demo.rs
use airlift_node::core::{EventBus, EventLoggerHandler, EventStatsHandler, Event, EventType, EventPriority};
use std::sync::Arc;

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .init();
    
    println!("=== EventBus Demo ===\n");
    
    // EventBus erstellen
    let mut event_bus = EventBus::new("demo_bus");
    
    // Handler registrieren
    let logger_handler = Arc::new(EventLoggerHandler::new("demo_logger", EventPriority::Debug));
    event_bus.register_handler(logger_handler)?;
    
    let stats_handler = Arc::new(EventStatsHandler::new("demo_stats"));
    event_bus.register_handler(stats_handler.clone())?;
    
    // EventBus starten
    event_bus.start()?;
    
    // Events publizieren
    for i in 0..5 {
        let event = Event::new(
            EventType::BufferCreated,
            EventPriority::Info,
            "Demo",
            "instance1",
            serde_json::json!({
                "buffer_id": i,
                "name": format!("buffer_{}", i),
                "capacity": 100 * (i + 1),
            }),
        );
        
        event_bus.publish(event)?;
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    
    // Kritischen Error simulieren
    let error_event = Event::new(
        EventType::CriticalError,
        EventPriority::Error,
        "Demo",
        "instance1",
        serde_json::json!({
            "error": "Simulated error",
            "component": "demo_component",
            "recommendation": "Check configuration",
        }),
    );
    
    event_bus.publish(error_event)?;
    
    // Warten und Statistiken anzeigen
    std::thread::sleep(std::time::Duration::from_millis(500));
    
    if let Ok(stats) = stats_handler.get_stats() {
        println!("\n=== Event Statistics ===");
        println!("Total events: {}", stats.total_events);
        println!("Events by type:");
        for (type_name, count) in &stats.events_by_type {
            println!("  {}: {}", type_name, count);
        }
    }
    
    // EventBus stoppen
    event_bus.stop()?;
    
    println!("\n✅ EventBus Demo completed!");
    Ok(())
}
