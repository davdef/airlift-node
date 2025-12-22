use log::{error, info};
use std::sync::{Arc, atomic::AtomicBool};

use crate::agent;
use crate::config::Config;
use crate::monitoring::{self, Metrics, RingStatsSource};

pub struct MonitoringService;

impl MonitoringService {
    pub fn start(
        cfg: &Config,
        agent: &agent::Agent,
        metrics: Arc<Metrics>,
        running: Arc<AtomicBool>,
    ) -> anyhow::Result<()> {
        monitoring::create_health_file()?;

        let ring: Arc<dyn RingStatsSource> = Arc::new(agent.ring.clone());
        let port = cfg.monitoring.http_port;

        std::thread::spawn(move || {
            if let Err(e) = monitoring::run_metrics_server(metrics, ring, port, running) {
                error!("[monitoring] error: {}", e);
            }
        });

        info!("[airlift] monitoring on port {}", port);
        Ok(())
    }

    pub fn mark_shutdown() -> anyhow::Result<()> {
        monitoring::update_health_status(false)
    }
}
