class ParticleSparksVisualizer extends BaseVisualizer {
    constructor(ctx, canvas) {
        super(ctx, canvas);

        this.count = 120;
        this.particles = [];
        this.decay = 0.82;
        this.smoothFFT = null;
        this.flashTimer = 0;

        this.initParticles();
    }

    initParticles() {
        this.particles.length = 0;
        const { width, height } = this.getCanvasSize();
        for (let i = 0; i < this.count; i++) {
            this.particles.push(this.createParticle(width, height));
        }
    }

    createParticle(width, height) {
        const angle = Math.random() * Math.PI * 2;
        const speed = 0.4 + Math.random() * 1.2;
        return {
            x: Math.random() * width,
            y: Math.random() * height,
            vx: Math.cos(angle) * speed,
            vy: Math.sin(angle) * speed,
            size: 1 + Math.random() * 2.5,
            flicker: Math.random() * Math.PI * 2,
            flash: 0
        };
    }

    draw(frequencyData, timeData, config, deltaTime) {
        const ctx = this.ctx;
        const { width: w, height: h } = this.getCanvasSize();
        const speed = config?.speed ?? 1;
        const sensitivity = config?.sensitivity ?? 5;
        const intensity = Math.max(0.4, sensitivity / 6);

        if (!this.smoothFFT || this.smoothFFT.length !== frequencyData.length) {
            this.smoothFFT = new Float32Array(frequencyData.length);
        }

        const decay = this.getDecay(this.decay, speed);
        for (let i = 0; i < frequencyData.length; i++) {
            this.smoothFFT[i] =
                this.smoothFFT[i] * decay +
                frequencyData[i] * (1 - decay);
        }

        ctx.fillStyle = 'rgba(0,0,0,0.22)';
        ctx.fillRect(0, 0, w, h);

        const bins = Math.min(this.count, this.smoothFFT.length);
        let energy = 0;
        for (let i = 0; i < Math.min(24, bins); i++) {
            energy += this.smoothFFT[i];
        }
        const beat = bins > 0 ? energy / (Math.min(24, bins) * 255) : 0;

        this.flashTimer -= deltaTime * 0.001;
        if (beat > 0.55 && this.flashTimer <= 0) {
            const flashCount = 3 + Math.floor(beat * 6);
            for (let i = 0; i < flashCount; i++) {
                const p = this.particles[Math.floor(Math.random() * this.particles.length)];
                p.flash = 1;
            }
            this.flashTimer = 0.25;
        }

        for (let i = 0; i < this.particles.length; i++) {
            const p = this.particles[i];
            const amp = this.smoothFFT[i % bins] / 255;
            const drift = (0.6 + amp * 2.4) * (0.4 + intensity);

            p.vx += Math.cos(p.flicker) * 0.01 * drift;
            p.vy += Math.sin(p.flicker) * 0.01 * drift;

            const maxSpeed = 2.4 + beat * 3.2;
            p.vx = Math.max(-maxSpeed, Math.min(maxSpeed, p.vx));
            p.vy = Math.max(-maxSpeed, Math.min(maxSpeed, p.vy));

            p.x += p.vx * deltaTime * 0.06 * speed;
            p.y += p.vy * deltaTime * 0.06 * speed;

            if (p.x < 0 || p.x > w) {
                p.vx *= -1;
                p.x = Math.max(0, Math.min(w, p.x));
            }
            if (p.y < 0 || p.y > h) {
                p.vy *= -1;
                p.y = Math.max(0, Math.min(h, p.y));
            }

            p.flicker += deltaTime * 0.002 * speed;
            p.flash = Math.max(0, p.flash - deltaTime * 0.004);

            const alpha = Math.min(1, 0.35 + amp * 0.7 + p.flash * 0.8);
            const size = p.size + amp * 3 + p.flash * 3;

            ctx.fillStyle = this.toRgba(config.primaryColor, alpha);
            ctx.beginPath();
            ctx.arc(p.x, p.y, size, 0, Math.PI * 2);
            ctx.fill();

            if (p.flash > 0) {
                ctx.strokeStyle = this.toRgba(config.secondaryColor, 0.6 * p.flash);
                ctx.lineWidth = 1.2;
                ctx.beginPath();
                ctx.arc(p.x, p.y, size * (1.6 + p.flash), 0, Math.PI * 2);
                ctx.stroke();
            }
        }
    }

    onResize() {
        this.initParticles();
    }

    toRgba(color, alpha) {
        if (!color) {
            return `rgba(255, 255, 255, ${alpha})`;
        }

        if (color.startsWith('#')) {
            const hex = color.replace('#', '');
            const full = hex.length === 3
                ? hex.split('').map(c => c + c).join('')
                : hex;
            const num = Number.parseInt(full, 16);
            const r = (num >> 16) & 255;
            const g = (num >> 8) & 255;
            const b = num & 255;
            return `rgba(${r}, ${g}, ${b}, ${alpha})`;
        }

        if (color.startsWith('rgba(')) {
            const parts = color
                .replace('rgba(', '')
                .replace(')', '')
                .split(',')
                .slice(0, 3)
                .map(part => part.trim());
            return `rgba(${parts.join(', ')}, ${alpha})`;
        }

        if (color.startsWith('rgb(')) {
            return color.replace('rgb(', 'rgba(').replace(')', `, ${alpha})`);
        }

        return color;
    }
}
