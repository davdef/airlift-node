class ParticleNebulaVisualizer extends BaseVisualizer {
    constructor(ctx, canvas) {
        super(ctx, canvas);

        this.count = 140;
        this.particles = [];
        this.decay = 0.84;
        this.smoothFFT = null;
        this.smoothTime = null;
        this.rotation = 0;

        this.initParticles();
    }

    initParticles() {
        this.particles.length = 0;
        const { width, height } = this.getCanvasSize();
        const cx = width / 2;
        const cy = height / 2;
        const maxRadius = Math.min(cx, cy) * 0.7;
        const minRadius = maxRadius * 0.18;

        for (let i = 0; i < this.count; i++) {
            const angle = Math.random() * Math.PI * 2;
            const radius = minRadius + Math.random() * (maxRadius - minRadius);
            this.particles.push({
                angle,
                radius,
                drift: (0.2 + Math.random() * 0.6) * (Math.random() > 0.5 ? 1 : -1),
                size: 0.8 + Math.random() * 2.4,
                twinkle: Math.random() * Math.PI * 2,
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
        const sensitivity = config?.sensitivity ?? 5;
        const intensity = Math.max(0.4, sensitivity / 6);
        const maxRadius = Math.min(cx, cy) * 0.7;

        if (!this.smoothFFT || this.smoothFFT.length !== frequencyData.length) {
            this.smoothFFT = new Float32Array(frequencyData.length);
        }

        if (!this.smoothTime || this.smoothTime.length !== timeData.length) {
            this.smoothTime = new Float32Array(timeData.length);
        }

        const decay = this.getDecay(this.decay, speed);
        for (let i = 0; i < frequencyData.length; i++) {
            this.smoothFFT[i] =
                this.smoothFFT[i] * decay +
                frequencyData[i] * (1 - decay);
        }

        for (let i = 0; i < timeData.length; i++) {
            this.smoothTime[i] =
                this.smoothTime[i] * decay +
                timeData[i] * (1 - decay);
        }

        this.rotation += deltaTime * 0.00018 * speed;

        ctx.fillStyle = 'rgba(0,0,0,0.24)';
        ctx.fillRect(0, 0, w, h);

        const bins = Math.min(this.count, this.smoothFFT.length);
        const timeBins = Math.min(120, this.smoothTime.length);

        let midEnergy = 0;
        const midStart = Math.floor(bins * 0.2);
        const midEnd = Math.min(bins, midStart + 16);
        for (let i = midStart; i < midEnd; i++) {
            midEnergy += this.smoothFFT[i];
        }
        const midAmp = midEnd > midStart ? midEnergy / ((midEnd - midStart) * 255) : 0;

        const nebulaRadius = maxRadius * (0.35 + midAmp * 0.4);
        const nebulaGlow = ctx.createRadialGradient(cx, cy, 0, cx, cy, nebulaRadius * 2.2);
        nebulaGlow.addColorStop(0, this.toRgba(config.secondaryColor, 0.45));
        nebulaGlow.addColorStop(0.5, this.toRgba(config.primaryColor, 0.2));
        nebulaGlow.addColorStop(1, 'rgba(0,0,0,0)');
        ctx.fillStyle = nebulaGlow;
        ctx.beginPath();
        ctx.arc(cx, cy, nebulaRadius * 2, 0, Math.PI * 2);
        ctx.fill();

        ctx.save();
        ctx.translate(cx, cy);
        for (let i = 0; i < this.particles.length; i++) {
            const p = this.particles[i];
            const amp = this.smoothFFT[i % bins] / 255;
            const ampDecay = this.getDecay(0.9, speed);
            p.amp = p.amp * ampDecay + amp * (1 - ampDecay);

            p.angle += deltaTime * 0.0004 * speed * p.drift * (0.6 + intensity);

            const wave = Math.sin(this.rotation * 2 + p.twinkle) * maxRadius * 0.04;
            const r = Math.min(
                maxRadius,
                p.radius + wave + p.amp * maxRadius * 0.26
            );

            const angle = p.angle + Math.sin(this.rotation + p.twinkle) * 0.05;
            const x = Math.cos(angle) * r;
            const y = Math.sin(angle) * r;

            if (p.amp > 0.12) {
                ctx.strokeStyle = this.toRgba(config.secondaryColor, 0.15 + p.amp * 0.35);
                ctx.lineWidth = 1;
                ctx.beginPath();
                ctx.moveTo(x * 0.4, y * 0.4);
                ctx.lineTo(x, y);
                ctx.stroke();
            }

            const size = p.size + p.amp * 2.8;
            ctx.fillStyle = this.toRgba(config.primaryColor, 0.5 + p.amp * 0.5);
            ctx.beginPath();
            ctx.arc(x, y, size, 0, Math.PI * 2);
            ctx.fill();
        }
        ctx.restore();

        ctx.strokeStyle = this.toRgba(config.secondaryColor, 0.2);
        ctx.lineWidth = 1;
        ctx.beginPath();
        for (let i = 0; i < timeBins; i += 5) {
            const amp = (this.smoothTime[i] - 128) / 128;
            const angle = (i / timeBins) * Math.PI * 2 - this.rotation * 0.8;
            const r = maxRadius * (0.38 + amp * 0.09 + Math.sin(this.rotation + i * 0.15) * 0.02);
            const x = cx + Math.cos(angle) * r;
            const y = cy + Math.sin(angle) * r;
            if (i === 0) ctx.moveTo(x, y);
            else ctx.lineTo(x, y);
        }
        ctx.closePath();
        ctx.stroke();
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
