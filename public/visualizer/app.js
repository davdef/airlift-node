// app.js - Komplette Version mit YAMNet Integration
class AudioVisualizerApp {
    constructor() {
        this.canvas = document.getElementById('visualizer-canvas');
        this.ctx = this.canvas.getContext('2d');

        this.audioCapture = new AudioCapture();

        this.currentVisualizer = null;
        this.visualizers = {};

        this.lastTime = 0;
        this.fps = 0;
        this.frameCount = 0;
        this.lastFpsUpdate = 0;

        this.config = {
            sensitivity: 5,
            speed: 1,
            primaryColor: '#5aa0ff',
            secondaryColor: '#8cc2ff'
        };

        this.init();
    }

    async init() {
        try {
            await this.audioCapture.init();

            this.registerVisualizers();
            this.createVisualizerButtons();
            this.setupEventListeners();

            this.resizeCanvas();
            window.addEventListener('resize', () => this.resizeCanvas());

            // Standard-Visualizer: Waveform
            this.setVisualizer('waveform');

            this.animate();
            
        } catch (error) {
            console.error('Fehler bei der Initialisierung:', error);
        }
    }

    registerVisualizers() {
        // EXISTIERENDE VISUALIZER
        this.visualizers.waveform = new WaveformVisualizer(this.ctx, this.canvas);
        this.visualizers.frequencyBars = new FrequencyBarsVisualizer(this.ctx, this.canvas);
        this.visualizers.particleNebula = new ParticleNebulaVisualizer(this.ctx, this.canvas);
        this.visualizers.particleSparks = new ParticleSparksVisualizer(this.ctx, this.canvas);
        this.visualizers.spectrumCircle = new SpectrumCircleVisualizer(this.ctx, this.canvas);
        this.visualizers.kaleidoscope = new KaleidoscopeVisualizer(this.ctx, this.canvas);
        this.visualizers.aortaLine = new AortaLineVisualizer(this.ctx, this.canvas);
        this.visualizers.aortaTunnel = new AortaTunnelVisualizer(this.ctx, this.canvas);
        this.visualizers.specNHopp = new SpecNHoppVisualizer(this.ctx, this.canvas);
        
        // YAMNET TAG CLOUD VISUALIZER
        if (typeof YamnetTagCloudVisualizer !== 'undefined') {
            this.visualizers.yamnetTagCloud = new YamnetTagCloudVisualizer(this.ctx, this.canvas);
            console.log('✅ YAMNetTagCloudVisualizerregistriert');
        } else {
            console.warn('⚠️  YamnetTagCloudVisualizer nicht gefunden');
        }
    }

    createVisualizerButtons() {
        const container = document.getElementById('visualizer-buttons');
        if (!container) return;

        // VISUALIZER-LISTE MIT ALLEN OPTIONEN
        const visualizers = {
            waveform: 'Wellenform',
            frequencyBars: 'Frequenz-Balken',
            particleNebula: 'Partikel-Nebel',
            particleSparks: 'Partikel-Sparks',
            spectrumCircle: 'Spektrum',
            kaleidoscope: 'Kaleidoskop',
            aortaLine: 'Aorta-Linie',
            aortaTunnel: 'Aorta-Tunnel',
            specNHopp: "Spec'n'Hopp"
        };
        
        // YAMNET NUR HINZUFÜGEN WENN VORHANDEN
        if (this.visualizers.yamnetTagCloud) {
            visualizers.yamnetTagCloud = 'Tag-Wolke';
        }

        container.innerHTML = '';

        Object.entries(visualizers).forEach(([id, name]) => {
            const btn = document.createElement('button');
            btn.className = 'visualizer-btn';
            btn.textContent = name;
            btn.dataset.visualizer = id;
            btn.addEventListener('click', () => this.setVisualizer(id));
            container.appendChild(btn);
        });

        // Standard-Button aktivieren
        const defaultBtn = container.querySelector('[data-visualizer="waveform"]');
        if (defaultBtn) {
            defaultBtn.classList.add('active');
        }
    }

    setVisualizer(id) {
        if (!this.visualizers[id]) {
            console.warn(`Visualizer "${id}" nicht gefunden`);
            return;
        }

        console.log(`🎨 Wechsle zu Visualizer: ${id}`);

        // Aktuellen Visualizer deaktivieren (falls vorhanden)
        if (this.currentVisualizer) {
            // Spezielle Deaktivierung für YAMNet Tag Cloud
            if (this.currentVisualizer.deactivate && 
                typeof this.currentVisualizer.deactivate === 'function') {
                this.currentVisualizer.deactivate();
            }
            
            // Canvas leeren
            this.ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);
        }

