import { CONFIG } from './config.js';

export class ViewportController {
    constructor(bufferStart, bufferEnd) {
        this.bufferStart = bufferStart;
        this.bufferEnd = bufferEnd;
        this.left = 0;
        this.right = 0;
        this.duration = CONFIG.DEFAULT_VISIBLE_DURATION;
        this.followLive = true;
    }
    
    updateBuffer(bufferStart, bufferEnd) {
        this.bufferStart = bufferStart;
        this.bufferEnd = bufferEnd;
        this.clampToBuffer();
    }
    
    setLive(liveTime) {
        if (!this.followLive) return;
        this.right = liveTime;
        this.left = liveTime - this.duration;
        this.clampToBuffer();
    }

    pan(dx, width) {
        const span = this.duration;
        if (!span || !width) return;
        const msShift = (dx / width) * span;
        this.left -= msShift;
        this.right = this.left + span;
        this.clampToBuffer();
    }

    zoom(factor, centerX, width) {
        this.followLive = false;
        const span = this.duration;
        if (!span || !width) return;

        const rel = centerX / width;
        const centerTs = this.left + rel * span;

        const bufferSpan = this.bufferStart != null && this.bufferEnd != null
            ? (this.bufferEnd - this.bufferStart)
            : CONFIG.MAX_VISIBLE_DURATION;

        const maxDur = Math.min(CONFIG.MAX_VISIBLE_DURATION, bufferSpan || CONFIG.MAX_VISIBLE_DURATION);
        let newDur = span * factor;
        newDur = Math.max(CONFIG.MIN_VISIBLE_DURATION, Math.min(maxDur, newDur));

        this.duration = newDur;
        this.left = centerTs - newDur / 2;
        this.right = centerTs + newDur / 2;
        this.clampToBuffer();
    }

    clampToBuffer() {
        const span = this.duration;
        if (!span) return;

        if (this.bufferStart != null && this.bufferEnd != null) {
            if (span >= (this.bufferEnd - this.bufferStart)) {
                this.left = this.bufferStart;
                this.right = this.bufferEnd;
                this.duration = this.right - this.left;
                return;
            }

            if (this.left < this.bufferStart) {
                this.left = this.bufferStart;
                this.right = this.left + span;
            }

            if (this.right > this.bufferEnd) {
                this.right = this.bufferEnd;
                this.left = this.right - span;
            }
        }
    }
    
    get visibleRange() {
        return { left: this.left, right: this.right, duration: this.duration };
    }
}
