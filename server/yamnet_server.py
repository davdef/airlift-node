#!/usr/bin/env python3
import subprocess
import numpy as np
import tensorflow as tf
import tensorflow_hub as hub
from flask import Flask, jsonify, Response
from flask_cors import CORS
import threading
import time
import queue
import json
from collections import defaultdict

app = Flask(__name__)
CORS(app)

class YamnetAnalyzer:
    def __init__(self, stream_url):
        self.stream_url = stream_url
        self.model = hub.load('https://tfhub.dev/google/yamnet/1')
        self.ffmpeg_process = None
        self.analysis_queue = queue.Queue(maxsize=20)
        self.latest_analysis = None
        self.running = False
        
        # Klassennamen und Kategorisierung
        self.class_names = self.load_class_names()
        self.class_categories = self.categorize_all_classes()
        
        print(f"✅ YAMNet geladen, {len(self.class_names)} Klassen")
        
        # DEBUG: Test YAMNet
        print("🔧 DEBUG: Testing YAMNet with dummy audio...")
        test_audio = np.zeros(16000 * 3, dtype=np.float32)
        scores, _, _ = self.model(test_audio)
        print(f"✅ YAMNet test passed, scores shape: {scores.shape}")
        
    def load_class_names(self):
        """Lädt alle 521 YAMNet-Klassennamen"""
        class_map_path = self.model.class_map_path().numpy().decode('utf-8')
        class_names = {}
        with open(class_map_path, 'r') as f:
            for line in f:
                parts = line.strip().split(',')
                if len(parts) >= 3:
                    try:
                        index = int(parts[0])
                        display_name = parts[2]
                        class_names[index] = display_name
                    except:
                        continue
        return class_names
    
    def categorize_class(self, class_name):
        """Kategorisiert eine Klasse für die Visualisierung"""
        if not class_name:
            return 'other'
        
        lower = class_name.lower()
        
        # Musik
        if any(keyword in lower for keyword in 
               ['music', 'song', 'singing', 'sing', 'melody', 'harmony',
                'choir', 'vocal', 'opera', 'symphony', 'orchestra', 'choir']):
            return 'music'
        
        # Instrumente
        if any(keyword in lower for keyword in
               ['guitar', 'drum', 'piano', 'violin', 'cello', 'trumpet',
                'saxophone', 'flute', 'clarinet', 'harp', 'banjo', 'ukulele',
                'accordion', 'organ', 'keyboard', 'synthesizer', 'bass',
                'mandolin', 'harmonica', 'xylophone', 'marimba', 'bell']):
            return 'instrument'
        
        # Sprache
        if any(keyword in lower for keyword in
               ['speech', 'talk', 'conversation', 'narration', 'monologue',
                'dialog', 'dialogue', 'voice', 'announce', 'announcer',
                'commentary', 'comment', 'interview', 'whispering']):
            return 'speech'
        
        # Menschliche Laute
        if any(keyword in lower for keyword in
               ['laughter', 'laugh', 'crying', 'cry', 'sob', 'sigh',
                'cough', 'sneeze', 'snore', 'breathing', 'gasp', 'grunt',
                'groan', 'moan', 'whimper', 'hiccup', 'burp', 'scream',
                'shout', 'yell', 'whistle', 'hum', 'sniff']):
            return 'human'
        
        # Tiere
        if any(keyword in lower for keyword in
               ['dog', 'cat', 'bird', 'horse', 'cow', 'sheep', 'pig',
                'chicken', 'rooster', 'duck', 'goose', 'owl', 'eagle',
                'crow', 'sparrow', 'parrot', 'canary', 'lion', 'tiger',
                'elephant', 'monkey', 'wolf', 'frog', 'cricket', 'insect',
                'bee', 'mosquito', 'fly', 'cicada', 'cricket']):
            return 'animal'
        
        # Fahrzeuge
        if any(keyword in lower for keyword in
               ['car', 'vehicle', 'engine', 'motor', 'train', 'airplane',
                'aircraft', 'helicopter', 'boat', 'ship', 'siren', 'horn',
                'alarm', 'truck', 'bus', 'motorcycle', 'bicycle', 'ambulance',
                'fire engine', 'police', 'construction']):
            return 'vehicle'
        
        # Natur
        if any(keyword in lower for keyword in
               ['rain', 'wind', 'thunder', 'lightning', 'storm', 'water',
                'wave', 'stream', 'river', 'ocean', 'sea', 'fire', 'crackle',
                'earthquake', 'avalanche', 'waterfall', 'fountain']):
            return 'nature'
        
        # Elektronik
        if any(keyword in lower for keyword in
               ['telephone', 'phone', 'cell phone', 'computer', 'keyboard',
                'typewriter', 'printer', 'scanner', 'radio', 'television',
                'tv', 'microwave', 'oven', 'refrigerator', 'washer', 'dryer',
                'clock', 'watch', 'timer', 'bell', 'buzzer', 'beep']):
            return 'electronic'
        
        # Haushalt
        if any(keyword in lower for keyword in
               ['door', 'window', 'gate', 'drawer', 'cabinet', 'chair',
                'table', 'bed', 'curtain', 'blender', 'mixer', 'vacuum',
                'fan', 'air conditioner', 'heater', 'faucet', 'shower',
                'toilet', 'flush', 'zipper', 'keys', 'jingle']):
            return 'household'
        
        # Werkzeuge
        if any(keyword in lower for keyword in
               ['hammer', 'saw', 'drill', 'wrench', 'screwdriver', 'nail',
                'jackhammer']):
            return 'tool'
        
        # Sport
        if any(keyword in lower for keyword in
               ['applause', 'cheering', 'crowd', 'stadium', 'whistle',
                'referee', 'basketball', 'football', 'soccer', 'tennis',
                'baseball', 'golf', 'bowling', 'pool', 'swimming']):
            return 'sport'
        
        # Explosionen
        if any(keyword in lower for keyword in
               ['gunshot', 'gunfire', 'explosion', 'blast', 'fireworks']):
            return 'impact'
        
        return 'other'
    
    def categorize_all_classes(self):
        """Kategorisiert alle 521 Klassen einmalig"""
        categories = {}
        for idx, name in self.class_names.items():
            categories[idx] = self.categorize_class(name)
        return categories
    
    def get_category_color(self, category):
        """Gibt Farbcode für Kategorie zurück"""
        colors = {
            'music': '#5aff8c',
            'instrument': '#2ecc71',
            'speech': '#ff8c5a',
            'human': '#e74c3c',
            'animal': '#9b59b6',
            'vehicle': '#3498db',
            'nature': '#1abc9c',
            'electronic': '#00bcd4',
            'household': '#795548',
            'tool': '#f39c12',
            'sport': '#e67e22',
            'impact': '#e91e63',
            'other': '#607d8b'
        }
        return colors.get(category, '#607d8b')
    
    def start_analysis(self):
        """Startet die kontinuierliche Analyse mit DEBUG"""
        self.running = True
        
        print("🔧 DEBUG: Starting analysis thread")
        
        # Teste FFmpeg zuerst
        print("🔧 DEBUG: Testing FFmpeg...")
        test_cmd = ['ffmpeg', '-version']
        try:
            result = subprocess.run(test_cmd, capture_output=True, text=True)
            print(f"✅ FFmpeg verfügbar: {result.stdout.split()[2]}")
        except Exception as e:
            print(f"❌ FFmpeg nicht verfügbar: {e}")
            self.running = False
            return
        
        # Teste Stream-Zugriff
        print(f"🔧 DEBUG: Testing stream access: {self.stream_url}")
        test_stream_cmd = ['curl', '-I', '-s', self.stream_url]
        try:
            result = subprocess.run(test_stream_cmd, capture_output=True, text=True, timeout=5)
            if '200 OK' in result.stdout:
                print("✅ Stream ist erreichbar")
            else:
                print(f"⚠️  Stream nicht erreichbar: {result.stdout[:100]}")
        except Exception as e:
            print(f"⚠️  Stream test failed: {e}")
        
        # FFmpeg für OGG → PCM 16kHz
        ffmpeg_cmd = [
            'ffmpeg',
            '-i', self.stream_url,
            '-f', 's16le',
            '-acodec', 'pcm_s16le',
            '-ac', '1',
            '-ar', '16000',
            '-loglevel', 'error',  # Wichtig: 'error' statt 'quiet'
            'pipe:1'
        ]
        
        print(f"🎯 Starte Analyse von: {self.stream_url}")
        print(f"🔧 DEBUG: FFmpeg command: {' '.join(ffmpeg_cmd)}")
        
        try:
            self.ffmpeg_process = subprocess.Popen(
                ffmpeg_cmd,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,  # Capture stderr for debugging
                bufsize=10**6
            )
            
            print("✅ FFmpeg process started")
            
            # Warte kurz bis Prozess startet
            time.sleep(0.5)
            
            # Prüfe ob Prozess noch läuft
            if self.ffmpeg_process.poll() is not None:
                stderr = self.ffmpeg_process.stderr.read().decode('utf-8', errors='ignore')
                print(f"❌ FFmpeg process died immediately. Stderr: {stderr[:200]}")
                self.running = False
                return
            
        except Exception as e:
            print(f"❌ FFmpeg start failed: {e}")
            self.running = False
            return
        
        # CHUNK-GRÖSSE für stabilere Updates
        chunk_duration = 1.0  # 1.0 Sekunden
        chunk_size = int(16000 * chunk_duration)
        
        analysis_count = 0
        last_sent_time = 0
        consecutive_empty_reads = 0
        
        print("🔧 DEBUG: Entering main analysis loop...")
        
        try:
            while self.running:
                # Prüfe ob FFmpeg Prozess noch läuft
                if self.ffmpeg_process.poll() is not None:
                    stderr = self.ffmpeg_process.stderr.read().decode('utf-8', errors='ignore')
                    print(f"⚠️  FFmpeg process died. Stderr: {stderr[:500]}")
                    break
                
                # Audio-Daten lesen
                raw_bytes = self.ffmpeg_process.stdout.read(chunk_size * 2)
                
                if not raw_bytes:
                    consecutive_empty_reads += 1
                    if consecutive_empty_reads > 5:
                        print(f"⚠️  {consecutive_empty_reads} consecutive empty reads")
                        # Versuche stderr zu lesen
                        try:
                            stderr_line = self.ffmpeg_process.stderr.readline()
                            if stderr_line:
                                print(f"🔧 DEBUG: FFmpeg stderr: {stderr_line.decode('utf-8', errors='ignore')}")
                        except:
                            pass
                    
                    if consecutive_empty_reads > 20:
                        print("❌ Too many empty reads, restarting...")
                        break
                    
                    time.sleep(0.01)
                    continue
                
                consecutive_empty_reads = 0
                
                if len(raw_bytes) < chunk_size * 2 * 0.3:
                    print(f"⚠️  Not enough data: {len(raw_bytes)} bytes (expected: {chunk_size * 2})")
                    continue
                
                # Konvertieren
                try:
                    audio_int16 = np.frombuffer(raw_bytes, dtype=np.int16)
                    audio_float32 = audio_int16.astype(np.float32) / 32768.0
                    
                    # Debug: Erste Analyse
                    if analysis_count == 0:
                        print(f"🔧 DEBUG: First audio chunk - shape: {audio_float32.shape}, "
                              f"min: {audio_float32.min():.3f}, max: {audio_float32.max():.3f}, "
                              f"mean: {audio_float32.mean():.3f}")
                    
                except Exception as e:
                    print(f"❌ Audio conversion failed: {e}")
                    continue
                
                # YAMNet-Analyse
                analysis = self.analyze_audio(audio_float32)
                self.latest_analysis = analysis
                
                # Debug erste Analyse
                if analysis_count == 0 and analysis['topClasses']:
                    print(f"🎯 First analysis successful: {len(analysis['topClasses'])} tags")
                
                # Nur senden wenn genug Zeit vergangen (max 10Hz = 100ms)
                current_time = time.time()
                if current_time - last_sent_time >= 0.1:  # 100ms Mindestabstand
                    try:
                        self.analysis_queue.put_nowait(analysis)
                        last_sent_time = current_time
                        
                        if analysis_count % 20 == 0:  # Alle 20 Analysen loggen
                            if analysis['topClasses']:
                                top_tag = analysis['topClasses'][0]
                                print(f"📊 Analysis #{analysis_count}: {top_tag['name']} ({top_tag['confidence']:.1%}), "
                                      f"Total: {analysis['totalConfidence']:.2f}, Tags: {len(analysis['topClasses'])}")
                            
                    except queue.Full:
                        # Queue voll -> ältesten entfernen
                        try:
                            self.analysis_queue.get_nowait()
                            self.analysis_queue.put_nowait(analysis)
                            last_sent_time = current_time
                        except:
                            pass
                
                analysis_count += 1
                
        except Exception as e:
            print(f"❌ Analysefehler: {type(e).__name__}: {e}")
            import traceback
            traceback.print_exc()
            
        finally:
            print(f"🔄 Analysis loop ended after {analysis_count} analyses")
            self.stop()
            
            # Automatischer Neustart nach 3 Sekunden
            if self.running:  # Falls nicht manuell gestoppt
                print("🔄 Attempting restart in 3 seconds...")
                time.sleep(3)
                self.start_analysis()
    
    def analyze_audio(self, audio_data):
        """Führt YAMNet-Analyse durch und gibt relevante Klassen zurück"""
        try:
            scores, _, _ = self.model(audio_data)
        except Exception as e:
            print(f"❌ YAMNet inference failed: {e}")
            return {
                'timestamp': time.time(),
                'topClasses': [],
                'dominantCategory': 'error',
                'totalConfidence': 0,
                'totalClasses': 0,
                'analysisId': int(time.time() * 1000)
            }
        
        # Durchschnittliche Scores über Zeitfenster
        avg_scores = np.mean(scores, axis=0)
        
        # Top 20 Klassen finden
        top_indices = np.argsort(avg_scores)[-20:][::-1]
        
        top_classes = []
        total_confidence = 0
        
        for idx in top_indices:
            class_name = self.class_names.get(idx, f"Class_{idx}")
            confidence = float(avg_scores[idx])
            
            # NIEDRIGERE SCHWELLE für mehr Klassen
            if confidence < 0.005:  # 0.5% statt 1%
                continue
                
            category = self.class_categories.get(idx, 'other')
            
            top_classes.append({
                'id': int(idx),
                'name': class_name,
                'confidence': confidence,  # ORIGINAL CONFIDENCE
                'category': category,
                'color': self.get_category_color(category)
            })
            
            total_confidence += confidence
        
        # Nach Confidence sortieren
        top_classes.sort(key=lambda x: x['confidence'], reverse=True)
        
        # Auf 8-15 Klassen limitieren
        if len(top_classes) > 15:
            top_classes = top_classes[:15]
        
        # Dominante Kategorie bestimmen
        category_scores = defaultdict(float)
        for cls in top_classes:
            category_scores[cls['category']] += cls['confidence']
        
        dominant_category = max(category_scores.items(), key=lambda x: x[1], default=('other', 0))[0]
        
        return {
            'timestamp': time.time(),
            'topClasses': top_classes,
            'dominantCategory': dominant_category,
            'totalConfidence': total_confidence,
            'totalClasses': len(top_classes),
            'analysisId': int(time.time() * 1000)
        }
    
    def stop(self):
        """Stoppt die Analyse"""
        self.running = False
        if self.ffmpeg_process:
            try:
                self.ffmpeg_process.terminate()
                self.ffmpeg_process.wait(timeout=2)
                print("✅ FFmpeg process stopped")
            except:
                try:
                    self.ffmpeg_process.kill()
                except:
                    pass
        print("⏹️  Analyse gestoppt")

