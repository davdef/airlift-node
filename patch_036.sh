#!/bin/bash
# patch_036_clean.sh

echo "=== Sauberer Debug ==="

# Backup
cp src/producers/file.rs src/producers/file.rs.backup2
cp src/core/processor.rs src/core/processor.rs.backup2

# 1. FileProducer: Debug-Log für jedes geschriebene Frame
cat > /tmp/fixed_file_producer << 'EOF'
                    // In RingBuffer speichern
                    if let Some(rb) = &ring_buffer {
                        let frame = crate::core::PcmFrame {
                            utc_ns: crate::core::utc_ns_now(),
                            samples: chunk.to_vec(),
                            sample_rate: self.sample_rate,
                            channels: self.channels,
                        };
                        log::debug!("FileProducer '{}': Schreibe Frame ({} samples) in Buffer addr: {:?}", 
                            name, chunk.len(), Arc::as_ptr(rb));
                        rb.push(frame);
                    } else {
                        log::warn!("FileProducer '{}': KEIN Buffer attached!", name);
                    }
EOF

# Finde und ersetze die Zeilen in file.rs
sed -i '73,83d' src/producers/file.rs
sed -i '73r /tmp/fixed_file_producer' src/producers/file.rs

# 2. Passthrough: Debug-Log für jedes gelesene Frame
cat > /tmp/fixed_passthrough << 'EOF'
        fn process(&mut self, input_buffer: &AudioRingBuffer, output_buffer: &AudioRingBuffer) -> Result<()> {
            let mut frames = 0;
            while let Some(frame) = input_buffer.pop() {
                log::debug!("Passthrough '{}': Lese Frame {} ({} samples)", 
                    self.name, frames + 1, frame.samples.len());
                output_buffer.push(frame);
                frames += 1;
            }
            if frames > 0 {
                log::debug!("Passthrough '{}': Verarbeitete {} Frames", self.name, frames);
            }
            Ok(())
        }
EOF

# Finde und ersetze in processor.rs
sed -i '/fn process(&mut self, input_buffer: &AudioRingBuffer, output_buffer: &AudioRingBuffer)/,/^        }/c'"$(cat /tmp/fixed_passthrough)" src/core/processor.rs

# 3. Buffer-Größe erhöhen
sed -i 's/AudioRingBuffer::new(100)/AudioRingBuffer::new(1000)/g' src/core/node.rs
sed -i 's/AudioRingBuffer::new(100)/AudioRingBuffer::new(1000)/g' src/core/ringbuffer.rs 2>/dev/null || true

echo "✅ Debug-Logs hinzugefügt"
echo "🔧 Test starten..."
RUST_LOG=debug cargo run 2>&1 | grep -E "(FileProducer.*Schreibe|Passthrough.*Lese|KEIN Buffer|Buffer.*Frames|Verarbeitete)" | head -40
