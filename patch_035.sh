#!/bin/bash
# patch_035.sh - Debug Buffer Inhalt

echo "=== Debug Buffer Inhalt ==="

# 1. Größere Buffer für Test
sed -i 's/AudioRingBuffer::new(100)/AudioRingBuffer::new(1000)/g' src/core/node.rs

# 2. Debug-Log im FileProducer für jeden geschriebenen Frame
sed -i '/if let Some(rb) = \&ring_buffer {/,/}/c\
                    if let Some(rb) = \&ring_buffer {\
                        let frame = crate::core::PcmFrame {\
                            utc_ns: crate::core::utc_ns_now(),\
                            samples: chunk.to_vec(),\
                            sample_rate: self.sample_rate,\
                            channels: self.channels,\
                        };\
                        log::debug!("FileProducer '\''{}'\'': Schreibe Frame ({} samples) in Buffer", \
                            name, chunk.len());\
                        rb.push(frame);\
                    } else {\
                        log::warn!("FileProducer '\''{}'\'': KEIN Buffer attached!", name);\
                    }' src/producers/file.rs

# 3. Debug-Log im Passthrough für jeden gelesenen Frame
sed -i '/while let Some(frame) = input_buffer.pop() {/,/}/c\
            while let Some(frame) = input_buffer.pop() {\
                log::debug!("Passthrough '\''{}'\'': Lese Frame ({} samples)", self.name, frame.samples.len());\
                output_buffer.push(frame);\
            }' src/core/processor.rs

echo "✅ Buffer-Größe erhöht, Debug-Logs hinzugefügt"
echo "🔧 Teste mit: RUST_LOG=debug cargo run 2>&1 | grep -E '(FileProducer.*Schreibe|Passthrough.*Lese|KEIN Buffer)'"
