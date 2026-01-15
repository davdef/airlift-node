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
import signal
import sys
from collections import defaultdict
from dataclasses import dataclass
from typing import Optional
import logging
from datetime import datetime

# Logging einrichten
logging.basicConfig(
    level=logging.INFO,
    format='%(asctime)s - %(name)s - %(levelname)s - %(message)s',
    handlers=[
        logging.StreamHandler(),
        logging.FileHandler('/var/log/yamnet_server.log')
    ]
)
logger = logging.getLogger(__name__)

app = Flask(__name__)
CORS(app)

@dataclass
class StreamState:
    """Zustand des Audio-Streams"""
    connected: bool = False
    last_connect_time: Optional[float] = None
    connection_attempts: int = 0
    total_analyses: int = 0
    stream_start_time: Optional[float] = None
    last_audio_data: Optional[float] = None
    ffmpeg_pid: Optional[int] = None
    buffer_offset: float = 0.0

class YamnetAnalyzer:
    def __init__(self, stream_url: str, stream_delay: float = 0.0):
        self.stream_url = stream_url
        self.stream_delay = stream_delay
        self.model = hub.load('https://tfhub.dev/google/yamnet/1')
        self.ffmpeg_process: Optional[subprocess.Popen] = None
        self.analysis_queue = queue.Queue(maxsize=50)
        self.latest_analysis = None
        self.running = False
        self.should_reconnect = True
        self.state = StreamState()
        
        # Klassennamen und Kategorisierung
        self.class_names = self.load_class_names()
        self.class_categories = self.categorize_all_classes()
        
        logger.info(f"YAMNet geladen, {len(self.class_names)} Klassen")
        
        # Test YAMNet
        logger.info("Testing YAMNet with dummy audio...")
        test_audio = np.zeros(16000 * 3, dtype=np.float32)
        scores, _, _ = self.model(test_audio)
        logger.info(f"YAMNet test passed, scores shape: {scores.shape}")
        
        # Signal-Handler
        signal.signal(signal.SIGTERM, self.signal_handler)
        signal.signal(signal.SIGINT, self.signal_handler)
    
    def signal_handler(self, signum, frame):
        """Behandelt Signale für sauberes Beenden"""
        logger.info(f"Received signal {signum}, shutting down...")
        self.stop()
        sys.exit(0)
    
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
    
    def categorize_class(self, class_name: str) -> str:
        """Kategorisiert eine Klasse für die Visualisierung"""
        if not class_name:
            return 'other'
        
        lower = class_name.lower()
        
        # Musik
        if any(keyword in lower for keyword in 
               ['music', 'song', 'singing', 'sing', 'melody', 'harmony',
                'choir', 'vocal', 'opera', 'symphony', 'orchestra']):
            return 'music'
        
        # Instrumente
        if any(keyword in lower for keyword in
               ['guitar', 'drum', 'piano', 'violin', 'cello', 'trumpet',
                'saxophone', 'flute', 'clarinet', 'harp', 'banjo']):
            return 'instrument'
        
        # Sprache
        if any(keyword in lower for keyword in
               ['speech', 'talk', 'conversation', 'narration', 'monologue',
                'dialog', 'voice', 'announce', 'announcer', 'commentary']):
            return 'speech'
        
        # Menschliche Laute
        if any(keyword in lower for keyword in
               ['laughter', 'laugh', 'crying', 'cry', 'sob', 'sigh',
                'cough', 'sneeze', 'snore', 'breathing', 'gasp']):
            return 'human'
        
        # Tiere
        if any(keyword in lower for keyword in
               ['dog', 'cat', 'bird', 'horse', 'cow', 'sheep', 'pig',
                'chicken', 'rooster', 'duck', 'goose', 'owl']):
            return 'animal'
        
        # Fahrzeuge
        if any(keyword in lower for keyword in
               ['car', 'vehicle', 'engine', 'motor', 'train', 'airplane',
                'aircraft', 'helicopter', 'boat', 'ship', 'siren']):
            return 'vehicle'
        
        # Natur
        if any(keyword in lower for keyword in
               ['rain', 'wind', 'thunder', 'lightning', 'storm', 'water',
                'wave', 'stream', 'river', 'ocean', 'sea', 'fire']):
            return 'nature'
        
        # Elektronik
        if any(keyword in lower for keyword in
               ['telephone', 'phone', 'cell phone', 'computer', 'keyboard',
                'typewriter', 'printer', 'scanner', 'radio', 'television']):
            return 'electronic'
        
        # Haushalt
        if any(keyword in lower for keyword in
               ['door', 'window', 'gate', 'drawer', 'cabinet', 'chair',
                'table', 'bed', 'curtain', 'blender', 'mixer']):
            return 'household'
        
        # Werkzeuge
        if any(keyword in lower for keyword in
               ['hammer', 'saw', 'drill', 'wrench', 'screwdriver', 'nail']):
            return 'tool'
        
        # Sport
        if any(keyword in lower for keyword in
               ['applause', 'cheering', 'crowd', 'stadium', 'whistle']):
            return 'sport'
        
        # Explosionen
        if any(keyword in lower for keyword in
               ['gunshot', 'gunfire', 'explosion', 'blast', 'fireworks']):
            return 'impact'
        
        return 'other'
    
    def categorize_all_classes(self):
        """Kategorisiert alle 521 Klassen einmalig"""
        return {idx: self.categorize_class(name) for idx, name in self.class_names.items()}
    
    def get_category_color(self, category: str) -> str:
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
    
    def cleanup_ffmpeg(self):
        """Bereinigt FFmpeg-Prozess sicher"""
        if self.ffmpeg_process:
            try:
                self.ffmpeg_process.terminate()
                try:
                    self.ffmpeg_process.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    logger.warning("FFmpeg did not terminate, sending SIGKILL")
                    self.ffmpeg_process.kill()
                    self.ffmpeg_process.wait(timeout=1)
            except Exception as e:
                logger.error(f"Error cleaning up FFmpeg: {e}")
            finally:
                self.ffmpeg_process = None
                self.state.ffmpeg_pid = None
    
    def start_ffmpeg_stream(self) -> bool:
        """Startet FFmpeg-Prozess für Stream - einfache Version"""
        try:
            # FFmpeg für OGG → PCM 16kHz (bewährte Einstellungen)
            ffmpeg_cmd = [
                'ffmpeg',
                '-i', self.stream_url,
                '-f', 's16le',
                '-acodec', 'pcm_s16le',
                '-ac', '1',
                '-ar', '16000',
                '-reconnect', '1',
                '-reconnect_streamed', '1',
                '-reconnect_delay_max', '5',
                '-timeout', '15000000',
                '-loglevel', 'error',
                'pipe:1'
            ]
            
            logger.info(f"Starting FFmpeg: {' '.join(ffmpeg_cmd[:10])}...")
            
            self.ffmpeg_process = subprocess.Popen(
                ffmpeg_cmd,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                bufsize=10**6,
                start_new_session=True
            )
            
            self.state.ffmpeg_pid = self.ffmpeg_process.pid
            logger.info(f"FFmpeg started with PID {self.state.ffmpeg_pid}")
            
            # Warte auf Initialisierung
            time.sleep(3)
            
            # Prüfe ob Prozess läuft
            if self.ffmpeg_process.poll() is not None:
                stderr = ""
                try:
                    stderr = self.ffmpeg_process.stderr.read().decode('utf-8', errors='ignore')
                except:
                    pass
                if stderr:
                    logger.error(f"FFmpeg died: {stderr[:500]}")
                else:
                    logger.error("FFmpeg died without error output")
                self.cleanup_ffmpeg()
                return False
            
            logger.info("FFmpeg is running")
            return True
            
        except Exception as e:
            logger.error(f"Failed to start FFmpeg: {e}")
            self.cleanup_ffmpeg()
            return False
    
    def apply_stream_delay(self):
        """Wendet Stream-Delay an durch Warten"""
        if self.stream_delay <= 0:
            return
        
        logger.info(f"Waiting {self.stream_delay:.1f} seconds for stream delay...")
        wait_start = time.time()
        while time.time() - wait_start < self.stream_delay and self.running:
            time.sleep(0.1)
        logger.info("Stream delay applied")
    
    def start_analysis(self):
        """Haupt-Analyse-Schleife"""
        self.running = True
        
        max_backoff = 300
        base_backoff = 5
        
        logger.info("Starting YAMNet analysis loop")
        
        while self.running:
            reconnect_attempt = 0
            
            while self.running:
                logger.info(f"Connection attempt {reconnect_attempt + 1}")
                
                # Starte FFmpeg
                if not self.start_ffmpeg_stream():
                    wait_time = min(base_backoff * (2 ** reconnect_attempt), max_backoff)
                    logger.error(f"Failed to start FFmpeg, waiting {wait_time}s")
                    
                    for i in range(int(wait_time)):
                        if not self.running:
                            return
                        time.sleep(1)
                    
                    reconnect_attempt += 1
                    continue
                
                # Reset reconnect counter
                reconnect_attempt = 0
                
                # Wende Stream-Delay an
                self.apply_stream_delay()
                
                # Setze Zustand
                self.state.connected = True
                self.state.last_connect_time = time.time()
                self.state.stream_start_time = time.time()
                self.state.connection_attempts += 1
                self.state.last_audio_data = time.time()
                
                logger.info(f"Stream connected, starting analysis (delay: {self.stream_delay}s)")
                
                # Haupt-Analyse-Loop
                chunk_duration = 1.0
                chunk_size = int(16000 * chunk_duration)
                empty_reads = 0
                max_empty_reads = 100
                
                while self.running and self.ffmpeg_process and self.ffmpeg_process.poll() is None:
                    try:
                        # Audio-Daten lesen
                        raw_bytes = self.ffmpeg_process.stdout.read(chunk_size * 2)
                        
                        if not raw_bytes:
                            empty_reads += 1
                            
                            # Prüfe stderr auf Fehler
                            try:
                                stderr_line = self.ffmpeg_process.stderr.readline()
                                if stderr_line:
                                    error_msg = stderr_line.decode('utf-8', errors='ignore').strip()
                                    if error_msg and any(err in error_msg for err in 
                                                       ['Connection timed out', 'Server returned', '404', '400', '403']):
                                        logger.error(f"FFmpeg error: {error_msg}")
                                        break
                            except:
                                pass
                            
                            if empty_reads > max_empty_reads:
                                logger.warning(f"Too many empty reads ({empty_reads}), reconnecting")
                                break
                            
                            time.sleep(0.01)
                            continue
                        
                        # Reset empty counter
                        empty_reads = 0
                        self.state.last_audio_data = time.time()
                        
                        if len(raw_bytes) < chunk_size * 2 * 0.5:
                            logger.debug(f"Partial chunk: {len(raw_bytes)}/{chunk_size * 2} bytes")
                            continue
                        
                        # Analysiere
                        try:
                            audio_int16 = np.frombuffer(raw_bytes, dtype=np.int16)
                            audio_float32 = audio_int16.astype(np.float32) / 32768.0
                            
                            analysis = self.analyze_audio(audio_float32)
                            
                            # Berücksichtige Stream-Delay in timestamp
                            if self.state.stream_start_time:
                                adjusted_time = time.time() - self.stream_delay
                                analysis['timestamp'] = adjusted_time
                                analysis['stream_time'] = time.time() - self.state.stream_start_time
                            
                            self.latest_analysis = analysis
                            self.state.total_analyses += 1
                            
                            # In Queue ablegen
                            try:
                                self.analysis_queue.put_nowait(analysis)
                            except queue.Full:
                                try:
                                    self.analysis_queue.get_nowait()
                                    self.analysis_queue.put_nowait(analysis)
                                except:
                                    pass
                            
                            # Status-Log
                            if self.state.total_analyses % 60 == 0:
                                if analysis['topClasses']:
                                    top = analysis['topClasses'][0]
                                    logger.info(f"Analysis #{self.state.total_analyses}: "
                                               f"{top['name']} ({top['confidence']:.1%}), "
                                               f"Queue: {self.analysis_queue.qsize()}")
                            
                        except Exception as e:
                            logger.error(f"Analysis error: {e}")
                            continue
                            
                    except Exception as e:
                        logger.error(f"Audio processing error: {e}")
                        break
                
                # Loop beendet, bereinige
                logger.info(f"Analysis loop ended, total analyses: {self.state.total_analyses}")
                self.cleanup_ffmpeg()
                self.state.connected = False
                
                # Kurze Pause vor Reconnect
                time.sleep(2)
                break
        
        logger.info("Analysis stopped")
    
    def analyze_audio(self, audio_data: np.ndarray) -> dict:
        """Führt YAMNet-Analyse durch"""
        try:
            scores, _, _ = self.model(audio_data)
        except Exception as e:
            logger.error(f"YAMNet inference failed: {e}")
            return {
                'timestamp': time.time(),
                'topClasses': [],
                'dominantCategory': 'error',
                'totalConfidence': 0,
                'totalClasses': 0,
                'analysisId': int(time.time() * 1000)
            }
        
        avg_scores = np.mean(scores, axis=0)
        top_indices = np.argsort(avg_scores)[-20:][::-1]
        
        top_classes = []
        total_confidence = 0
        
        for idx in top_indices:
            class_name = self.class_names.get(idx, f"Class_{idx}")
            confidence = float(avg_scores[idx])
            
            if confidence < 0.005:
                continue
                
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
        
        if len(top_classes) > 15:
            top_classes = top_classes[:15]
        
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
            'analysisId': int(time.time() * 1000),
            'state': {
                'connected': self.state.connected,
                'total_analyses': self.state.total_analyses,
                'queue_size': self.analysis_queue.qsize()
            }
        }
    
    def stop(self):
        """Stoppt die Analyse sauber"""
        logger.info("Stopping analyzer...")
        self.running = False
        self.should_reconnect = False
        self.cleanup_ffmpeg()
        logger.info("Analyzer stopped")

