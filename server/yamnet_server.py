#!/usr/bin/env python3
import subprocess
import numpy as np
import tensorflow as tf
import tensorflow_hub as hub
from flask import Flask, jsonify, Response, request
from flask_cors import CORS
import threading
import time
import queue
import json
import logging
import signal
import sys
from collections import defaultdict, deque
from datetime import datetime
import psutil  # pip install psutil

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
        self.stream_delay = 10.5  # Sekunden für Buffer-Aufbau
        
        # Klassennamen
        self.class_names = self.load_class_names()
        self.class_categories = self.categorize_all_classes()
        
        print(f"✅ YAMNet geladen, {len(self.class_names)} Klassen")
        
    def load_class_names(self):
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
        if not class_name: return 'other'
        lower = class_name.lower()
        
        if any(k in lower for k in ['music', 'song', 'singing', 'choir', 'vocal']): return 'music'
        if any(k in lower for k in ['guitar', 'drum', 'piano', 'violin', 'trumpet']): return 'instrument'
        if any(k in lower for k in ['speech', 'talk', 'conversation', 'narration']): return 'speech'
        if any(k in lower for k in ['laughter', 'cough', 'sneeze', 'breathing']): return 'human'
        if any(k in lower for k in ['dog', 'cat', 'bird', 'horse', 'owl']): return 'animal'
        if any(k in lower for k in ['car', 'engine', 'train', 'airplane', 'siren']): return 'vehicle'
        if any(k in lower for k in ['rain', 'wind', 'thunder', 'water', 'fire']): return 'nature'
        if any(k in lower for k in ['telephone', 'computer', 'radio', 'television']): return 'electronic'
        if any(k in lower for k in ['door', 'window', 'chair', 'table', 'curtain']): return 'household'
        if any(k in lower for k in ['hammer', 'saw', 'drill', 'wrench']): return 'tool'
        if any(k in lower for k in ['applause', 'cheering', 'crowd', 'stadium']): return 'sport'
        if any(k in lower for k in ['gunshot', 'explosion', 'blast', 'fireworks']): return 'impact'
        return 'other'
    
    def categorize_all_classes(self):
        return {idx: self.categorize_class(name) for idx, name in self.class_names.items()}
    
    def get_category_color(self, category):
        colors = {
            'music': '#5aff8c', 'instrument': '#2ecc71', 'speech': '#ff8c5a',
            'human': '#e74c3c', 'animal': '#9b59b6', 'vehicle': '#3498db',
            'nature': '#1abc9c', 'electronic': '#00bcd4', 'household': '#795548',
            'tool': '#f39c12', 'sport': '#e67e22', 'impact': '#e91e63',
            'other': '#607d8b'
        }
        return colors.get(category, '#607d8b')
    
    def start_analysis(self):
        self.running = True
        
        while self.running:
            try:
                # FFmpeg starten
                ffmpeg_cmd = [
                    'ffmpeg', '-i', self.stream_url,
                    '-f', 's16le', '-acodec', 'pcm_s16le',
                    '-ac', '1', '-ar', '16000',
                    '-reconnect', '1', '-reconnect_streamed', '1',
                    '-reconnect_delay_max', '5', '-loglevel', 'error',
                    'pipe:1'
                ]
                
                print(f"🎯 Verbinde zu: {self.stream_url}")
                self.ffmpeg_process = subprocess.Popen(
                    ffmpeg_cmd,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    bufsize=10**6,
                    start_new_session=True
                )
                
                time.sleep(3)
                
                if self.ffmpeg_process.poll() is not None:
                    print("❌ FFmpeg start fehlgeschlagen")
                    time.sleep(10)
                    continue
                
                # WICHTIG: Audio-Buffer für Delay aufbauen
                print(f"⏳ Baue {self.stream_delay}s Audio-Buffer auf...")
                buffer_needed = int(16000 * self.stream_delay) * 2  # Bytes
                buffer_data = bytearray()
                
                while len(buffer_data) < buffer_needed and self.running:
                    chunk = self.ffmpeg_process.stdout.read(4096)
                    if chunk:
                        buffer_data.extend(chunk)
                    else:
                        time.sleep(0.01)
                
                print(f"✅ Buffer aufgebaut: {len(buffer_data)/32000:.1f}s")
                
                # Haupt-Analyse-Loop
                chunk_size = int(16000 * 1.0) * 2  # 1 Sekunde
                
                while self.running and self.ffmpeg_process.poll() is None:
                    try:
                        raw_bytes = self.ffmpeg_process.stdout.read(chunk_size)
                        if not raw_bytes:
                            time.sleep(0.01)
                            continue
                        
                        # Analysiere
                        audio_int16 = np.frombuffer(raw_bytes, dtype=np.int16)
                        audio_float32 = audio_int16.astype(np.float32) / 32768.0
                        
                        analysis = self.analyze_audio(audio_float32)
                        self.latest_analysis = analysis
                        
                        try:
                            self.analysis_queue.put_nowait(analysis)
                        except queue.Full:
                            try:
                                self.analysis_queue.get_nowait()
                                self.analysis_queue.put_nowait(analysis)
                            except:
                                pass
                                
                    except Exception as e:
                        print(f"❌ Analyse-Fehler: {e}")
                        break
                        
            except Exception as e:
                print(f"❌ Stream-Fehler: {e}")
                time.sleep(5)
            finally:
                if self.ffmpeg_process:
                    try:
                        self.ffmpeg_process.terminate()
                        self.ffmpeg_process.wait(timeout=2)
                    except:
                        pass
    
    def analyze_audio(self, audio_data):
        try:
            scores, _, _ = self.model(audio_data)
        except Exception as e:
            print(f"❌ YAMNet Fehler: {e}")
            return {
                'timestamp': time.time(),
                'topClasses': [],
                'dominantCategory': 'error',
                'analysisId': int(time.time() * 1000)
            }
        
        avg_scores = np.mean(scores, axis=0)
        top_indices = np.argsort(avg_scores)[-20:][::-1]
        
        top_classes = []
        for idx in top_indices:
            confidence = float(avg_scores[idx])
            if confidence < 0.005: continue
                
            class_name = self.class_names.get(idx, f"Class_{idx}")
            category = self.class_categories.get(idx, 'other')
            
            top_classes.append({
                'id': int(idx),
                'name': class_name,
                'confidence': confidence,
                'category': category,
                'color': self.get_category_color(category)
            })
        
        top_classes.sort(key=lambda x: x['confidence'], reverse=True)
        if len(top_classes) > 15:
            top_classes = top_classes[:15]
        
        category_scores = defaultdict(float)
        for cls in top_classes:
            category_scores[cls['category']] += cls['confidence']
        
        dominant_category = max(category_scores.items(), key=lambda x: x[1], default=('other', 0))[0]
        
        return {
            'timestamp': time.time(),  # ECHTE Zeit, kein Delay mehr
            'topClasses': top_classes,
            'dominantCategory': dominant_category,
            'totalClasses': len(top_classes),
            'analysisId': int(time.time() * 1000),
            'stream_delay': self.stream_delay
        }
    
    def stop(self):
        self.running = False
        if self.ffmpeg_process:
            try:
                self.ffmpeg_process.terminate()
                self.ffmpeg_process.wait(timeout=2)
            except:
                pass