# Globale Instanz
STREAM_URL = "https://icecast.radiorfm.de/rfm.ogg"
analyzer = YamnetAnalyzer(STREAM_URL)

# Starte Analyse-Thread
analysis_thread = threading.Thread(target=analyzer.start_analysis, daemon=True)

@app.route('/api/yamnet/analysis')
def get_analysis():
    """Gibt die neueste Analyse zurück"""
    if analyzer.latest_analysis:
        return jsonify(analyzer.latest_analysis)
    else:
        return jsonify({
            'status': 'starting',
            'message': 'Analyse wird gestartet...',
            'timestamp': time.time(),
            'queueSize': analyzer.analysis_queue.qsize(),
            'analyzerRunning': analyzer.running
        })

@app.route('/api/yamnet/stream')
def stream_analysis():
    """Server-Sent Events Stream für Echtzeit-Updates"""
    def generate():
        delay_seconds = 3.0
        last_keepalive = time.time()
        last_analysis_id = None
        empty_count = 0
        
        print(f"🔧 DEBUG: SSE stream started, queue size: {analyzer.analysis_queue.qsize()}")
        
        while True:
            try:
                # Versuche neue Analyse zu bekommen
                analysis = analyzer.analysis_queue.get(timeout=1.0)
                
                # Nur senden wenn sich was geändert hat
                if analysis.get('analysisId') != last_analysis_id:
                    wait_time = (analysis.get('timestamp', time.time()) + delay_seconds) - time.time()
                    if wait_time > 0:
                        time.sleep(wait_time)
                    yield f"data: {json.dumps(analysis)}\n\n"
                    last_analysis_id = analysis.get('analysisId')
                    last_keepalive = time.time()
                    empty_count = 0
                    
                    # Debug: Erste Nachricht
                    if empty_count == 0:
                        print(f"✅ SSE: First data sent, {len(analysis.get('topClasses', []))} tags")
                else:
                    empty_count += 1
                
            except queue.Empty:
                empty_count += 1
                
                # Alle 3 Sekunden Keep-Alive senden
                if time.time() - last_keepalive > 3:
                    yield f"data: {{\"keepalive\": true, \"timestamp\": {time.time()}, \"queueSize\": {analyzer.analysis_queue.qsize()}}}\n\n"
                    last_keepalive = time.time()
                
                # Bei zu vielen leeren Queues debug info
                if empty_count > 10 and empty_count % 10 == 0:
                    print(f"🔧 DEBUG: SSE queue empty for {empty_count} cycles, analyzer running: {analyzer.running}")
    
    return Response(
        generate(),
        mimetype='text/event-stream',
        headers={
            'Cache-Control': 'no-cache',
            'Content-Type': 'text/event-stream',
            'X-Accel-Buffering': 'no',
            'Access-Control-Allow-Origin': '*'
        }
    )

