class AortaTunnelVisualizer extends BaseVisualizer {
    constructor(ctx, canvas) {
        super(ctx, canvas);

        this.rings = [];
        this.ringCount = 40;
        this.segments = 48;

        this.speed = 0.6;
        this.depth = 1.2;

        this.decay = 0.85;
        this.smoothFFT = null;

        this.initRings();
    }

    initRings() {
        this.rings.length = 0;
        for (let i = 0; i < this.ringCount; i++) {
            this.rings.push({
                z: i / this.ringCount
            });
        }
    }

    draw(frequencyData, timeData, config, deltaTime) {
        const ctx = this.ctx;
        const { width: w, height: h } = this.getCanvasSize();
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

        // sanfter Decay-Clear
        ctx.fillStyle = 'rgba(0,0,0,0.18)';
        ctx.fillRect(0, 0, w, h);

        ctx.strokeStyle = config.primaryColor;
        ctx.lineWidth = 1;

        const bins = Math.min(this.segments, this.smoothFFT.length);

        // Ringe nach vorne bewegen
        for (const ring of this.rings) {
            ring.z -= deltaTime * 0.0002 * this.speed;
            if (ring.z <= 0) ring.z += 1;
        }

        // hinten → vorne zeichnen
        for (const ring of this.rings) {
            const z = ring.z;
            const scale = 1 / (0.2 + z * this.depth);
            const baseRadius = Math.min(cx, cy) * 0.15 * scale;

            ctx.beginPath();

            for (let i = 0; i <= bins; i++) {
                const idx = i % bins;
                const angle = (idx / bins) * Math.PI * 2;

                const amp = this.smoothFFT[idx] / 255;
                const deform = amp * 60 * scale;

                const r = baseRadius + deform;
                const x = cx + Math.cos(angle) * r;
                const y = cy + Math.sin(angle) * r;

                if (i === 0) ctx.moveTo(x, y);
                else ctx.lineTo(x, y);
            }

            ctx.closePath();
            ctx.globalAlpha = Math.max(0.1, 1 - z);
            ctx.stroke();
        }

        ctx.globalAlpha = 1;
    }

    onResize() {}
}