        // Neuen Visualizer setzen
        this.currentVisualizer = this.visualizers[id];
        
        // Spezielle Aktivierung für YAMNet Tag Cloud
        if (id === 'yamnetTagCloud' && 
            this.currentVisualizer.activate && 
            typeof this.currentVisualizer.activate === 'function') {
            this.currentVisualizer.activate();
        }

        // Visualizer-Name aktualisieren
        const visualizerNameElement = document.getElementById('current-visualizer');
        if (visualizerNameElement) {
            visualizerNameElement.textContent = id.toUpperCase();
        }

        // Buttons aktualisieren
        document.querySelectorAll('.visualizer-btn')
            .forEach(b => b.classList.remove('active'));
        
        const activeBtn = document.querySelector(`[data-visualizer="${id}"]`);
        if (activeBtn) {
            activeBtn.classList.add('active');
        }

        // Visualizer-spezifische Resize-Handler aufrufen
        if (this.currentVisualizer.onResize && 
            typeof this.currentVisualizer.onResize === 'function') {
            this.currentVisualizer.onResize();
        }
    }

    setupEventListeners() {
        const STREAM_URL = 'https://icecast.radiorfm.de/rfm.ogg';

        const playBtn = document.getElementById('play-btn');
        const stopBtn = document.getElementById('stop-btn');

        // Play-Button
        if (playBtn) {
            playBtn.addEventListener('click', async () => {
                try {
                    // Audio Context resume (für Autoplay Policy)
                    if (this.audioCapture.audioCtx && 
                        this.audioCapture.audioCtx.state === 'suspended') {
                        await this.audioCapture.audioCtx.resume();
                    }
                    
                    // Stream starten
                    this.audioCapture.startStream(STREAM_URL);
                    
                    // Button-Status aktualisieren
                    playBtn.classList.add('playing');
                    playBtn.innerHTML = '<i class="fas fa-arrows-rotate"></i>';
                    playBtn.title = 'Aktualisieren';
                    
                } catch (error) {
                    console.error('Fehler beim Starten des Streams:', error);
                }
            });
        }

        // Stop-Button
        if (stopBtn) {
            stopBtn.addEventListener('click', () => {
                this.audioCapture.stop();
                
                // Button-Status zurücksetzen
                if (playBtn) {
                    playBtn.classList.remove('playing');
                    playBtn.innerHTML = '<i class="fas fa-arrows-rotate"></i>';
                    playBtn.title = 'Aktualisieren';
                }
            });
        }

        // Sensitivität Slider
        const sensitivitySlider = document.getElementById('sensitivity');
        if (sensitivitySlider) {
            sensitivitySlider.addEventListener('input', e => {
                this.config.sensitivity = parseFloat(e.target.value);
            });
        }

        // Geschwindigkeit Slider
        const speedSlider = document.getElementById('speed');
        if (speedSlider) {
            speedSlider.addEventListener('input', e => {
                this.config.speed = parseFloat(e.target.value);
            });
        }

        // Primärfarbe Picker
        const primaryColorPicker = document.getElementById('primary-color');
        if (primaryColorPicker) {
            primaryColorPicker.addEventListener('input', e => {
                this.config.primaryColor = e.target.value;
            });
        }

        // Sekundärfarbe Picker
        const secondaryColorPicker = document.getElementById('secondary-color');
        if (secondaryColorPicker) {
            secondaryColorPicker.addEventListener('input', e => {
                this.config.secondaryColor = e.target.value;
            });
        }

        // Globales Keyboard-Shortcut für Play/Pause (Leertaste)
        document.addEventListener('keydown', (e) => {
            if (e.code === 'Space' && !e.target.matches('input, textarea, button')) {
                e.preventDefault();
                
                if (playBtn) {
                    if (this.audioCapture.isCapturing) {
                        stopBtn.click();
                    } else {
                        playBtn.click();
                    }
                }
            }
        });

        // Audio-Info aktualisieren
        this.updateAudioInfo();
    }

    updateAudioInfo() {
        const audioInfoElement = document.getElementById('audio-info');
        if (audioInfoElement) {
            audioInfoElement.textContent = 'RFM Live Stream';
        }
    }

