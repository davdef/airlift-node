use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

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
    pub fn new(cap: usize) -> Self {
        let mut slots = Vec::with_capacity(cap);
        for _ in 0..cap {
            slots.push(EncodedSlot {
                seq: 0,
                utc_ns: 0,
                frame: Arc::new(EncodedFrame {
                    payload: Vec::new(),
                    info: crate::codecs::CodecInfo {
                        kind: crate::codecs::CodecKind::OpusOgg,
                        sample_rate: 48_000,
                        channels: 2,
                        container: crate::codecs::ContainerKind::Ogg,
                    },
                }),
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

    pub fn fill(&self) -> u64 {
        let head = self.ring.head_seq();
        head.saturating_sub(self.last_seq)
    }
}

impl EncodedSource for EncodedRingReader {
    fn poll(&mut self) -> EncodedRingRead {
        EncodedRingReader::poll(self)
    }
}
