# /opt/rfm/airlift-node/server/yamnet_service.py
#!/usr/bin/env python3
"""
RFM YAMNet Production Service
Mit automatischem Neustart, Health Checks und Monitoring
"""

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
import logging
import signal
import sys
from collections import defaultdict
from datetime import datetime
import psutil  # pip install psutil

# Logging konfigurieren
logging.basicConfig(
    level=logging.INFO,
    format='%(asctime)s - %(name)s - %(levelname)s - %(message)s',
    handlers=[
        logging.FileHandler('/var/log/rfm-yamnet.log'),
        logging.StreamHandler()
    ]
)
logger = logging.getLogger(__name__)

class ProductionYamnetAnalyzer:
    def __init__(self, stream_url):
        self.stream_url = stream_url
        self.model = None
        self.ffmpeg_process = None
        self.analysis_queue = queue.Queue(maxsize=30)
        self.latest_analysis = None
        self.running = False
        self.start_time = time.time()
        self.analysis_count = 0
        self.error_count = 0
        self.retry_count = 0
        self.max_retries = 5
        self.retry_delay = 30  # Sekunden
        
        # Statistik
        self.stats = {
            'total_analyses': 0,
            'average_processing_time': 0,
            'last_error': None,
            'uptime': 0
        }
        
        # Klassennamen und Kategorisierung
        self.class_names = {}
        self.class_categories = {}
        
        # Initialisiere in separatem Thread um Blockieren zu vermeiden
        self.init_thread = threading.Thread(target=self._initialize, daemon=True)
        self.init_thread.start()
        
    def _initialize(self):
        """Initialisiert YAMNet im Hintergrund"""
        try:
            logger.info("🚀 Initialisiere YAMNet Modell...")
            self.model = hub.load('https://tfhub.dev/google/yamnet/1')
            
            # Klassennamen laden
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
            
            self.class_names = class_names
            self.class_categories = self.categorize_all_classes()
            
            logger.info(f"✅ YAMNet initialisiert: {len(self.class_names)} Klassen")
            
            # Test mit Dummy-Audio
            test_audio = np.zeros(16000, dtype=np.float32)
            scores, _, _ = self.model(test_audio)
            logger.info(f"✅ Modell-Test erfolgreich: {scores.shape}")
            
        except Exception as e:
            logger.error(f"❌ YAMNet Initialisierung fehlgeschlagen: {e}")
            self.running = False
            raise
    
    def start(self):
        """Startet den Analyse-Service"""
        if not self.model:
            logger.warning("⚠️ Modell noch nicht initialisiert, warte...")
            self.init_thread.join(timeout=30)
            
        if not self.model:
            logger.error("❌ Modell konnte nicht initialisiert werden")
            return False
            
        self.running = True
        self.analysis_thread = threading.Thread(target=self._analysis_loop, daemon=True)
        self.analysis_thread.start()
        
        logger.info("▶️ Analyse-Service gestartet")
        return True
    
    def _analysis_loop(self):
        """Haupt-Analyse-Loop mit Fehlerbehandlung"""
        restart_attempts = 0
        
        while self.running and restart_attempts < self.max_retries:
            try:
                self._run_ffmpeg_analysis()
            except Exception as e:
                logger.error(f"❌ Analyse-Loop Fehler: {e}")
                self.error_count += 1
                self.stats['last_error'] = str(e)
                restart_attempts += 1
                
                if restart_attempts < self.max_retries:
                    logger.info(f"🔄 Neustartversuch {restart_attempts}/{self.max_retries} in 10s...")
                    time.sleep(10)
                else:
                    logger.error(f"⛔ Maximale Neustartversuche erreicht")
                    break
        
        self.running = False
        logger.info("⏹️ Analyse-Loop beendet")
    
    def _run_ffmpeg_analysis(self):
        """FFmpeg Analyse-Prozess"""
        # FFmpeg Konfiguration
        ffmpeg_cmd = [
            'ffmpeg',
            '-i', self.stream_url,
            '-f', 's16le',
            '-acodec', 'pcm_s16le',
            '-ac', '1',
            '-ar', '16000',
            '-loglevel', 'error',
            '-reconnect', '1',
            '-reconnect_streamed', '1',
            '-reconnect_delay_max', '5',
            'pipe:1'
        ]
        
        logger.info(f"🎯 Starte Audio-Stream: {self.stream_url}")
        
        try:
            self.ffmpeg_process = subprocess.Popen(
                ffmpeg_cmd,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                bufsize=10**7
            )
        except Exception as e:
            logger.error(f"❌ FFmpeg start failed: {e}")
            raise
        
        # Prüfe ob Prozess läuft
        time.sleep(1)
        if self.ffmpeg_process.poll() is not None:
            stderr = self.ffmpeg_process.stderr.read().decode('utf-8', errors='ignore')
            logger.error(f"❌ FFmpeg beendet sofort: {stderr[:200]}")
            raise RuntimeError(f"FFmpeg failed: {stderr[:200]}")
        
        # Audio-Verarbeitung
        chunk_size = 16000 * 3  # 3 Sekunden
        last_update = time.time()
        
        while self.running and self.ffmpeg_process.poll() is None:
            try:
                # Audio lesen
                raw_bytes = self.ffmpeg_process.stdout.read(chunk_size * 2)
                
                if not raw_bytes:
                    time.sleep(0.1)
                    continue
                
                # Konvertieren
                audio_int16 = np.frombuffer(raw_bytes, dtype=np.int16)
                if len(audio_int16) < chunk_size * 0.5:
                    continue
                    
                audio_float32 = audio_int16.astype(np.float32) / 32768.0
                
                # Analyse
                start_time = time.time()
                analysis = self.analyze_audio(audio_float32)
                processing_time = time.time() - start_time
                
                self.latest_analysis = analysis
                self.analysis_count += 1
                self.stats['total_analyses'] = self.analysis_count
                
                # Update Statistik
                if self.stats['average_processing_time'] == 0:
                    self.stats['average_processing_time'] = processing_time
                else:
                    # Gleitender Durchschnitt
                    self.stats['average_processing_time'] = (
                        0.9 * self.stats['average_processing_time'] + 0.1 * processing_time
                    )
                
                # In Queue ablegen (nicht blockierend)
                try:
                    self.analysis_queue.put_nowait(analysis)
                except queue.Full:
                    # Ältesten entfernen
                    try:
                        self.analysis_queue.get_nowait()
                        self.analysis_queue.put_nowait(analysis)
                    except:
                        pass
                
                # Status-Log alle 30 Sekunden
                if time.time() - last_update > 30:
                    queue_size = self.analysis_queue.qsize()
                    logger.info(f"📊 Status: {self.analysis_count} Analysen, Queue: {queue_size}, "
                              f"Avg time: {self.stats['average_processing_time']:.3f}s")
                    last_update = time.time()
                    
            except Exception as e:
                logger.error(f"❌ Audio-Verarbeitungsfehler: {e}")
                time.sleep(1)
                continue
        
        # FFmpeg beenden
        self._stop_ffmpeg()
    
    def analyze_audio(self, audio_data):
        """Audio-Analyse mit YAMNet"""
        try:
            scores, _, _ = self.model(audio_data)
        except Exception as e:
            logger.error(f"❌ YAMNet Inferenz fehlgeschlagen: {e}")
            return self._create_error_analysis()
        
        avg_scores = np.mean(scores, axis=0)
        top_indices = np.argsort(avg_scores)[-15:][::-1]
        
        top_classes = []
        total_confidence = 0
        
        for idx in top_indices:
            confidence = float(avg_scores[idx])
            if confidence < 0.005:
                continue
                
            class_name = self.class_names.get(idx, f"Class_{idx}")
            category = self.class_categories.get(idx, 'other')
            
            top_classes.append({
                'id': int(idx),
                'name': class_name,
                'confidence': confidence,
                'category': category,
                'color': self.get_category_color(category)
            })
            
            total_confidence += confidence
        
        top_classes.sort(key=lambda x: x['confidence'], reverse=True)
        
        # Kategorie-Statistik
        category_scores = defaultdict(float)
        for cls in top_classes:
            category_scores[cls['category']] += cls['confidence']
        
        dominant_category = max(category_scores.items(), key=lambda x: x[1], default=('other', 0))[0]
        
        return {
            'timestamp': time.time(),
            'topClasses': top_classes[:10],  # Nur Top 10
            'dominantCategory': dominant_category,
            'totalConfidence': round(total_confidence, 3),
            'totalClasses': len(top_classes),
            'analysisId': int(time.time() * 1000),
            'serverTime': datetime.now().isoformat()
        }
    
    def _create_error_analysis(self):
        """Erstellt eine Fehler-Analyse"""
        return {
            'timestamp': time.time(),
            'topClasses': [],
            'dominantCategory': 'error',
            'totalConfidence': 0,
            'totalClasses': 0,
            'analysisId': int(time.time() * 1000),
            'error': True,
            'serverTime': datetime.now().isoformat()
        }
    
    def _stop_ffmpeg(self):
        """Stoppt FFmpeg sicher"""
        if self.ffmpeg_process:
            try:
                self.ffmpeg_process.terminate()
                self.ffmpeg_process.wait(timeout=2)
            except:
                try:
                    self.ffmpeg_process.kill()
                except:
                    pass
            finally:
                self.ffmpeg_process = None
    
    def stop(self):
        """Stoppt den Service komplett"""
        self.running = False
        self._stop_ffmpeg()
        logger.info("⏹️ Service gestoppt")
    
    def get_status(self):
        """Gibt detaillierten Status zurück"""
        return {
            'running': self.running,
            'uptime': round(time.time() - self.start_time, 1),
            'analysis_count': self.analysis_count,
            'error_count': self.error_count,
            'queue_size': self.analysis_queue.qsize(),
            'stats': self.stats,
            'ffmpeg_alive': self.ffmpeg_process is not None and self.ffmpeg_process.poll() is None,
            'stream_url': self.stream_url,
            'model_loaded': self.model is not None,
            'memory_usage': psutil.Process().memory_info().rss / 1024 / 1024  # MB
        }
    
    # Hilfsmethoden (aus deinem Originalcode)
    def categorize_all_classes(self):
        categories = {}
        for idx, name in self.class_names.items():
            categories[idx] = self.categorize_class(name)
        return categories
    
    def categorize_class(self, class_name):
        # ... (gleiche Methode wie in deinem Code)
        pass
    
    def get_category_color(self, category):
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

