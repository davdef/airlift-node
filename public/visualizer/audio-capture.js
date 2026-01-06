class AudioCapture {
    constructor() {
        this.audioCtx = null;
        this.analyser = null;
        this.source = null;
        this.audioEl = null;

        this.isCapturing = false;
        this.fftSize = 256;
    }

    async init() {
        this.audioCtx = new (window.AudioContext || window.webkitAudioContext)();

        this.analyser = this.audioCtx.createAnalyser();
        this.analyser.fftSize = this.fftSize;

        this.frequencyData = new Uint8Array(this.analyser.frequencyBinCount);
        this.timeData = new Uint8Array(this.analyser.fftSize);
    }

    startStream(url) {
        if (this.audioEl) {
            this.stop();
        }

        this.audioEl = document.createElement('audio');
        this.audioEl.src = url;
        this.audioEl.crossOrigin = 'anonymous';
        this.audioEl.autoplay = true;
        this.audioEl.playsInline = true;

        this.source = this.audioCtx.createMediaElementSource(this.audioEl);
        this.source.connect(this.analyser);
        this.analyser.connect(this.audioCtx.destination);

        this.audioEl.play();
        this.isCapturing = true;
    }

    stop() {
        if (this.audioEl) {
            this.audioEl.pause();
            this.audioEl.src = '';
            this.audioEl = null;
        }
        this.isCapturing = false;
    }

    getFrequencyData() {
        if (!this.isCapturing) return null;
        this.analyser.getByteFrequencyData(this.frequencyData);
        return this.frequencyData;
    }

    getTimeData() {
        if (!this.isCapturing) return null;
        this.analyser.getByteTimeDomainData(this.timeData);
        return this.timeData;
    }
}
