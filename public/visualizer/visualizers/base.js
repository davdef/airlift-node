class BaseVisualizer {
    constructor(ctx, canvas) {
        this.ctx = ctx;
        this.canvas = canvas;
    }
    
    draw(frequencyData, timeData, config, deltaTime) {
        // Basis-Implementierung
    }
    
    onResize() {
        // Canvas-Größenänderung behandeln
    }
}
