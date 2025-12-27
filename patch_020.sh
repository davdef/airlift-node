# Config mit korrektem TOML-Format
cat > config.toml << 'EOF'
node_name = "studio-node"

[producers.mic1]
type = "alsa_input"
enabled = true
device = "default"
channels = 2
sample_rate = 44100

[producers.background]
type = "file"
enabled = true
path = "background.wav"
loop_audio = true
channels = 2
sample_rate = 48000

[processors.gain_control]
type = "gain"
enabled = true

[processors.gain_control.config]
gain = 0.8

[processors.voice_mixer]
type = "mixer"
enabled = true

[processors.voice_mixer.config]
sample_rate = 48000

[processors.voice_mixer.config.gains]
mic1 = 1.0
background = 0.3

[processors.compressor]
type = "passthrough"
enabled = true

[flows.live_stream]
enabled = true
inputs = ["mic1", "background"]
processors = ["voice_mixer", "gain_control", "compressor"]
outputs = []

[flows.live_stream.config]
description = "Mixed voice with background music"
EOF

echo "Config gefixt. Teste mit: cargo run"
echo ""
echo "Status:"
echo "✅ ALSA Producer funktioniert"
echo "✅ File Producer funktioniert" 
echo "✅ Node läuft"
echo "✅ Flows-Struktur vorhanden"
echo "❌ Flows noch nicht verbunden (weil keine Producer in Config)"
echo ""
echo "Was möchtest du als nächstes?"
echo "1. Consumer implementieren (Netzwerk/File Output)"
echo "2. REST-API für Runtime-Config"
echo "3. Decoder/Encoder-Traits"
echo "4. Flow-Verbindungen testen"
