use std::sync::Arc;
use anyhow::Result;
use crate::core::ringbuffer::AudioRingBuffer;
use std::io::{self, Seek};

pub trait Consumer: Send + Sync {
    fn name(&self) -> &str;
    fn start(&mut self) -> Result<()>;
    fn stop(&mut self) -> Result<()>;
    fn status(&self) -> ConsumerStatus;
    fn attach_input_buffer(&mut self, buffer: Arc<AudioRingBuffer>);
}

#[derive(Debug, Clone)]
pub struct ConsumerStatus {
    pub running: bool,
    pub connected: bool,
    pub frames_processed: u64,
    pub bytes_written: u64,
    pub errors: u64,
}

pub mod file_writer {
    use super::*;
    use std::fs::File;
    use std::io::{Write, BufWriter, Seek, self};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    
    pub struct FileConsumer {
        name: String,
        running: Arc<AtomicBool>,
        input_buffer: Option<Arc<AudioRingBuffer>>,
        output_path: String,
        thread_handle: Option<std::thread::JoinHandle<()>>,
        frames_processed: Arc<AtomicU64>,
        bytes_written: Arc<AtomicU64>,
    }
    
    impl FileConsumer {
        pub fn new(name: &str, output_path: &str) -> Self {
            Self {
                name: name.to_string(),
                running: Arc::new(AtomicBool::new(false)),
                input_buffer: None,
                output_path: output_path.to_string(),
                thread_handle: None,
                frames_processed: Arc::new(AtomicU64::new(0)),
                bytes_written: Arc::new(AtomicU64::new(0)),
            }
        }
        
        fn write_wav_header(writer: &mut BufWriter<File>, sample_rate: u32, channels: u16, bits_per_sample: u16) -> Result<()> {
            writer.write_all(b"RIFF")?;
            writer.write_all(&0u32.to_le_bytes())?;
            writer.write_all(b"WAVE")?;
            
            writer.write_all(b"fmt ")?;
            writer.write_all(&16u32.to_le_bytes())?;
            writer.write_all(&1u16.to_le_bytes())?;
            writer.write_all(&channels.to_le_bytes())?;
            writer.write_all(&sample_rate.to_le_bytes())?;
            
            let byte_rate = sample_rate as u32 * channels as u32 * bits_per_sample as u32 / 8;
            writer.write_all(&byte_rate.to_le_bytes())?;
            
            let block_align = channels as u16 * bits_per_sample as u16 / 8;
            writer.write_all(&block_align.to_le_bytes())?;
            writer.write_all(&bits_per_sample.to_le_bytes())?;
            
            writer.write_all(b"data")?;
            writer.write_all(&0u32.to_le_bytes())?;
            
            Ok(())
        }
        
        fn update_wav_header(file: &mut File, data_size: u32) -> Result<()> {
            let file_size = data_size + 36;
            file.seek(std::io::SeekFrom::Start(4))?;
            file.write_all(&file_size.to_le_bytes())?;
            
            file.seek(std::io::SeekFrom::Start(40))?;
            file.write_all(&data_size.to_le_bytes())?;
            
            Ok(())
        }
    }
    
    impl Consumer for FileConsumer {
        fn name(&self) -> &str {
            &self.name
        }
        
        fn start(&mut self) -> Result<()> {
            if self.running.load(Ordering::Relaxed) {
                return Ok(());
            }
            
            log::info!("FileConsumer '{}' starting to write to {}", self.name, self.output_path);
            self.running.store(true, Ordering::SeqCst);
            
            let running = self.running.clone();
            let input_buffer = self.input_buffer.clone();
            let output_path = self.output_path.clone();
            let frames_processed = self.frames_processed.clone();
            let bytes_written = self.bytes_written.clone();
            
            let handle = std::thread::spawn(move || {
                match File::create(&output_path) {
                    Ok(file) => {
                        let mut writer = BufWriter::new(file);
                        
                        if let Err(e) = Self::write_wav_header(&mut writer, 48000, 2, 16) {
                            log::error!("Failed to write WAV header: {}", e);
                            return;
                        }
                        
                        let mut total_samples: u32 = 0;
                        
                        while running.load(Ordering::Relaxed) {
                            if let Some(buffer) = &input_buffer {
                                if let Some(frame) = buffer.pop() {
                                    for sample in &frame.samples {
                                        if let Err(e) = writer.write_all(&sample.to_le_bytes()) {
                                            log::error!("Write error: {}", e);
                                            break;
                                        }
                                        bytes_written.fetch_add(2, Ordering::Relaxed);
                                    }
                                    
                                    total_samples += frame.samples.len() as u32;
                                    frames_processed.fetch_add(1, Ordering::Relaxed);
                                    
                                    if frames_processed.load(Ordering::Relaxed) % 10 == 0 {
                                        if let Err(e) = writer.flush() {
                                            log::error!("Flush error: {}", e);
                                        }
                                    }
                                } else {
                                    std::thread::sleep(std::time::Duration::from_millis(10));
                                }
                            } else {
                                std::thread::sleep(std::time::Duration::from_millis(100));
                            }
                        }
                        
                        if let Ok(mut file) = writer.into_inner() {
                            let data_size = total_samples * 2;
                            if let Err(e) = Self::update_wav_header(&mut file, data_size) {
                                log::error!("Failed to update WAV header: {}", e);
                            }
                            if let Err(e) = file.sync_all() {
                                log::error!("Failed to sync file: {}", e);
                            }
                        }
                        
                        log::info!("FileConsumer stopped. Wrote {} frames to {}", 
                            frames_processed.load(Ordering::Relaxed), output_path);
                    }
                    Err(e) => {
                        log::error!("Failed to create file {}: {}", output_path, e);
                    }
                }
            });
            
            self.thread_handle = Some(handle);
            Ok(())
        }
        
        fn stop(&mut self) -> Result<()> {
            log::info!("FileConsumer '{}' stopping...", self.name);
            self.running.store(false, Ordering::SeqCst);
            
            if let Some(handle) = self.thread_handle.take() {
                if let Err(e) = handle.join() {
                    log::error!("Failed to join consumer thread: {:?}", e);
                }
            }
            
            Ok(())
        }
        
        fn status(&self) -> ConsumerStatus {
            ConsumerStatus {
                running: self.running.load(Ordering::Relaxed),
                connected: self.input_buffer.is_some(),
                frames_processed: self.frames_processed.load(Ordering::Relaxed),
                bytes_written: self.bytes_written.load(Ordering::Relaxed),
                errors: 0,
            }
        }
        
        fn attach_input_buffer(&mut self, buffer: Arc<AudioRingBuffer>) {
            self.input_buffer = Some(buffer);
            log::info!("FileConsumer '{}' attached to buffer", self.name);
        }
    }
}
