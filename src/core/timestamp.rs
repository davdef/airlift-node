use std::time::{SystemTime, UNIX_EPOCH};

pub fn utc_ns_now() -> u64 {
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    d.as_secs() * 1_000_000_000 + d.subsec_nanos() as u64
}

pub fn format_utc_ns(utc_ns: u64) -> String {
    let seconds = utc_ns / 1_000_000_000;
    let nanos = utc_ns % 1_000_000_000;
    format!("{}.{:09}", seconds, nanos)
}

pub fn ns_since_midnight(utc_ns: u64) -> u64 {
    let seconds_since_epoch = utc_ns / 1_000_000_000;
    let seconds_in_day = 24 * 60 * 60;
    let seconds_since_midnight = seconds_since_epoch % seconds_in_day;
    seconds_since_midnight * 1_000_000_000 + (utc_ns % 1_000_000_000)
}