@app.route('/api/yamnet/status')
def get_status():
    """Gibt Server-Status zurück"""
    return jsonify({
        'status': 'running' if analyzer.running else 'stopped',
        'streamUrl': STREAM_URL,
        'analyzing': analyzer.running,
        'queueSize': analyzer.analysis_queue.qsize(),
        'lastUpdate': analyzer.latest_analysis['timestamp'] if analyzer.latest_analysis else None,
        'totalClasses': len(analyzer.class_names),
        'debug': {
            'ffmpegAlive': analyzer.ffmpeg_process is not None and analyzer.ffmpeg_process.poll() is None,
            'analysisThreadAlive': analysis_thread.is_alive()
        }
    })

@app.route('/api/yamnet/classes')
def get_all_classes():
    """Gibt alle 521 Klassen mit Kategorien zurück"""
    classes_by_category = defaultdict(list)
    
    for idx, name in analyzer.class_names.items():
        category = analyzer.class_categories.get(idx, 'other')
        classes_by_category[category].append({
            'id': int(idx),
            'name': name,
            'color': analyzer.get_category_color(category)
        })
    
    # Sortieren
    for category in classes_by_category:
        classes_by_category[category].sort(key=lambda x: x['name'])
    
    return jsonify({
        'total': len(analyzer.class_names),
        'categories': {
            category: {
                'count': len(classes),
                'classes': classes[:30]  # Max 30 pro Kategorie anzeigen
            }
            for category, classes in classes_by_category.items()
        }
    })

