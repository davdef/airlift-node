use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use anyhow::{Context, Result};

use crate::producers::wait::StopWait;

// Timing constants for output capture loops.
const STOP_WAIT_IDLE_MS: u64 = 1;
const STOP_WAIT_ERROR_MS: u64 = 10;
const DEMO_TICK_INTERVAL_MS: u64 = 100;
const DEMO_LOG_EVERY_TICKS: u64 = 10;

pub struct AlsaOutputCapture {
    name: String,
    running: Arc<AtomicBool>,
    samples_processed: Arc<AtomicU64>,
    config: crate::config::ProducerConfig,
    thread_handle: Option<std::thread::JoinHandle<()>>,
    ring_buffer: Option<Arc<crate::core::AudioRingBuffer>>,
    stop_wait: Arc<StopWait>,
}

impl AlsaOutputCapture {
    pub fn new(name: &str, config: &crate::config::ProducerConfig) -> Result<Self> {
        Ok(Self {
            name: name.to_string(),
            running: Arc::new(AtomicBool::new(false)),
            samples_processed: Arc::new(AtomicU64::new(0)),
            config: config.clone(),
            thread_handle: None,
            ring_buffer: None,
            stop_wait: Arc::new(StopWait::new()),
        })
    }
}

impl crate::core::Producer for AlsaOutputCapture {
    fn name(&self) -> &str {
        &self.name
    }
    
    fn start(&mut self) -> Result<()> {
        if self.running.load(Ordering::Relaxed) {
            return Ok(());
        }
        
        log::info!("ALSA Output Capture '{}' starting...", self.name);
        
        // Für Output-Capture: loopback Device verwenden
        // Oder PulseAudio Monitor Source
        let device = self.config.device.clone()
            .unwrap_or_else(|| {
                // Versuche verschiedene Output-Capture Devices
                "pulse".to_string()  // PulseAudio Monitor
                // "hw:Loopback,1,0"  // ALSA Loopback wenn konfiguriert
            });
        
        let sample_rate = self.config.sample_rate.unwrap_or(48000);
        let channels = self.config.channels.unwrap_or(2) as u32;
        
        log::info!("Output Capture config: device={}, rate={}, channels={}", 
            device, sample_rate, channels);
        
        self.running.store(true, Ordering::SeqCst);
        
        let running = self.running.clone();
        let samples_processed = self.samples_processed.clone();
        let name = self.name.clone();
        let ring_buffer = self.ring_buffer.clone();
        let stop_wait = self.stop_wait.clone();
        
        let handle = std::thread::spawn(move || {
            if let Err(e) = Self::capture_output(
                &device, sample_rate, channels, 
                running.clone(), samples_processed.clone(),
                ring_buffer,
                stop_wait,
            ) {
                log::error!("Output Capture '{}' error: {}", name, e);
            }
            log::info!("Output Capture '{}' thread stopped", name);
        });
        
        self.thread_handle = Some(handle);
        Ok(())
    }
    
    fn stop(&mut self) -> Result<()> {
        log::info!("Output Capture '{}' stopping...", self.name);
        self.running.store(false, Ordering::SeqCst);
        self.stop_wait.notify_all();
        
        if let Some(handle) = self.thread_handle.take() {
            if let Err(e) = handle.join() {
                log::error!("Failed to join output capture thread: {:?}", e);
            }
        }
        
        Ok(())
    }
    
    fn status(&self) -> crate::core::ProducerStatus {
        crate::core::ProducerStatus {
            running: self.running.load(Ordering::Relaxed),
            connected: true,
            samples_processed: self.samples_processed.load(Ordering::Relaxed),
            errors: 0,
            buffer_stats: self.ring_buffer.as_ref().map(|b| b.stats()),
        }
    }
    
    fn attach_ring_buffer(&mut self, buffer: Arc<crate::core::AudioRingBuffer>) {
        self.ring_buffer = Some(buffer);
    }
}

