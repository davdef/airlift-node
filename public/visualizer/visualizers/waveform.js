class WaveformVisualizer extends BaseVisualizer {
    draw(frequencyData, timeData, config, deltaTime) {
        const { width, height } = this.getCanvasSize();
        const centerY = height / 2;
        const sliceWidth = width / timeData.length;
        
        this.ctx.lineWidth = 2;
        this.ctx.strokeStyle = config.primaryColor;
        this.ctx.beginPath();
        
        for (let i = 0; i < timeData.length; i++) {
            const value = timeData[i] / 128.0;
            const x = i * sliceWidth;
            const y = value * height / 2;
            
            if (i === 0) {
                this.ctx.moveTo(x, y);
            } else {
                this.ctx.lineTo(x, y);
            }
        }
        
        this.ctx.stroke();
    }
}
