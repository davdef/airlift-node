export class Renderer {
    constructor(canvas) {
        this.canvas = canvas;
        this.ctx = canvas.getContext('2d');
    }
    
    render(viewport, history, audioState, bufferRange) {
        // Platzhalter
        const w = this.canvas.width;
        const h = this.canvas.height;
        
        this.ctx.fillStyle = '#111';
        this.ctx.fillRect(0, 0, w, h);
        
        this.ctx.fillStyle = '#fff';
        this.ctx.font = '12px sans-serif';
        this.ctx.fillText('Renderer placeholder', 10, 20);
    }
}