impl AlsaOutputCapture {
    fn capture_output(
        device: &str,
        sample_rate: u32,
        channels: u32,
        running: Arc<AtomicBool>,
        samples_processed: Arc<AtomicU64>,
        ring_buffer: Option<Arc<crate::core::AudioRingBuffer>>,
        stop_wait: Arc<StopWait>,
    ) -> Result<()> {
        use alsa::{pcm::{Access, Format, HwParams, PCM}, Direction, ValueOr};
        
        // Output-Capture ist auch Capture, aber von einem speziellen Device
        let pcm = PCM::new(device, Direction::Capture, false)
            .with_context(|| format!("Failed to open output device: {}", device))?;
        
        let hwp = HwParams::any(&pcm)?;
        hwp.set_access(Access::RWInterleaved)?;
        
        let format_result = hwp.set_format(Format::s16())
            .or_else(|_| hwp.set_format(Format::s32()))
            .or_else(|_| hwp.set_format(Format::float()));
        
        if let Err(e) = format_result {
            log::warn!("No supported format for output capture: {}", e);
            // Fallback zu Demo-Modus
            return Self::capture_demo(
                sample_rate,
                channels,
                running,
                samples_processed,
                ring_buffer,
                stop_wait,
            );
        }
        
        hwp.set_channels(channels)?;
        hwp.set_rate(sample_rate, ValueOr::Nearest)?;
        
        let period_frames = hwp.set_period_size_near(480, ValueOr::Nearest)?;
        let _buffer_size = hwp.set_buffer_size_near(period_frames * 4)?;
        
        pcm.hw_params(&hwp)?;
        pcm.prepare()?;
        
        log::info!("Output capture started: {}Hz, {}ch", sample_rate, channels);
        
        if let Ok(io) = pcm.io_i16() {
            Self::capture_i16(
                io,
                period_frames as usize,
                channels as usize,
                sample_rate,
                running,
                samples_processed,
                ring_buffer,
                stop_wait.clone(),
            )?;
        } else {
            log::warn!("i16 capture failed, using demo mode");
            Self::capture_demo(
                sample_rate,
                channels,
                running,
                samples_processed,
                ring_buffer,
                stop_wait,
            )?;
        }
        
        Ok(())
    }
    
    fn capture_i16(
        io: alsa::pcm::IO<i16>,
        period_frames: usize,
        channels: usize,
        sample_rate: u32,
        running: Arc<AtomicBool>,
        samples_processed: Arc<AtomicU64>,
        ring_buffer: Option<Arc<crate::core::AudioRingBuffer>>,
        stop_wait: Arc<StopWait>,
    ) -> Result<()> {
        let target_frames = sample_rate as usize / 10;
        let target_samples = target_frames * channels;
        
        let period_samples = period_frames * channels;
        let mut buffer = vec![0i16; period_samples];
        let mut fifo: Vec<i16> = Vec::with_capacity(target_samples * 2);
        
        while running.load(Ordering::Relaxed) {
            match io.readi(&mut buffer) {
                Ok(frames) if frames > 0 => {
                    let samples_read = frames as usize * channels;
                    let slice = &buffer[..samples_read];
                    
                    fifo.extend_from_slice(slice);
                    samples_processed.fetch_add(samples_read as u64, Ordering::Relaxed);
                    
                    while fifo.len() >= target_samples {
                        let chunk_samples: Vec<i16> = fifo.drain(..target_samples).collect();
                        
                        if let Some(rb) = &ring_buffer {
                            let frame = crate::core::PcmFrame {
                                utc_ns: crate::core::timestamp::utc_ns_now(),
                                samples: chunk_samples.clone(),
                                sample_rate,
                                channels: channels as u8,
                            };
                            rb.push(frame);
                        }
                    }
                }
                Ok(_) => stop_wait.wait_timeout(Duration::from_millis(STOP_WAIT_IDLE_MS)),
                Err(e) => {
                    log::warn!("Output capture read error: {}", e);
                    stop_wait.wait_timeout(Duration::from_millis(STOP_WAIT_ERROR_MS));
                }
            }
        }
        
        Ok(())
    }
    
    fn capture_demo(
        sample_rate: u32,
        channels: u32,
        running: Arc<AtomicBool>,
        samples_processed: Arc<AtomicU64>,
        ring_buffer: Option<Arc<crate::core::AudioRingBuffer>>,
        stop_wait: Arc<StopWait>,
    ) -> Result<()> {
        log::warn!("Output capture demo mode - simulating system audio");
        
        let target_frames = sample_rate as usize / 10;
        let target_samples = target_frames * channels as usize;
        
        let mut tick = 0;
        while running.load(Ordering::Relaxed) {
            stop_wait.wait_timeout(Duration::from_millis(DEMO_TICK_INTERVAL_MS));
            tick += 1;
            
            if tick % DEMO_LOG_EVERY_TICKS == 0 {
                let chunk_samples = vec![0i16; target_samples];
                samples_processed.fetch_add(target_samples as u64, Ordering::Relaxed);
                
                if let Some(rb) = &ring_buffer {
                    let frame = crate::core::PcmFrame {
                        utc_ns: crate::core::timestamp::utc_ns_now(),
                        samples: chunk_samples,
                        sample_rate,
                        channels: channels as u8,
                    };
                    rb.push(frame);
                }
            }
        }
        
        Ok(())
    }
}
