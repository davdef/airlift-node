class AudioVisualizerApp {
    constructor() {
        this.canvas = document.getElementById('visualizer-canvas');
        this.ctx = this.canvas.getContext('2d');
        this.audioCapture = new AudioCapture();
        this.currentVisualizer = null;
        this.visualizers = {};
        this.animationId = null;
        this.lastTime = 0;
        this.fps = 0;
        this.frameCount = 0;
        this.lastFpsUpdate = 0;
        
        this.config = {
            sensitivity: 5,
            speed: 1,
            primaryColor: '#00ff00',
            secondaryColor: '#009900',
            effects: {
                invert: false,
                blur: false,
                trail: true
            }
        };
        
        this.init();
    }

    async init() {
        // Intro-Animation
        setTimeout(() => {
            document.getElementById('intro-overlay').classList.add('fade-out');
            setTimeout(() => {
                document.getElementById('intro-overlay').style.display = 'none';
            }, 1000);
        }, 2000);

        // Audio Capture initialisieren
        await this.audioCapture.init();
        
        // Visualizer registrieren
        this.registerVisualizers();
        
        // Event Listener
        this.setupEventListeners();
        
        // Canvas-Größe anpassen
        this.resizeCanvas();
        window.addEventListener('resize', () => this.resizeCanvas());
        
        // Standard-Visualizer setzen
        this.setVisualizer('waveform');
        
        // Animation starten
        this.animate();
    }

    registerVisualizers() {
        // Basis-Visualizer
        this.visualizers.waveform = new WaveformVisualizer(this.ctx, this.canvas);
        this.visualizers.frequencyBars = new FrequencyBarsVisualizer(this.ctx, this.canvas);
        this.visualizers.particleSystem = new ParticleSystemVisualizer(this.ctx, this.canvas);
        this.visualizers.spectrumCircle = new SpectrumCircleVisualizer(this.ctx, this.canvas);
        this.visualizers.kaleidoscope = new KaleidoscopeVisualizer(this.ctx, this.canvas);
        
        // UI-Buttons generieren
        this.createVisualizerButtons();
    }

    createVisualizerButtons() {
        const container = document.getElementById('visualizer-buttons');
        const visualizers = {
            waveform: 'Waveform',
            frequencyBars: 'Frequenz-Balken',
            particleSystem: 'Partikel-System',
            spectrumCircle: 'Spektrum-Kreis',
            kaleidoscope: 'Kaleidoskop'
        };
        
        Object.entries(visualizers).forEach(([id, name]) => {
            const button = document.createElement('button');
            button.className = 'visualizer-btn';
            button.dataset.visualizer = id;
            button.textContent = name;
            button.addEventListener('click', () => this.setVisualizer(id));
            container.appendChild(button);
        });
    }

    setVisualizer(id) {
        if (this.visualizers[id]) {
            // Aktiven Button markieren
            document.querySelectorAll('.visualizer-btn').forEach(btn => {
                btn.classList.remove('active');
            });
            document.querySelector(`[data-visualizer="${id}"]`).classList.add('active');
            
            // Visualizer setzen
            this.currentVisualizer = this.visualizers[id];
            document.getElementById('current-visualizer').textContent = 
                document.querySelector(`[data-visualizer="${id}"]`).textContent;
        }
    }

    setupEventListeners() {
        // Audio-Quellen
        document.getElementById('source-file').addEventListener('click', () => {
            document.getElementById('audio-file').click();
        });
        
        document.getElementById('audio-file').addEventListener('change', async (e) => {
            const file = e.target.files[0];
            if (file) {
                const info = await this.audioCapture.loadAudioFile(file);
                document.getElementById('track-title').textContent = 
                    `${file.name} • ${Math.round(info.duration)}s • ${info.sampleRate/1000}kHz`;
            }
        });
        
        document.getElementById('source-system').addEventListener('click', async () => {
            const success = await this.audioCapture.captureSystemAudio();
            if (success) {
                document.getElementById('track-title').textContent = 'System-Audio • Live-Capture';
            }
        });
        
        // URL-Loader
        document.getElementById('source-url').addEventListener('click', () => {
            document.getElementById('url-input').classList.toggle('hidden');
        });
        
        document.getElementById('load-url').addEventListener('click', async () => {
            const url = document.getElementById('audio-url').value;
            if (url) {
                try {
                    await this.audioCapture.loadAudioURL(url);
                    document.getElementById('track-title').textContent = `URL • ${url}`;
                } catch (error) {
                    console.error('URL loading failed:', error);
                }
            }
        });
        
        // Kontrollen
        document.getElementById('play-pause').addEventListener('click', () => {
            // Play/Pause-Logik hier implementieren
            const btn = document.getElementById('play-pause');
            const icon = btn.querySelector('i');
            
            if (icon.classList.contains('fa-play')) {
                icon.classList.replace('fa-play', 'fa-pause');
                btn.innerHTML = '<i class="fas fa-pause"></i> Pause';
                // Audio fortsetzen
            } else {
                icon.classList.replace('fa-pause', 'fa-play');
                btn.innerHTML = '<i class="fas fa-play"></i> Play';
                // Audio pausieren
            }
        });
        
        document.getElementById('stop').addEventListener('click', () => {
            this.audioCapture.stop();
            document.getElementById('play-pause').innerHTML = '<i class="fas fa-play"></i> Play';
        });
        
        // Slider
        document.getElementById('sensitivity').addEventListener('input', (e) => {
            this.config.sensitivity = parseFloat(e.target.value);
        });
        
        document.getElementById('speed').addEventListener('input', (e) => {
            this.config.speed = parseFloat(e.target.value);
        });
        
        document.getElementById('volume').addEventListener('input', (e) => {
            // Lautstärke-Steuerung
        });
        
        // Farben
        document.getElementById('primary-color').addEventListener('input', (e) => {
            this.config.primaryColor = e.target.value;
        });
        
        document.getElementById('secondary-color').addEventListener('input', (e) => {
            this.config.secondaryColor = e.target.value;
        });
        
        // Effekte
        document.getElementById('effect-invert').addEventListener('click', (e) => {
            this.config.effects.invert = !this.config.effects.invert;
            e.target.classList.toggle('active');
        });
        
        document.getElementById('effect-blur').addEventListener('click', (e) => {
            this.config.effects.blur = !this.config.effects.blur;
            e.target.classList.toggle('active');
        });
        
        document.getElementById('effect-trail').addEventListener('click', (e) => {
            this.config.effects.trail = !this.config.effects.trail;
            e.target.classList.toggle('active');
        });
    }

    resizeCanvas() {
        const container = document.querySelector('.visualizer-container');
        this.canvas.width = container.clientWidth * window.devicePixelRatio;
        this.canvas.height = container.clientHeight * window.devicePixelRatio;
        
        if (this.currentVisualizer && this.currentVisualizer.onResize) {
            this.currentVisualizer.onResize();
        }
    }

    animate(currentTime = 0) {
        this.animationId = requestAnimationFrame((timestamp) => this.animate(timestamp));
        
        // FPS-Berechnung
        this.frameCount++;
        if (currentTime - this.lastFpsUpdate >= 1000) {
            this.fps = Math.round((this.frameCount * 1000) / (currentTime - this.lastFpsUpdate));
            this.lastFpsUpdate = currentTime;
            this.frameCount = 0;
            
            document.getElementById('fps').textContent = this.fps;
            document.getElementById('footer-fps').textContent = this.fps;
        }
        
        // Delta-Zeit für gleichmäßige Animation
        const deltaTime = currentTime - this.lastTime;
        this.lastTime = currentTime;
        
        // Effekte anwenden
        this.applyEffects();
        
        // Visualizer zeichnen
        if (this.currentVisualizer && this.audioCapture.isCapturing) {
            const frequencyData = this.audioCapture.getFrequencyData();
            const timeData = this.audioCapture.getTimeData();
            
            if (frequencyData && timeData) {
                this.currentVisualizer.draw(
                    frequencyData, 
                    timeData, 
                    this.config,
                    deltaTime * this.config.speed
                );
            }
        }
    }

    applyEffects() {
        if (this.config.effects.invert) {
            this.ctx.globalCompositeOperation = 'difference';
            this.ctx.fillStyle = 'white';
            this.ctx.fillRect(0, 0, this.canvas.width, this.canvas.height);
            this.ctx.globalCompositeOperation = 'source-over';
        }
        
        if (this.config.effects.blur) {
            this.ctx.filter = 'blur(2px)';
            setTimeout(() => {
                this.ctx.filter = 'none';
            }, 0);
        }
        
        if (!this.config.effects.trail) {
            this.ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);
        } else {
            this.ctx.fillStyle = 'rgba(0, 0, 20, 0.1)';
            this.ctx.fillRect(0, 0, this.canvas.width, this.canvas.height);
        }
    }
}

// App starten
document.addEventListener('DOMContentLoaded', () => {
    new AudioVisualizerApp();
});
