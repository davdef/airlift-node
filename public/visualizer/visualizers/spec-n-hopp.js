class SpecNHoppVisualizer {
    constructor(ctx, canvas) {
        this.ctx = ctx;
        this.canvas = canvas;

        this.bins = 64;
        this.pos = new Float32Array(this.bins);
        this.vel = new Float32Array(this.bins);

        this.stiffness = 0.12;
        this.damping = 0.75;
    }

    draw(frequencyData, timeData, config, deltaTime) {
        const ctx = this.ctx;
        const w = this.canvas.width;
        const h = this.canvas.height;

        // Decay-Clear
        ctx.fillStyle = 'rgba(0,0,0,0.2)';
        ctx.fillRect(0, 0, w, h);

        const bins = Math.min(this.bins, frequencyData.length);
        const barWidth = w / bins;
        const baseY = h * 0.75;

        ctx.strokeStyle = config.primaryColor;
        ctx.lineWidth = 2;

        for (let i = 0; i < bins; i++) {
            const target = (frequencyData[i] / 255) * h * 0.5;

            // Federphysik
            const force = (target - this.pos[i]) * this.stiffness;
            this.vel[i] += force;
            this.vel[i] *= this.damping;
            this.pos[i] += this.vel[i];

            const x = i * barWidth + barWidth * 0.5;
            const y = baseY - this.pos[i];

            ctx.beginPath();
            ctx.moveTo(x, baseY);
            ctx.lineTo(x, y);
            ctx.stroke();
        }
    }

    onResize() {}
}
