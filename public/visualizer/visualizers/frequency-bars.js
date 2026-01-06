class FrequencyBarsVisualizer extends BaseVisualizer {
    draw(frequencyData, timeData, config, deltaTime) {
        const { width, height } = this.getCanvasSize();
        const barWidth = (width / frequencyData.length) * 2.5;
        let barHeight;
        let x = 0;
        
        for (let i = 0; i < frequencyData.length; i++) {
            barHeight = frequencyData[i] / 255 * height;
            
            // WinAmp-typischer Farbverlauf
            const gradient = this.ctx.createLinearGradient(
                x, height - barHeight,
                x, height
            );
            
            gradient.addColorStop(0, config.primaryColor);
            gradient.addColorStop(1, config.secondaryColor);
            
            this.ctx.fillStyle = gradient;
            this.ctx.fillRect(x, height - barHeight, barWidth - 1, barHeight);
            
            x += barWidth + 1;
        }
    }
}
