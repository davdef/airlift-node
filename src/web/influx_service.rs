use crate::io::influx_out::InfluxOut;
use crate::io::peak_analyzer::PeakEvent;

pub struct InfluxService {
    inner: InfluxOut,
}

impl InfluxService {
    pub fn new(url: String, db: String, min_interval_ms: u64) -> Self {
        Self {
            inner: InfluxOut::new(url, db, min_interval_ms),
        }
    }

    pub fn handle_peak(&self, evt: &PeakEvent) {
        self.inner.handle(evt);
    }
}
