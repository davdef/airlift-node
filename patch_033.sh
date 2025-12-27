#!/bin/bash
# patch_033.sh - Debug FileProducer Buffer

echo "=== Debug FileProducer Buffer ==="

# Backup
cp src/producers/file.rs src/producers/file.rs.backup

# 1. Füge Debug-Log zu attach_ring_buffer hinzu
sed -i '/pub fn attach_ring_buffer/,/^    }/c\
    pub fn attach_ring_buffer(&mut self, buffer: Arc<AudioRingBuffer>) {\
        log::debug!("FileProducer '\''{}'\'': attach_ring_buffer mit addr: {:?}", \
            self.name, Arc::as_ptr(&buffer));\
        self.output_buffer = Some(buffer);\
    }' src/producers/file.rs

# 2. Füge Debug-Log zum Schreiben hinzu
sed -i '/let frame = PcmFrame {/,/buffer.push(frame);/c\
                        let frame = PcmFrame {\
                            utc_ns: Self::utc_ns_now(),\
                            samples: chunk.to_vec(),\
                            sample_rate: self.sample_rate,\
                            channels: self.channels,\
                        };\
                        if let Some(ref buffer) = self.output_buffer {\
                            log::debug!("FileProducer '\''{}'\'': Schreibe Frame in Buffer addr: {:?}", \
                                self.name, Arc::as_ptr(buffer));\
                            buffer.push(frame);\
                        }' src/producers/file.rs

echo "✅ FileProducer Debug-Logs hinzugefügt"
echo "🔧 Teste mit: RUST_LOG=debug cargo run 2>&1 | grep -E '(FileProducer.*Schreibe|FileProducer.*attach)'"
