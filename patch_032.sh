cat > patch_030.sh << 'EOF'
#!/bin/bash
# patch_030.sh - Debug Buffer-Verbindung

echo "=== Debug Buffer-Verbindung ==="

# 1. Füge Debug-Log zu connect_producer_to_flow hinzu
cat > /tmp/debug_connect << 'EOF2'
    pub fn connect_producer_to_flow(&mut self, producer_index: usize, flow_index: usize) -> Result<()> {
        if producer_index < self.producer_buffers.len() && flow_index < self.flows.len() {
            let buffer = self.producer_buffers[producer_index].clone();
            log::debug!("connect_producer_to_flow: Producer buffer addr: {:?}, Flow bekommt clone", 
                Arc::as_ptr(&self.producer_buffers[producer_index]));
            self.flows[flow_index].add_input_buffer(buffer);
            info!("Connected producer {} to flow {}", producer_index, flow_index);
            Ok(())
        } else {
            anyhow::bail!("Invalid producer or flow index");
        }
    }
EOF2

# Ersetze die Funktion in node.rs
sed -i '/pub fn connect_producer_to_flow/,/^    }/c'"$(cat /tmp/debug_connect)" src/core/node.rs

# 2. Füge Debug-Log zu add_input_buffer hinzu
cat > /tmp/debug_add_input << 'EOF2'
    pub fn add_input_buffer(&mut self, buffer: Arc<AudioRingBuffer>) {
        log::debug!("Flow '{}': add_input_buffer mit Buffer addr: {:?}", 
            self.name, Arc::as_ptr(&buffer));
        self.input_buffers.push(buffer);
    }
EOF2

sed -i '/pub fn add_input_buffer/,/^    }/c'"$(cat /tmp/debug_add_input)" src/core/node.rs

# 3. Füge einfachen Debug zur processing_loop hinzu
cat > /tmp/debug_loop_simple << 'EOF2'
    fn processing_loop(
        running: Arc<AtomicBool>,
        input_buffers: Vec<Arc<AudioRingBuffer>>,
        processor_buffers: Vec<Arc<AudioRingBuffer>>,
        output_buffer: Arc<AudioRingBuffer>,
        mut processors: Vec<Box<dyn Processor>>,
        flow_name: &str,
    ) {
        info!("Flow '{}' processing thread started", flow_name);
        
        let mut iteration = 0;
        while running.load(Ordering::Relaxed) {
            iteration += 1;
            
            if input_buffers.is_empty() {
                std::thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }
            
            // Einfacher Debug: Zeige Buffer-Status alle 20 Iterationen
            if iteration % 20 == 0 {
                let total_frames: usize = input_buffers.iter().map(|b| b.len()).sum();
                log::debug!("Flow '{}': Input buffers haben {} Frames", flow_name, total_frames);
                
                for (i, buf) in input_buffers.iter().enumerate() {
                    if buf.len() > 0 {
                        log::debug!("  Buffer {}: {} Frames (addr: {:?})", i, buf.len(), Arc::as_ptr(buf));
                    }
                }
            }
            
            // Einfache Pipeline-Verarbeitung
            for (i, processor) in processors.iter_mut().enumerate() {
                let input = if i == 0 {
                    &input_buffers[0]
                } else {
                    &processor_buffers[i - 1]
                };
                
                let output = if i < processor_buffers.len() {
                    &processor_buffers[i]
                } else {
                    &output_buffer
                };
                
                if let Err(e) = processor.process(input, output) {
                    log::error!("Processor '{}' error: {}", processor.name(), e);
                }
            }
            
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        
        info!("Flow '{}' processing thread stopped", flow_name);
    }
EOF2

sed -i '/fn processing_loop/,/^    }/c'"$(cat /tmp/debug_loop_simple)" src/core/node.rs

echo "✅ Patch 030 angewandt: Buffer-Verbindungs-Debug"
echo "🔧 Starte mit: RUST_LOG=debug cargo run"
EOF

chmod +x patch_030.sh
./patch_030.sh

# Teste
RUST_LOG=debug cargo run 2>&1 | grep -E "(connect_producer_to_flow|add_input_buffer|Flow.*Frames|Buffer.*addr)" | head -20
