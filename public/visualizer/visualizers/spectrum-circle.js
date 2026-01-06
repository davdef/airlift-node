class SpectrumCircleVisualizer extends BaseVisualizer {
    constructor(ctx, canvas) {
        super(ctx, canvas);

        this.rotation = 0;
        this.decay = 0.86;
        this.smoothFFT = null;
    }

    draw(frequencyData, timeData, config, deltaTime) {
        const ctx = this.ctx;
        const { width: w, height: h } = this.getCanvasSize();
        const cx = w / 2;
        const cy = h / 2;

        const speed = config?.speed ?? 1;

        // langsame Rotation
        this.rotation += deltaTime * 0.0003 * speed;

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
        ctx.fillStyle = 'rgba(0,0,0,0.18)';
        ctx.fillRect(0, 0, w, h);

        const bins = Math.min(128, this.smoothFFT.length);
        const baseRadius = Math.min(cx, cy) * 0.45;
        const maxRadius = Math.min(cx, cy) * 0.85;
        const angleStep = (Math.PI * 2) / bins;

        ctx.strokeStyle = config.primaryColor;
        ctx.lineWidth = 1;
        ctx.lineCap = 'round';

        for (let i = 0; i < bins; i++) {
            const amp = this.smoothFFT[i] / 255;
            if (amp < 0.02) continue;

            const angle = i * angleStep + this.rotation;
            const r = baseRadius + amp * (maxRadius - baseRadius);

            const x1 = cx + Math.cos(angle) * baseRadius;
            const y1 = cy + Math.sin(angle) * baseRadius;
            const x2 = cx + Math.cos(angle) * r;
            const y2 = cy + Math.sin(angle) * r;

            ctx.beginPath();
            ctx.moveTo(x1, y1);
            ctx.lineTo(x2, y2);
            ctx.stroke();
        }
    }

    onResize() {
        // nichts nötig
    }
}