# Globale Instanz
STREAM_URL = "https://icecast.radiorfm.de/rfm.ogg"
STREAM_DELAY = 10.5

analyzer = YamnetAnalyzer(STREAM_URL, STREAM_DELAY)

# Starte Analyse-Thread
analysis_thread = threading.Thread(target=analyzer.start_analysis, daemon=True)

@app.route('/api/yamnet/analysis')
def get_analysis():
    """Gibt die neueste Analyse zurück"""
    if analyzer.latest_analysis:
        return jsonify(analyzer.latest_analysis)
    else:
        return jsonify({
            'status': 'starting' if analyzer.running else 'stopped',
            'message': 'Analyse wird gestartet...' if analyzer.running else 'Analyse gestoppt',
            'timestamp': time.time(),
            'queueSize': analyzer.analysis_queue.qsize(),
            'streamConnected': analyzer.state.connected,
            'totalAnalyses': analyzer.state.total_analyses
        })

@app.route('/api/yamnet/stream')
def stream_analysis():
    """Server-Sent Events Stream mit Synchronisation"""
    # Hole client_id AUSSERHALB des Generators
    client_id = request.args.get('client', 'unknown')
    
    def generate():
        logger.info(f"SSE stream started for client {client_id}")
        
        last_sent_id = None
        keepalive_counter = 0
        
        # Sende Initialzustand
        init_data = {
            'type': 'init',
            'status': 'connected',
            'timestamp': time.time(),
            'client_id': client_id,
            'stream_delay': STREAM_DELAY,
            'analyzer_state': {
                'running': analyzer.running,
                'connected': analyzer.state.connected,
                'queue_size': analyzer.analysis_queue.qsize()
            }
        }
        yield f"data: {json.dumps(init_data)}\n\n"
        
        while True:
            try:
                # Versuche neue Analyse
                try:
                    analysis = analyzer.analysis_queue.get(timeout=1.0)
                except queue.Empty:
                    analysis = analyzer.latest_analysis
                
                if analysis and analysis.get('analysisId') != last_sent_id:
                    # Füge Synchronisations-Info hinzu
                    analysis['type'] = 'analysis'
                    analysis['synchronized'] = True
                    analysis['server_time'] = time.time()
                    
                    yield f"data: {json.dumps(analysis)}\n\n"
                    last_sent_id = analysis.get('analysisId')
                    keepalive_counter = 0
                else:
                    keepalive_counter += 1
                    
                    # Keep-Alive alle 10 Sekunden
                    if keepalive_counter >= 10:
                        keepalive_data = {
                            'type': 'keepalive',
                            'timestamp': time.time(),
                            'queue_size': analyzer.analysis_queue.qsize(),
                            'stream_connected': analyzer.state.connected,
                            'total_analyses': analyzer.state.total_analyses
                        }
                        yield f"data: {json.dumps(keepalive_data)}\n\n"
                        keepalive_counter = 0
                
            except Exception as e:
                logger.error(f"SSE error for client {client_id}: {e}")
                error_data = {
                    'type': 'error',
                    'message': str(e),
                    'timestamp': time.time()
                }
                yield f"data: {json.dumps(error_data)}\n\n"
                time.sleep(1)
    
    return Response(
        generate(),
        mimetype='text/event-stream',
        headers={
            'Cache-Control': 'no-cache',
            'Content-Type': 'text/event-stream',
            'X-Accel-Buffering': 'no',
            'Access-Control-Allow-Origin': '*',
            'Connection': 'keep-alive'
        }
    )

