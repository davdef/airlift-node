class KaleidoscopeVisualizer {
    constructor(ctx, canvas) {
        this.ctx = ctx;
        this.canvas = canvas;

        // Sonique-Style Parameter
        this.slices = 6;
        this.rotation = 0;

        this.decay = 0.85;
        this.smoothFFT = null;
    }

    draw(frequencyData, timeData, config, deltaTime) {
        const ctx = this.ctx;
        const w = this.canvas.width;
        const h = this.canvas.height;
        const cx = w / 2;
        const cy = h / 2;

        // Rotation sehr langsam
        this.rotation += deltaTime * 0.00025;

        // FFT glätten (einmal!)
        if (!this.smoothFFT || this.smoothFFT.length !== frequencyData.length) {
            this.smoothFFT = new Float32Array(frequencyData.length);
        }

        for (let i = 0; i < frequencyData.length; i++) {
            this.smoothFFT[i] =
                this.smoothFFT[i] * this.decay +
                frequencyData[i] * (1 - this.decay);
        }

        // Decay-Clear (statt Fullscreen-Fill)
        ctx.fillStyle = 'rgba(0,0,0,0.15)';
        ctx.fillRect(0, 0, w, h);

        ctx.save();
        ctx.translate(cx, cy);

        const sliceAngle = (Math.PI * 2) / this.slices;
        const maxRadius = Math.min(cx, cy) * 0.9;
        const bins = Math.min(128, this.smoothFFT.length);

        ctx.strokeStyle = config.primaryColor;
        ctx.lineWidth = 1;

        for (let s = 0; s < this.slices; s++) {
            ctx.save();
            ctx.rotate(s * sliceAngle + this.rotation);

            ctx.beginPath();

            for (let i = 0; i < bins; i++) {
                const amp = this.smoothFFT[i] / 255;
                if (amp < 0.02) continue;

                const r = (i / bins) * maxRadius;
                const y = -r;
                const x = amp * 120;

                if (i === 0) ctx.moveTo(x, y);
                else ctx.lineTo(x, y);
            }

            ctx.stroke();
            ctx.restore();
        }

        ctx.restore();
    }

    onResize() {
        // nichts nötig
    }
}