resizeCanvas() {
    const container = document.querySelector('.visualizer-container');
    if (!container || !this.canvas) return;
    
    // WICHTIG: Verwende getBoundingClientRect für genaue Größe
    const rect = container.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    
    // Setze die tatsächliche Canvas-Größe (in Pixel)
    this.canvas.width = Math.floor(rect.width * dpr);
    this.canvas.height = Math.floor(rect.height * dpr);
    
    // CSS-Größe auf Container-Größe setzen
    this.canvas.style.width = `${rect.width}px`;
    this.canvas.style.height = `${rect.height}px`;
    
    // Skalierung für scharfe Darstellung
    this.ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    
    console.log(`📐 Canvas resized:`, {
        canvasPixels: `${this.canvas.width}x${this.canvas.height}`,
        cssSize: `${rect.width}x${rect.height}px`,
        dpr: dpr,
        containerRect: {
            width: rect.width,
            height: rect.height,
            top: rect.top,
            left: rect.left
        }
    });
    
    // Visualizer über Resize informieren
    if (this.currentVisualizer && 
        this.currentVisualizer.onResize && 
        typeof this.currentVisualizer.onResize === 'function') {
        this.currentVisualizer.onResize();
    }
}

    animate(currentTime = 0) {
        // Nächsten Frame anfordern
        requestAnimationFrame(t => this.animate(t));

        // FPS-Berechnung
        this.frameCount++;
        if (currentTime - this.lastFpsUpdate >= 1000) {
            this.fps = this.frameCount;
            this.frameCount = 0;
            this.lastFpsUpdate = currentTime;

            // FPS-Anzeige aktualisieren
            const fpsElement = document.getElementById('fps');
            const footerFpsElement = document.getElementById('footer-fps');
            
            if (fpsElement) fpsElement.textContent = this.fps;
            if (footerFpsElement) footerFpsElement.textContent = this.fps;
        }

        // Delta-Time berechnen
        const delta = currentTime - this.lastTime;
        this.lastTime = currentTime;

        // Nur zeichnen, wenn Audio läuft und Visualizer aktiv ist
        if (this.currentVisualizer && this.audioCapture.isCapturing) {
            // Canvas leeren
            this.ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);

            // Audio-Daten abrufen
            const freqData = this.audioCapture.getFrequencyData();
            const timeData = this.audioCapture.getTimeData();

            if (freqData && timeData) {
                // Visualizer zeichnen
                try {
                    this.currentVisualizer.draw(freqData, timeData, this.config, delta);
                } catch (error) {
                    console.error('Fehler beim Zeichnen des Visualizers:', error);
                    // Fallback: Einfache Fehlermeldung zeichnen
                    this.ctx.fillStyle = '#ff0000';
                    this.ctx.font = '20px Arial';
                    this.ctx.fillText('Visualizer Error', 20, 50);
                }
            } else {
                // Warte auf Audio-Daten
                this.ctx.fillStyle = 'rgba(255, 255, 255, 0.3)';
                this.ctx.font = '24px Arial';
                this.ctx.textAlign = 'center';
                this.ctx.fillText('Warte auf Audio...', 
                    this.canvas.width / 2, 
                    this.canvas.height / 2);
            }
            
        } else if (this.currentVisualizer && this.currentVisualizer.drawWithoutAudio) {
            // Visualizer kann ohne Audio zeichnen (z.B. YAMNet Tag Cloud im Demo-Modus)
            try {
                this.currentVisualizer.draw(null, null, this.config, delta);
            } catch (error) {
                console.error('Fehler beim Zeichnen ohne Audio:', error);
            }
            
        } else {
            // Kein Audio, leeren Canvas anzeigen
            this.ctx.fillStyle = 'rgba(10, 20, 40, 0.8)';
            this.ctx.fillRect(0, 0, this.canvas.width, this.canvas.height);
            
            // Start-Hinweis
            this.ctx.fillStyle = 'rgba(90, 160, 255, 0.7)';
            this.ctx.font = '28px Arial';
            this.ctx.textAlign = 'center';
            this.ctx.fillText('RFM Visualizer', 
                this.canvas.width / 2, 
                this.canvas.height / 2 - 30);
            
            this.ctx.fillStyle = 'rgba(255, 255, 255, 0.5)';
            this.ctx.font = '18px Arial';
            this.ctx.fillText('Klicke auf Play, um den Stream zu starten', 
                this.canvas.width / 2, 
                this.canvas.height / 2 + 20);
            
            // Aktuellen Visualizer anzeigen
            if (this.currentVisualizer) {
                const visualizerName = Object.keys(this.visualizers).find(
                    key => this.visualizers[key] === this.currentVisualizer
                );
                
                this.ctx.fillStyle = 'rgba(255, 255, 255, 0.3)';
                this.ctx.font = '14px Arial';
                this.ctx.fillText(`Visualizer: ${visualizerName || 'Unbekannt'}`, 
                    this.canvas.width / 2, 
                    this.canvas.height / 2 + 60);
            }
        }
    }

    // Hilfsfunktion zum Abrufen von Visualizer-Name
    getVisualizerName(id) {
        const names = {
            waveform: 'Waveform',
            frequencyBars: 'Frequenz-Balken',
            particleNebula: 'Partikel-Nebel',
            particleSparks: 'Partikel-Sparks',
            spectrumCircle: 'Spektrum',
            kaleidoscope: 'Kaleidoskop',
            aortaLine: 'Aorta-Line',
            aortaTunnel: 'Aorta-Tunnel',
            specNHopp: "Spec'n'Hopp",
            yamnetTagCloud: 'AI Tag Cloud'
        };
        return names[id] || id;
    }

    // Cleanup beim Beenden
    destroy() {
        // Audio stoppen
        if (this.audioCapture) {
            this.audioCapture.stop();
        }
        
        // YAMNet Visualizer deaktivieren
        if (this.visualizers.yamnetTagCloud && 
            this.visualizers.yamnetTagCloud.deactivate) {
            this.visualizers.yamnetTagCloud.deactivate();
        }
        
        // Event Listeners entfernen
        window.removeEventListener('resize', () => this.resizeCanvas());
        
        console.log('👋 AudioVisualizerApp wurde zerstört');
    }
}

