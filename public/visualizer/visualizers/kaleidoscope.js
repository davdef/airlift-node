class KaleidoscopeVisualizer extends BaseVisualizer {
    constructor(ctx, canvas) {
        super(ctx, canvas);

        // Sonique-Style Parameter
        this.slices = 12;
        this.rotation = 0;

        this.decay = 0.85;
        this.smoothFFT = null;
        this.smoothTime = null;
    }

    draw(frequencyData, timeData, config, deltaTime) {
        const ctx = this.ctx;
        const { width: w, height: h } = this.getCanvasSize();
        const cx = w / 2;
        const cy = h / 2;

        const speed = config?.speed ?? 1;

        // Rotation
        this.rotation += deltaTime * 0.00035 * speed;

        // FFT glätten (einmal!)
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

        // Decay-Clear (statt Fullscreen-Fill)
        ctx.fillStyle = 'rgba(0,0,0,0.18)';
        ctx.fillRect(0, 0, w, h);

        ctx.save();
        ctx.translate(cx, cy);

        const sliceAngle = (Math.PI * 2) / this.slices;
        const maxRadius = Math.min(cx, cy) * 0.95;
        const bins = Math.min(96, this.smoothFFT.length);
        const timeBins = Math.min(128, this.smoothTime.length);

        const glow = ctx.createRadialGradient(0, 0, 10, 0, 0, maxRadius);
        glow.addColorStop(0, config.secondaryColor);
        glow.addColorStop(0.4, config.primaryColor);
        glow.addColorStop(1, 'rgba(0,0,0,0)');

        ctx.lineWidth = 1.2;
        ctx.lineJoin = 'round';
        ctx.lineCap = 'round';

        for (let s = 0; s < this.slices; s++) {
            const flip = s % 2 === 0 ? 1 : -1;
            ctx.save();
            ctx.rotate(s * sliceAngle + this.rotation);

            ctx.beginPath();
            ctx.moveTo(0, 0);
            ctx.lineTo(maxRadius * Math.tan(sliceAngle / 2), -maxRadius);
            ctx.lineTo(-maxRadius * Math.tan(sliceAngle / 2), -maxRadius);
            ctx.closePath();
            ctx.clip();

            ctx.scale(flip, 1);

            const wavePath = new Path2D();
            for (let i = 0; i < bins; i++) {
                const amp = this.smoothFFT[i] / 255;
                const r = (i / (bins - 1)) * maxRadius;
                const wobble = (amp * 0.6 + 0.2) * 80;
                const offset = Math.sin(this.rotation * 2 + i * 0.25) * wobble;
                const x = offset;
                const y = -r;
                if (i === 0) wavePath.moveTo(x, y);
                else wavePath.lineTo(x, y);
            }

            ctx.strokeStyle = config.primaryColor;
            ctx.stroke(wavePath);

            ctx.fillStyle = glow;
            ctx.globalAlpha = 0.65;
            ctx.beginPath();
            for (let i = 0; i < timeBins; i++) {
                const amp = (this.smoothTime[i] - 128) / 128;
                const r = (i / (timeBins - 1)) * maxRadius;
                const angle = (amp * 0.8 + 1) * 0.6;
                const x = Math.cos(angle) * r * 0.25;
                const y = -r;
                if (i === 0) ctx.moveTo(x, y);
                else ctx.lineTo(x, y);
            }
            ctx.lineTo(0, 0);
            ctx.closePath();
            ctx.fill();

            ctx.globalAlpha = 1;
            ctx.restore();
        }

        ctx.restore();
    }

    onResize() {
        // nichts nötig
    }
}
