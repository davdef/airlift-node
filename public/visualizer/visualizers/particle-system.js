class ParticleSystemVisualizer extends BaseVisualizer {
    constructor(ctx, canvas) {
        super(ctx, canvas);

        this.count = 64;              // Sonique-Größe
        this.particles = [];
        this.decay = 0.88;
        this.smoothFFT = null;

        this.initParticles();
    }

    initParticles() {
        this.particles.length = 0;
        const { width, height } = this.getCanvasSize();
        const cx = width / 2;
        const cy = height / 2;
        const radius = Math.min(cx, cy) * 0.6;

        for (let i = 0; i < this.count; i++) {
            const angle = (i / this.count) * Math.PI * 2;
            this.particles.push({
                angle,
                baseRadius: radius * (0.4 + Math.random() * 0.6),
                amp: 0
            });
        }
    }

    draw(frequencyData, timeData, config, deltaTime) {
        const ctx = this.ctx;
        const { width: w, height: h } = this.getCanvasSize();
        const cx = w / 2;
        const cy = h / 2;

        const speed = config?.speed ?? 1;

        // FFT glätten
        if (!this.smoothFFT || this.smoothFFT.length !== frequencyData.length) {
            this.smoothFFT = new Float32Array(frequencyData.length);
        }

        const decay = this.getDecay(this.decay, speed);
        for (let i = 0; i < frequencyData.length; i++) {
            this.smoothFFT[i] =
                this.smoothFFT[i] * decay +
                frequencyData[i] * (1 - decay);
        }

        // Decay-Clear
        ctx.fillStyle = 'rgba(0,0,0,0.2)';
        ctx.fillRect(0, 0, w, h);

        ctx.fillStyle = config.primaryColor;

        const bins = Math.min(this.count, this.smoothFFT.length);

        for (let i = 0; i < this.particles.length; i++) {
            const p = this.particles[i];
            const amp = this.smoothFFT[i % bins] / 255;

            // Amplitude glätten
            const ampDecay = this.getDecay(0.9, speed);
            p.amp = p.amp * ampDecay + amp * (1 - ampDecay);

            const r = p.baseRadius + p.amp * 120;
            const x = cx + Math.cos(p.angle) * r;
            const y = cy + Math.sin(p.angle) * r;

            ctx.beginPath();
            ctx.arc(x, y, 2 + p.amp * 4, 0, Math.PI * 2);
            ctx.fill();
        }
    }

    onResize() {
        this.initParticles();
    }
}
