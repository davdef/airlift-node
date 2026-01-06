class ParticleSystemVisualizer {
    constructor(ctx, canvas) {
        this.ctx = ctx;
        this.canvas = canvas;

        this.count = 64;              // Sonique-Größe
        this.particles = [];
        this.decay = 0.88;
        this.smoothFFT = null;

        this.initParticles();
    }

    initParticles() {
        this.particles.length = 0;
        const cx = this.canvas.width / 2;
        const cy = this.canvas.height / 2;
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
        const w = this.canvas.width;
        const h = this.canvas.height;
        const cx = w / 2;
        const cy = h / 2;

        // FFT glätten
        if (!this.smoothFFT || this.smoothFFT.length !== frequencyData.length) {
            this.smoothFFT = new Float32Array(frequencyData.length);
        }

        for (let i = 0; i < frequencyData.length; i++) {
            this.smoothFFT[i] =
                this.smoothFFT[i] * this.decay +
                frequencyData[i] * (1 - this.decay);
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
            p.amp = p.amp * 0.9 + amp * 0.1;

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
