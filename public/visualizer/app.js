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
        await this.audioCapture.init();

        this.registerVisualizers();
        this.createVisualizerButtons();
        this.setupEventListeners();

        this.resizeCanvas();
        window.addEventListener('resize', () => this.resizeCanvas());

        this.setVisualizer('waveform');

        this.animate();
    }

    registerVisualizers() {
        this.visualizers.waveform = new WaveformVisualizer(this.ctx, this.canvas);
        this.visualizers.frequencyBars = new FrequencyBarsVisualizer(this.ctx, this.canvas);
        this.visualizers.particleSystem = new ParticleSystemVisualizer(this.ctx, this.canvas);
        this.visualizers.particleNebula = new ParticleNebulaVisualizer(this.ctx, this.canvas);
        this.visualizers.spectrumCircle = new SpectrumCircleVisualizer(this.ctx, this.canvas);
        this.visualizers.kaleidoscope = new KaleidoscopeVisualizer(this.ctx, this.canvas);
        this.visualizers.aortaLine = new AortaLineVisualizer(this.ctx, this.canvas);
        this.visualizers.aortaTunnel = new AortaTunnelVisualizer(this.ctx, this.canvas);
        this.visualizers.specNHopp = new SpecNHoppVisualizer(this.ctx, this.canvas);
        this.visualizers.tagCloud = new TagCloudVisualizer(this.ctx, this.canvas);
    }

    createVisualizerButtons() {
        const container = document.getElementById('visualizer-buttons');
        if (!container) return;

        const visualizers = {
            waveform: 'Waveform',
            frequencyBars: 'Frequenz-Balken',
            particleSystem: 'Partikel',
            particleNebula: 'Partikel-Nebel',
            spectrumCircle: 'Spektrum',
            kaleidoscope: 'Kaleidoskop',
            aortaLine: 'Aorta Line',
            aortaTunnel: 'Aorta Tunnel',
            specNHopp: "Spec’n’Hopp",
            tagCloud: 'Tag-Wolke'
        };

        this.visualizerLabels = visualizers;
        container.innerHTML = '';

        Object.entries(visualizers).forEach(([id, name]) => {
            const btn = document.createElement('button');
            btn.className = 'visualizer-btn';
            btn.textContent = name;
            btn.dataset.visualizer = id;
            btn.addEventListener('click', () => this.setVisualizer(id));
            container.appendChild(btn);
        });
    }

    setVisualizer(id) {
        if (!this.visualizers[id]) return;

        this.currentVisualizer?.setActive?.(false);
        document.querySelectorAll('.visualizer-btn')
            .forEach(b => b.classList.remove('active'));

        const btn = document.querySelector(`[data-visualizer="${id}"]`);
        btn?.classList.add('active');

        this.currentVisualizer = this.visualizers[id];
        this.currentVisualizer?.setActive?.(true);
        document.getElementById('current-visualizer').textContent = this.visualizerLabels?.[id] ?? id;
    }

    setupEventListeners() {
        const STREAM_URL = 'https://icecast.radiorfm.de/rfm.ogg';

        const playBtn = document.getElementById('play-btn');
        const stopBtn = document.getElementById('stop-btn');

        playBtn?.addEventListener('click', async () => {
            await this.audioCapture.audioCtx.resume();
            this.audioCapture.startStream(STREAM_URL);
        });

        stopBtn?.addEventListener('click', () => {
            this.audioCapture.stop();
        });

        document.getElementById('sensitivity')?.addEventListener('input', e => {
            this.config.sensitivity = parseFloat(e.target.value);
        });

        document.getElementById('speed')?.addEventListener('input', e => {
            this.config.speed = parseFloat(e.target.value);
        });

        document.getElementById('primary-color')?.addEventListener('input', e => {
            this.config.primaryColor = e.target.value;
        });

        document.getElementById('secondary-color')?.addEventListener('input', e => {
            this.config.secondaryColor = e.target.value;
        });
    }

    resizeCanvas() {
        const container = document.querySelector('.visualizer-container');
        const dpr = window.devicePixelRatio || 1;

        this.canvas.width = container.clientWidth * dpr;
        this.canvas.height = container.clientHeight * dpr;
        this.ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

        this.currentVisualizer?.onResize?.();
    }

    animate(currentTime = 0) {
        requestAnimationFrame(t => this.animate(t));

        this.frameCount++;
        if (currentTime - this.lastFpsUpdate >= 1000) {
            this.fps = this.frameCount;
            this.frameCount = 0;
            this.lastFpsUpdate = currentTime;

            document.getElementById('fps').textContent = this.fps;
            document.getElementById('footer-fps').textContent = this.fps;
        }

        const delta = currentTime - this.lastTime;
        this.lastTime = currentTime;

        if (this.currentVisualizer && this.audioCapture.isCapturing) {

    this.ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);

    const freq = this.audioCapture.getFrequencyData();
    const time = this.audioCapture.getTimeData();

    if (freq && time) {
        this.currentVisualizer.draw(freq, time, this.config, delta);
    }

        }
    }
}

document.addEventListener('DOMContentLoaded', () => {
    new AudioVisualizerApp();
});
