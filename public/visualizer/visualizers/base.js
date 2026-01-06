class BaseVisualizer {
    constructor(ctx, canvas) {
        this.ctx = ctx;
        this.canvas = canvas;
    }

    getCanvasSize() {
        const dpr = window.devicePixelRatio || 1;
        return {
            width: this.canvas.width / dpr,
            height: this.canvas.height / dpr,
            dpr
        };
    }

    getDecay(baseDecay, speed = 1) {
        const clampedSpeed = Math.min(3, Math.max(0.1, speed ?? 1));
        return Math.pow(baseDecay, clampedSpeed);
    }
    
    draw(frequencyData, timeData, config, deltaTime) {
        // Basis-Implementierung
    }
    
    onResize() {
        // Canvas-Größenänderung behandeln
    }
}