@app.route('/api/yamnet/status')
def get_status():
    """Detaillierter Status"""
    return jsonify({
        'status': 'running' if analyzer.running else 'stopped',
        'stream_url': STREAM_URL,
        'stream_delay': STREAM_DELAY,
        'state': {
            'connected': analyzer.state.connected,
            'connection_attempts': analyzer.state.connection_attempts,
            'total_analyses': analyzer.state.total_analyses,
            'last_connect_time': analyzer.state.last_connect_time,
            'last_audio_data': analyzer.state.last_audio_data,
            'stream_start_time': analyzer.state.stream_start_time
        },
        'queue': {
            'size': analyzer.analysis_queue.qsize(),
            'maxsize': analyzer.analysis_queue.maxsize
        },
        'system': {
            'timestamp': time.time(),
            'uptime': time.time() - (analyzer.state.stream_start_time or 0),
            'python_version': sys.version,
            'tensorflow_version': tf.__version__
        }
    })

@app.route('/api/yamnet/health')
def health_check():
    """Health Check für Load Balancer"""
    health_status = {
        'status': 'healthy' if analyzer.running and analyzer.state.connected else 'unhealthy',
        'timestamp': time.time(),
        'checks': {
            'analyzer_running': analyzer.running,
            'stream_connected': analyzer.state.connected,
            'ffmpeg_alive': analyzer.ffmpeg_process is not None and analyzer.ffmpeg_process.poll() is None,
            'recent_data': analyzer.state.last_audio_data is not None and 
                          (time.time() - analyzer.state.last_audio_data) < 60,
            'queue_healthy': analyzer.analysis_queue.qsize() < analyzer.analysis_queue.maxsize * 0.8
        }
    }
    
    if health_status['status'] == 'unhealthy':
        return jsonify(health_status), 503
    return jsonify(health_status)

