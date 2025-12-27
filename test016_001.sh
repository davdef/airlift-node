# Teste die existierenden CLI-Funktionen
echo "=== Testing CLI Functions ==="

echo "1. Device Discovery:"
cargo run -- --discover 2>&1 | head -30

echo -e "\n2. Normal Mode (with demo):"
timeout 3 cargo run 2>&1 | tail -20

echo -e "\n3. Create test config for device testing:"
cat > test_device.toml << 'EOF'
node_name = "test"

[producers.test_mic]
type = "alsa_input"
enabled = true
device = "default"
channels = 1
sample_rate = 44100
EOF

echo -e "\n4. Device Test (if default device exists):"
if cargo run -- --discover 2>&1 | grep -q "default"; then
    cargo run -- --test-device default 2>&1 | head -30
else
    echo "No 'default' device found. Available devices:"
    cargo run -- --discover 2>&1 | grep "id:" | head -5
fi

echo -e "\n=== Current Architecture Status ==="
echo "✅ Producers: ALSA, File, Output-Capture"
echo "✅ Processors: PassThrough, Gain (not connected yet)"
echo "✅ Flows: Config-Struktur vorhanden"
echo "❌ Processors noch nicht mit Flows verbunden"
echo "❌ Mixer fehlt"
echo "❌ Consumer fehlen"

echo -e "\n=== Next Steps Options ==="
echo "1. Mixer-Processor implementieren (mehrere Inputs → ein Output)"
echo "2. Processors mit Flows verbinden (Pipeline starten)"
echo "3. Consumer für Netzwerk/File implementieren"
echo "4. Decoder/Encoder-Traits hinzufügen"
echo "5. REST-API für Runtime-Konfiguration"

echo -e "\nWas möchtest du als nächstes?"
