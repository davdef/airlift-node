use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use crate::codecs::EncodedFrame;
use crate::ring::{EncodedFramePacket, EncodedSink, EncodedSource, RingStats};

#[derive(Clone)]
pub struct EncodedSlot {
    pub seq: u64,
    pub utc_ns: u64,
    pub frame: Arc<EncodedFrame>,
}

struct Inner {
    cap: usize,
    slots: Vec<EncodedSlot>,
    head_seq: u64,
}

#[derive(Clone)]
pub struct EncodedRing {
    inner: Arc<Mutex<Inner>>,
    next_seq: Arc<AtomicU64>,
    notify: Arc<Condvar>,
}

#[derive(Clone)]
pub struct EncodedRingReader {
    ring: EncodedRing,
    last_seq: u64,
}

pub enum EncodedRingRead {
    Frame { frame: EncodedFrame, utc_ns: u64 },
    Gap { missed: u64 },
    Empty,
}

impl EncodedRing {
    pub fn new(cap: usize, default_frame: EncodedFrame) -> Self {
        let mut slots = Vec::with_capacity(cap);
        for _ in 0..cap {
            slots.push(EncodedSlot {
                seq: 0,
                utc_ns: 0,
                frame: Arc::new(default_frame.clone()),
            });
        }

        let inner = Inner {
            cap,
            slots,
            head_seq: 0,
        };

        Self {
            inner: Arc::new(Mutex::new(inner)),
            next_seq: Arc::new(AtomicU64::new(1)),
            notify: Arc::new(Condvar::new()),
        }
    }

    pub fn writer_push(&self, utc_ns: u64, frame: EncodedFrame) -> u64 {
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);

        let mut g = self.inner.lock().unwrap();
        let idx = (seq as usize) % g.cap;
        g.slots[idx] = EncodedSlot {
            seq,
            utc_ns,
            frame: Arc::new(frame),
        };
        g.head_seq = seq;
        self.notify.notify_all();
        seq
    }

    pub fn subscribe(&self) -> EncodedRingReader {
        let head = self.head_seq();
        EncodedRingReader {
            ring: self.clone(),
            last_seq: if head > 0 { head } else { 0 },
        }
    }

    pub fn stats(&self) -> RingStats {
        let g = self.inner.lock().unwrap();
        RingStats {
            capacity: g.cap,
            head_seq: g.head_seq,
            next_seq: self.next_seq.load(Ordering::Relaxed),
            arc_replacements: 0,
        }
    }

    fn head_seq(&self) -> u64 {
        let g = self.inner.lock().unwrap();
        g.head_seq
    }

    fn get_by_seq(&self, seq: u64) -> Option<EncodedSlot> {
        let g = self.inner.lock().unwrap();
        let idx = (seq as usize) % g.cap;
        let slot = &g.slots[idx];
        if slot.seq == seq && seq > 0 {
            Some(slot.clone())
        } else {
            None
        }
    }

    fn cap(&self) -> usize {
        let g = self.inner.lock().unwrap();
        g.cap
    }

    fn wait_for_data(&self, last_seq: u64) {
        let mut guard = self.inner.lock().unwrap();
        while guard.head_seq <= last_seq {
            guard = self.notify.wait(guard).unwrap();
        }
    }

    fn wait_for_data_or_stop(&self, last_seq: u64, stop: &AtomicBool) -> bool {
        let mut guard = self.inner.lock().unwrap();
        while guard.head_seq <= last_seq && !stop.load(Ordering::Relaxed) {
            guard = self.notify.wait(guard).unwrap();
        }
        guard.head_seq > last_seq
    }

    fn notifier(&self) -> Arc<Condvar> {
        self.notify.clone()
    }
}

impl EncodedSink for EncodedRing {
    fn push(&self, frame: EncodedFramePacket) -> anyhow::Result<()> {
        self.writer_push(frame.utc_ns, frame.frame);
        Ok(())
    }
}

impl EncodedRingReader {
    pub fn poll(&mut self) -> EncodedRingRead {
        let head = self.ring.head_seq();
        if head == 0 || head <= self.last_seq {
            return EncodedRingRead::Empty;
        }

        let next = self.last_seq + 1;
        let cap = self.ring.cap() as u64;
        if head.saturating_sub(next) >= cap {
            let missed = head.saturating_sub(next) + 1 - cap;
            self.last_seq = head.saturating_sub(cap - 1);
            return EncodedRingRead::Gap { missed };
        }

        match self.ring.get_by_seq(next) {
            Some(slot) => {
                self.last_seq = next;
                EncodedRingRead::Frame {
                    frame: (*slot.frame).clone(),
                    utc_ns: slot.utc_ns,
                }
            }
            None => EncodedRingRead::Empty,
        }
    }

    pub fn wait_for_read(&mut self) -> EncodedRingRead {
        loop {
            match self.poll() {
                EncodedRingRead::Empty => self.ring.wait_for_data(self.last_seq),
                read => return read,
            }
        }
    }

    pub fn wait_for_read_or_stop(&mut self, stop: &AtomicBool) -> Option<EncodedRingRead> {
        loop {
            if stop.load(Ordering::Relaxed) {
                return None;
            }
            match self.poll() {
                EncodedRingRead::Empty => {
                    if !self.ring.wait_for_data_or_stop(self.last_seq, stop) {
                        return None;
                    }
                }
                read => return Some(read),
            }
        }
    }

    pub fn fill(&self) -> u64 {
        let head = self.ring.head_seq();
        head.saturating_sub(self.last_seq)
    }

    pub fn notifier(&self) -> Arc<Condvar> {
        self.ring.notifier()
    }
}

impl EncodedSource for EncodedRingReader {
    fn poll(&mut self) -> EncodedRingRead {
        EncodedRingReader::poll(self)
    }

    fn wait_for_read(&mut self) -> EncodedRingRead {
        EncodedRingReader::wait_for_read(self)
    }

    fn wait_for_read_or_stop(&mut self, stop: &AtomicBool) -> Option<EncodedRingRead> {
        EncodedRingReader::wait_for_read_or_stop(self, stop)
    }

    fn notifier(&self) -> Option<Arc<Condvar>> {
        Some(self.notifier())
    }
}