@app.route('/api/yamnet/control/restart', methods=['POST'])
def restart_analyzer():
    """Manueller Restart"""
    global analyzer, analysis_thread
    
    logger.info("Manual restart requested")
    
    analyzer.stop()
    
    # Warte auf Thread
    if analysis_thread.is_alive():
        analysis_thread.join(timeout=5)
    
    # Neue Instanz
    analyzer = YamnetAnalyzer(STREAM_URL, STREAM_DELAY)
    
    # Neuen Thread
    analysis_thread = threading.Thread(target=analyzer.start_analysis, daemon=True)
    analysis_thread.start()
    
    time.sleep(2)
    
    return jsonify({
        'status': 'restarted',
        'timestamp': time.time(),
        'analyzer_running': analyzer.running,
        'thread_alive': analysis_thread.is_alive()
    })

@app.route('/api/yamnet/control/stop', methods=['POST'])
def stop_analyzer():
    """Manueller Stop"""
    analyzer.stop()
    return jsonify({
        'status': 'stopped',
        'timestamp': time.time()
    })

@app.route('/api/yamnet/control/start', methods=['POST'])
def start_analyzer():
    """Manueller Start"""
    global analysis_thread
    
    if not analyzer.running:
        analyzer.running = True
        analysis_thread = threading.Thread(target=analyzer.start_analysis, daemon=True)
        analysis_thread.start()
    
    return jsonify({
        'status': 'started' if analyzer.running else 'already_running',
        'timestamp': time.time()
    })

