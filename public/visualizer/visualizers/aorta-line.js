class AortaLineVisualizer {
    constructor(ctx, canvas) {
        this.ctx = ctx;
        this.canvas = canvas;

        this.points = 128;
        this.phase = 0;
        this.decay = 0.9;

        this.state = new Float32Array(this.points);
    }

    draw(frequencyData, timeData, config, deltaTime) {
        const ctx = this.ctx;
        const w = this.canvas.width;
        const h = this.canvas.height;

        // langsame Drift
        this.phase += deltaTime * 0.0004;

        // Decay-Clear
        ctx.fillStyle = 'rgba(0,0,0,0.15)';
        ctx.fillRect(0, 0, w, h);

        const bins = Math.min(this.points, frequencyData.length);

        for (let i = 0; i < bins; i++) {
            const target = (frequencyData[i] / 255 - 0.5) * h * 0.6;
            this.state[i] = this.state[i] * this.decay + target * (1 - this.decay);
        }

        ctx.beginPath();

        for (let i = 0; i < bins; i++) {
            const x = (i / (bins - 1)) * w;
            const wave = Math.sin(this.phase + i * 0.15) * 20;
            const y = h / 2 + this.state[i] + wave;

            if (i === 0) ctx.moveTo(x, y);
            else ctx.lineTo(x, y);
        }

        ctx.strokeStyle = config.primaryColor;
        ctx.lineWidth = 2;
        ctx.lineJoin = 'round';
        ctx.lineCap = 'round';
        ctx.stroke();
    }

    onResize() {}
}
