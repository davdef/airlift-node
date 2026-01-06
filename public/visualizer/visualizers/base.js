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
    
    draw(frequencyData, timeData, config, deltaTime) {
        // Basis-Implementierung
    }
    
    onResize() {
        // Canvas-Größenänderung behandeln
    }
}