@app.route('/api/yamnet/debug')
def get_debug():
    """Debug-Info mit mehr Details"""
    ffmpeg_info = None
    if analyzer.ffmpeg_process:
        ffmpeg_info = {
            'pid': analyzer.ffmpeg_process.pid if analyzer.ffmpeg_process else None,
            'alive': analyzer.ffmpeg_process.poll() is None,
            'returncode': analyzer.ffmpeg_process.poll()
        }
    
    return jsonify({
        'analyzer': {
            'running': analyzer.running,
            'should_reconnect': analyzer.should_reconnect,
            'state': analyzer.state.__dict__,
            'ffmpeg': ffmpeg_info
        },
        'threads': {
            'analysis_alive': analysis_thread.is_alive(),
            'analysis_name': analysis_thread.name,
            'thread_count': threading.active_count()
        },
        'memory': {
            'queue_size': analyzer.analysis_queue.qsize(),
            'queue_maxsize': analyzer.analysis_queue.maxsize
        },
        'timestamp': time.time(),
        'datetime': datetime.now().isoformat()
    })

if __name__ == '__main__':
    print("\n" + "="*60)
    print("🚀 RFM YAMNet Audio Analysis Server")
    print("="*60)
    
    # Zeige Konfiguration
    print(f"\n📡 Stream: {STREAM_URL}")
    print(f"⏳ Stream-Delay: {STREAM_DELAY} Sekunden")
    print(f"🎯 Audio-Rate: 16kHz, Mono")
    print(f"📊 Analyse-Intervall: ~1 Sekunde")
    print(f"🔄 Auto-Reconnect: Aktiviert")
    
    # Starte Analyse
    print("\n▶️  Starte Analyse-Thread...")
    analysis_thread.start()
    
    # Warte auf Initialisierung
    time.sleep(3)
    
    print(f"\n📊 Initialer Status:")
    print(f"  - Thread aktiv: {analysis_thread.is_alive()}")
    print(f"  - Analyzer läuft: {analyzer.running}")
    print(f"  - Stream verbunden: {analyzer.state.connected}")
    print(f"  - Warteschlange: {analyzer.analysis_queue.qsize()}")
    
    # API Endpoints
    print("\n🌐 API Endpoints:")
    print("  GET  /api/yamnet/analysis      - Aktuelle Analyse")
    print("  GET  /api/yamnet/stream        - Echtzeit-Stream (SSE)")
    print("  GET  /api/yamnet/status        - Detaillierter Status")
    print("  GET  /api/yamnet/health        - Health Check")
    print("  GET  /api/yamnet/debug         - Debug-Informationen")
    print("  POST /api/yamnet/control/restart - Restart Analyzer")
    print("  POST /api/yamnet/control/stop  - Stop Analyzer")
    print("  POST /api/yamnet/control/start - Start Analyzer")
    
    print(f"\n📡 Server läuft auf: http://0.0.0.0:5000")
    print("   Nginx Proxy: /api/yamnet/* → http://localhost:5000/api/yamnet/*")
    print("\n⏳ Warte auf erste Audio-Analyse...")
    print("📝 Logs: /var/log/yamnet_server.log")
    print("\nDrücke Ctrl+C zum Beenden\n")
    
    try:
        app.run(host='0.0.0.0', port=5000, debug=False, threaded=True, use_reloader=False)
    except KeyboardInterrupt:
        print("\n👋 Server wird beendet...")
        analyzer.stop()
        if analysis_thread.is_alive():
            analysis_thread.join(timeout=5)
        print("✅ Server gestoppt")
