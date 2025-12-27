#!/bin/bash
# fix_corrupted_files.sh

# 1. Node-Datei fixen - Zeile 46-49
echo "Fixing node.rs..."
sed -i '46,49d' src/core/node.rs
sed -i '46i\    pub fn add_consumer(&mut self, mut consumer: Box<dyn Consumer>) {\
        let consumer_name = consumer.name().to_string();\
        consumer.attach_input_buffer(self.output_buffer.clone());\
        self.consumers.push(consumer);\
        info!("Flow \"{}\": Added consumer \"{}\"", self.name, consumer_name);\
    }' src/core/node.rs

# 2. Mixer-Datei fixen - Zeile 81
echo "Fixing mixer.rs..."
sed -i '81,83d' src/processors/mixer.rs
sed -i '81i\    fn process(&mut self, _input_buffer: &AudioRingBuffer, output_buffer: &AudioRingBuffer) -> Result<()> {' src/processors/mixer.rs

# 3. Consumer-Datei: Seek Import korrigieren
echo "Fixing consumer.rs Seek import..."
sed -i '4d' src/core/consumer.rs
sed -i '3a\use std::io::{self, Seek};' src/core/consumer.rs

# Auch in der file_writer-Module anpassen
sed -i 's/use std::io::{Write, BufWriter, Seek};/use std::io::{Write, BufWriter, Seek, self};/' src/core/consumer.rs

# 4. Test build
echo "Testing compilation..."
cargo check