# Flask App
app = Flask(__name__)
CORS(app)
analyzer = ProductionYamnetAnalyzer("https://icecast.radiorfm.de/rfm.ogg")

@app.route('/api/yamnet/analysis')
def get_analysis():
    return jsonify(analyzer.latest_analysis or analyzer._create_error_analysis())

@app.route('/api/yamnet/stream')
def stream_analysis():
    def generate():
        while True:
            try:
                analysis = analyzer.analysis_queue.get(timeout=2)
                yield f"data: {json.dumps(analysis)}\n\n"
            except queue.Empty:
                yield f"data: {{\"keepalive\": true, \"timestamp\": {time.time()}}}\n\n"
    
    return Response(generate(), mimetype='text/event-stream')

@app.route('/api/yamnet/status')
def get_status():
    return jsonify(analyzer.get_status())

@app.route('/api/yamnet/health')
def health_check():
    status = analyzer.get_status()
    
    # Health Check Logik
    is_healthy = (
        status['running'] and
        status['model_loaded'] and
        status['ffmpeg_alive'] and
        analyzer.analysis_count > 0 and
        status['error_count'] < 10
    )
    
    return jsonify({
        'status': 'healthy' if is_healthy else 'unhealthy',
        'timestamp': datetime.now().isoformat(),
        'details': status
    })

@app.route('/api/yamnet/restart', methods=['POST'])
def restart_service():
    analyzer.stop()
    time.sleep(2)
    success = analyzer.start()
    return jsonify({'success': success, 'message': 'Service neugestartet'})

# Signal-Handler für sauberes Beenden
def signal_handler(signum, frame):
    logger.info(f"Signal {signum} empfangen, beende Service...")
    analyzer.stop()
    sys.exit(0)

if __name__ == '__main__':
    # Signal-Handler registrieren
    signal.signal(signal.SIGINT, signal_handler)
    signal.signal(signal.SIGTERM, signal_handler)
    
    logger.info("🚀 RFM YAMNet Production Service startet...")
    
    # Service starten
    if analyzer.start():
        logger.info("✅ Service erfolgreich gestartet")
        
        # Flask Server starten
        app.run(
            host='0.0.0.0',
            port=5000,
            debug=False,
            threaded=True,
            use_reloader=False
        )
    else:
        logger.error("❌ Service konnte nicht gestartet werden")
        sys.exit(1)