@app.route('/api/yamnet/debug')
def get_debug():
    """Debug-Informationen"""
    return jsonify({
        'analyzer': {
            'running': analyzer.running,
            'queue_size': analyzer.analysis_queue.qsize(),
            'has_latest': analyzer.latest_analysis is not None,
            'ffmpeg_alive': analyzer.ffmpeg_process is not None and analyzer.ffmpeg_process.poll() is None
        },
        'thread': {
            'alive': analysis_thread.is_alive(),
            'name': analysis_thread.name,
            'daemon': analysis_thread.daemon
        },
        'timestamp': time.time()
    })

@app.route('/api/yamnet/health')
def health_check():
    """Einfacher Health Check"""
    return jsonify({
        'status': 'healthy' if analyzer.running else 'stopped',
        'timestamp': time.time(),
        'analyzerRunning': analyzer.running,
        'queueSize': analyzer.analysis_queue.qsize(),
        'lastAnalysis': analyzer.latest_analysis is not None,
        'ffmpegAlive': analyzer.ffmpeg_process is not None and analyzer.ffmpeg_process.poll() is None
    })

if __name__ == '__main__':
    print("🚀 RFM YAMNet API Server (DEBUG VERSION)")
    print("=" * 60)
    
    # Kategorien-Statistik anzeigen
    category_stats = defaultdict(int)
    for cat in analyzer.class_categories.values():
        category_stats[cat] += 1
    
    print("📊 Klassenverteilung nach Kategorien (Top 10):")
    for cat, count in sorted(category_stats.items(), key=lambda x: x[1], reverse=True)[:10]:
        print(f"  {cat:15} {count:3d} Klassen")
    
    print(f"\n🎯 Stream: {STREAM_URL}")
    print("📈 Update-Rate: ~10Hz (100ms Interval)")
    print("🎯 Chunk-Größe: 1.0 Sekunden")
    print("⏱️  SSE Verzögerung: 3.0 Sekunden")
    print("🎯 Confidence-Schwelle: 0.5%")
    
    # Starte Analyse-Thread
    analysis_thread.start()
    print("▶️  Analyse-Thread gestartet")
    
    # Kurz warten und Status prüfen
    time.sleep(2)
    
    print(f"\n🔧 Initial status:")
    print(f"  - Thread alive: {analysis_thread.is_alive()}")
    print(f"  - Analyzer running: {analyzer.running}")
    print(f"  - Queue size: {analyzer.analysis_queue.qsize()}")
    
    # API Endpoints anzeigen
    print("\n🌐 API Endpoints:")
    print("  GET /api/yamnet/analysis     - Aktuelle Analyse")
    print("  GET /api/yamnet/stream       - Echtzeit-Stream (SSE)")
    print("  GET /api/yamnet/status       - Server-Status")
    print("  GET /api/yamnet/classes      - Alle 521 Klassen")
    print("  GET /api/yamnet/debug        - Debug-Informationen")
    print("  GET /api/yamnet/health       - Health Check")
    
    print(f"\n📡 Server läuft auf: http://localhost:5000")
    print("   Nginx Proxy: /api/yamnet/* → http://localhost:5000/api/yamnet/*")
    print("\n⏳ Warte auf erste Audio-Analyse...")
    print("Drücke Ctrl+C zum Beenden\n")
    
    try:
        app.run(host='0.0.0.0', port=5000, debug=False, threaded=True, use_reloader=False)
    except KeyboardInterrupt:
        print("\n\n👋 Server wird beendet...")
        analyzer.stop()
        if analysis_thread.is_alive():
            analysis_thread.join(timeout=2)