# Globale Instanz
STREAM_URL = "https://icecast.radiorfm.de/rfm.ogg"
analyzer = YamnetAnalyzer(STREAM_URL)
analysis_thread = threading.Thread(target=analyzer.start_analysis, daemon=True)

@app.route('/api/yamnet/analysis')
def get_analysis():
    if analyzer.latest_analysis:
        return jsonify(analyzer.latest_analysis)
    return jsonify({'status': 'starting', 'timestamp': time.time()})

@app.route('/api/yamnet/stream')
def stream_analysis():
    def generate():
        delay_seconds = 10
        buffer = deque()
        last_keepalive = time.time()
        while True:
            try:
                analysis = analyzer.analysis_queue.get(timeout=1)
                buffer.append(analysis)
            except queue.Empty:
                pass

            now = time.time()
            while buffer:
                analysis_timestamp = buffer[0].get('timestamp', now)
                if now - analysis_timestamp < delay_seconds:
                    break
                analysis = buffer.popleft()
                yield f"data: {json.dumps(analysis)}\n\n"
                last_keepalive = time.time()

            if time.time() - last_keepalive > 2 and not buffer:
                yield f"data: {{\"keepalive\": true, \"timestamp\": {time.time()}}}\n\n"
                last_keepalive = time.time()

            time.sleep(0.05)
    
    return Response(generate(), mimetype='text/event-stream')

@app.route('/api/yamnet/status')
def get_status():
    return jsonify({
        'running': analyzer.running,
        'queue_size': analyzer.analysis_queue.qsize(),
        'stream_delay': analyzer.stream_delay,
        'timestamp': time.time()
    })

@app.route('/api/yamnet/delay', methods=['GET', 'POST'])
def manage_delay():
    if request.method == 'POST':
        data = request.get_json()
        new_delay = float(data.get('delay', 10.5))
        analyzer.stream_delay = max(0, min(60, new_delay))
        return jsonify({
            'status': 'success',
            'delay': analyzer.stream_delay,
            'timestamp': time.time()
        })
    return jsonify({
        'current_delay': analyzer.stream_delay,
        'timestamp': time.time()
    })

if __name__ == '__main__':
    print("="*60)
    print("🎯 RFM YAMNet Server mit Audio-Buffer")
    print("="*60)
    print(f"📡 Stream: {STREAM_URL}")
    print(f"⏳ Audio-Buffer: {analyzer.stream_delay}s")
    print()
    
    analysis_thread.start()
    print("🌐 API Endpoints:")
    print("  GET  /api/yamnet/analysis")
    print("  GET  /api/yamnet/stream")
    print("  GET  /api/yamnet/status")
    print("  GET/POST /api/yamnet/delay")
    print()
    print("▶️  Starte Analyse mit Buffer...")
    
    app.run(host='0.0.0.0', port=5000, debug=False, threaded=True)
