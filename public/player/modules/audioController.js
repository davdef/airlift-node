export class AudioController {
    constructor(onStateChange, onError) {
        this.onStateChange = onStateChange;
        this.onError = onError;
        this.isLive = true;
        this.playbackStartTime = null;
        this.audio = new Audio();
    }
    
    getCurrentTime(referenceTime) {
        if (this.isLive) return referenceTime;
        return this.playbackStartTime || referenceTime;
    }
    
    getState() {
        return {
            isLive: this.isLive,
            paused: this.audio.paused,
            currentTime: this.audio.currentTime
        };
    }
    
    playLive() {
        console.log('[Audio] Play live placeholder');
    }
    
    pause() {
        this.audio.pause();
    }
}
