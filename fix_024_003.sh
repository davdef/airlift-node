#!/bin/bash
# fix_024_final.sh

# 1. Zuerst node.rs komplett reparieren
echo "Repariere node.rs..."
# Backup erstellen
cp src/core/node.rs src/core/node.rs.backup

# Korrekte Version der add_consumer Funktion schreiben
cat > /tmp/fixed_node_part << 'EOF'
    pub fn add_consumer(&mut self, mut consumer: Box<dyn Consumer>) {
        let consumer_name = consumer.name().to_string();
        consumer.attach_input_buffer(self.output_buffer.clone());
        self.consumers.push(consumer);
        info!("Flow \"{}\": Added consumer \"{}\"", self.name, consumer_name);
    }
EOF

# Zeilen 46-55 durch die korrekte Version ersetzen
sed -i '46,55c\    pub fn add_consumer(&mut self, mut consumer: Box<dyn Consumer>) {\
        let consumer_name = consumer.name().to_string();\
        consumer.attach_input_buffer(self.output_buffer.clone());\
        self.consumers.push(consumer);\
        info!("Flow \"{}\": Added consumer \"{}\"", self.name, consumer_name);\
    }' src/core/node.rs

# 2. Auch Mixer.rs vollständig reparieren
echo "Repariere mixer.rs..."
cp src/processors/mixer.rs src/processors/mixer.rs.backup

cat > /tmp/fixed_mixer_part << 'EOF'
    fn process(&mut self, _input_buffer: &AudioRingBuffer, output_buffer: &AudioRingBuffer) -> Result<()> {
        // Mixer ignoriert den single input_buffer Parameter
        // und nutzt seine eigenen input_buffers
        
        if let Some(mixed_frame) = self.mix_available_frames() {
            output_buffer.push(mixed_frame);
            log::debug!("Mixer '{}': Pushed mixed frame to output", self.name);
        }
        
        Ok(())
    }
EOF

# Zeilen 81-94 reparieren
sed -i '81,94c\    fn process(&mut self, _input_buffer: &AudioRingBuffer, output_buffer: &AudioRingBuffer) -> Result<()> {\
        // Mixer ignoriert den single input_buffer Parameter\
        // und nutzt seine eigenen input_buffers\
        \
        if let Some(mixed_frame) = self.mix_available_frames() {\
            output_buffer.push(mixed_frame);\
            log::debug!("Mixer \"{}\": Pushed mixed frame to output", self.name);\
        }\
        \
        Ok(())\
    }' src/processors/mixer.rs

# 3. Überprüfen
echo "=== Node.rs lines 46-55 ==="
sed -n '46,55p' src/core/node.rs

echo -e "\n=== Mixer.rs lines 81-95 ==="
sed -n '81,95p' src/processors/mixer.rs

# 4. Testen
echo -e "\nTesting compilation..."
cargo check
