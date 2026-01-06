class AudioCapture {
    constructor() {
        this.audioContext = null;
        this.analyser = null;
        this.source = null;
        this.stream = null;
        this.isCapturing = false;
        this.frequencyData = null;
        this.timeData = null;
    }

    async init() {
        try {
            this.audioContext = new (window.AudioContext || window.webkitAudioContext)();
            this.analyser = this.audioContext.createAnalyser();
            
            // Bessere Auflösung für Visualisierung
            this.analyser.fftSize = 4096;
            this.analyser.smoothingTimeConstant = 0.8;
            
            const bufferLength = this.analyser.frequencyBinCount;
            this.frequencyData = new Uint8Array(bufferLength);
            this.timeData = new Uint8Array(bufferLength);
            
            return true;
        } catch (error) {
            console.error('AudioContext initialization failed:', error);
            return false;
        }
    }

    async captureSystemAudio() {
        try {
            // System-Audio über MediaDevices API erfassen
            this.stream = await navigator.mediaDevices.getDisplayMedia({
                audio: true,
                video: false
            });
            
            this.source = this.audioContext.createMediaStreamSource(this.stream);
            this.source.connect(this.analyser);
            
            // Verbindung zum Ausgang für Monitoring
            this.analyser.connect(this.audioContext.destination);
            
            this.isCapturing = true;
            return true;
        } catch (error) {
            console.error('System audio capture failed:', error);
            return false;
        }
    }

    async loadAudioFile(file) {
        try {
            const arrayBuffer = await file.arrayBuffer();
            const audioBuffer = await this.audioContext.decodeAudioData(arrayBuffer);
            
            this.source = this.audioContext.createBufferSource();
            this.source.buffer = audioBuffer;
            this.source.connect(this.analyser);
            this.analyser.connect(this.audioContext.destination);
            
            this.source.start();
            this.isCapturing = true;
            
            return {
                duration: audioBuffer.duration,
                sampleRate: audioBuffer.sampleRate
            };
        } catch (error) {
            console.error('Audio file loading failed:', error);
            return null;
        }
    }

    loadAudioURL(url) {
        return new Promise((resolve, reject) => {
            const audio = new Audio();
            audio.crossOrigin = 'anonymous';
            
            audio.addEventListener('canplay', () => {
                this.source = this.audioContext.createMediaElementSource(audio);
                this.source.connect(this.analyser);
                this.analyser.connect(this.audioContext.destination);
                
                this.isCapturing = true;
                resolve(audio);
            });
            
            audio.addEventListener('error', reject);
            audio.src = url;
        });
    }

    getFrequencyData() {
        if (this.analyser && this.isCapturing) {
            this.analyser.getByteFrequencyData(this.frequencyData);
            return this.frequencyData;
        }
        return null;
    }

    getTimeData() {
        if (this.analyser && this.isCapturing) {
            this.analyser.getByteTimeDomainData(this.timeData);
            return this.timeData;
        }
        return null;
    }

    getFrequencyBinCount() {
        return this.analyser ? this.analyser.frequencyBinCount : 0;
    }

    stop() {
        if (this.source) {
            if (this.source.stop) this.source.stop();
            if (this.source.disconnect) this.source.disconnect();
        }
        
        if (this.stream) {
            this.stream.getTracks().forEach(track => track.stop());
        }
        
        this.isCapturing = false;
    }

    setSmoothing(value) {
        if (this.analyser) {
            this.analyser.smoothingTimeConstant = value;
        }
    }
}
