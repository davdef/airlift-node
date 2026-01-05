export class ViewportController {
    constructor(bufferStart, bufferEnd) {
        this.bufferStart = bufferStart;
        this.bufferEnd = bufferEnd;
        this.left = 0;
        this.right = 0;
        this.duration = 30000; // 30 Sekunden
        this.followLive = true;
    }
    
    updateBuffer(bufferStart, bufferEnd) {
        this.bufferStart = bufferStart;
        this.bufferEnd = bufferEnd;
    }
    
    setLive(liveTime) {
        if (!this.followLive) return;
        this.right = liveTime;
        this.left = liveTime - this.duration;
    }
    
    get visibleRange() {
        return { left: this.left, right: this.right, duration: this.duration };
    }
}