// App starten, wenn DOM geladen ist
document.addEventListener('DOMContentLoaded', () => {
    // Prüfen, ob benötigte Elemente vorhanden sind
    const canvas = document.getElementById('visualizer-canvas');
    if (!canvas) {
        console.error('Canvas-Element nicht gefunden!');
        return;
    }
    
    try {
        const app = new AudioVisualizerApp();
        console.log('✅ AudioVisualizerApp erfolgreich gestartet');
        
        // Global verfügbar machen für Debugging
        window.audioVisualizerApp = app;
        
        // Cleanup beim Verlassen der Seite
        window.addEventListener('beforeunload', () => {
            if (app && typeof app.destroy === 'function') {
                app.destroy();
            }
        });
        
    } catch (error) {
        console.error('Fehler beim Starten der App:', error);
        
        // Fallback: Fehlermeldung auf Canvas anzeigen
        const ctx = canvas.getContext('2d');
        ctx.fillStyle = '#ff0000';
        ctx.font = '20px Arial';
        ctx.fillText('App-Fehler: ' + error.message, 20, 50);
    }
});

// Hilfsfunktion für Debugging
function debugVisualizers() {
    if (window.audioVisualizerApp) {
        console.log('Aktuelle Visualizers:', window.audioVisualizerApp.visualizers);
        console.log('Aktueller Visualizer:', window.audioVisualizerApp.currentVisualizer);
        console.log('Audio läuft:', window.audioVisualizerApp.audioCapture?.isCapturing);
    }
}

// CSS für aktiven Button
const style = document.createElement('style');
style.textContent = `
    .visualizer-btn.active {
        background: linear-gradient(135deg, #5aa0ff, #8cc2ff) !important;
        color: white !important;
        border-color: #5aa0ff !important;
        box-shadow: 0 2px 8px rgba(90, 160, 255, 0.3) !important;
    }
    
    #play-btn.playing {
        background: linear-gradient(135deg, #ff5a8c, #ff8c5a) !important;
        border-color: #ff5a8c !important;
    }
    
    .visualizer-btn {
        transition: all 0.3s ease !important;
    }
    
    .visualizer-btn:hover:not(.active) {
        transform: translateY(-2px) !important;
        box-shadow: 0 4px 12px rgba(0, 0, 0, 0.2) !important;
    }
`;
document.head.appendChild(style);
