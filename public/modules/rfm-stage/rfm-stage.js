// public/modules/rfm-stage/rfm-stage.js
import '/visualizer/visualizers/yamnet-tagcloud.js';

export class RfmStage {
    constructor(rootEl, options = {}) {
        if (!rootEl) throw new Error('RfmStage: rootEl fehlt');

        this.rootEl = rootEl;
        this.options = {
            videoUrl: null,
            audioUrl: null,
            yamnetEndpoint: null,
            hlsScriptUrl: 'https://cdn.jsdelivr.net/npm/hls.js@1.5.18/dist/hls.min.js',
            timings: {
                pollInterval: 1500,
                playlistTimeout: 1000,
                playlistStaleTimeout: 5000,
                stallTimeout: 1500,
                switchCooldown: 400,
                transitionDuration: 350,
                videoStartTimeout: 5000,
            },
            switching: {
                initFailsToCanvas: 2,
                playlistAdvanceRequired: 2,
                minPlaylistAdvance: 1,
                maxHlsRetries: 3,
            },
            audio: {
                controls: false,
                autoplay: true,
                muted: false,
                preload: 'none',
            },
            debug: {
                logLevel: 'info',
                showMetrics: false,
                exposeGlobal: false,
            },
            ...options,
        };

        this.timings = { ...this.options.timings };
        this.switching = { ...this.options.switching };
        this.audioConfig = { ...this.options.audio };
        this.debug = { ...this.options.debug };

        if (this.debug.exposeGlobal) {
            window.rfmStage = this;
        }

        this.state = 'init';
        this.stageEl = null;
        this.video = null;
        this.audio = null;
        this.canvas = null;
        this.ctx = null;
        this.tagCloud = null;
        this.hls = null;

        this._mon = {
            playlist: {
                lastSignature: null,
                lastAdvanceAt: 0,
                advanceCount: 0,
                consecutiveFails: 0,
            },
            played: {
                mediaSequence: null,
                segmentUri: null,
            },
            video: {
                lastFrameAt: 0,
                hlsRetries: 0,
            },
            switching: {
                lastSwitchAt: 0,
                cooldownUntil: 0,
            },
        };

        this._timers = {
            poll: null,
            stall: null,
            metrics: null,
        };

        this._injectStyles();
        this._log('info', '🚀 RFM Stage initialisiert', this.timings);
    }

    /* ---------------- LOGGING ---------------- */

    _log(level, message, data = null) {
        const levels = { error: 0, warn: 1, info: 2, debug: 3 };
        const currentLevel = levels[this.debug.logLevel] ?? 1;
        const messageLevel = levels[level] ?? 1;

        if (messageLevel > currentLevel) return;

        const ts = performance.now().toFixed(1);
        const prefix = `[${ts}ms]`;

        const colors = {
            info: '#4ECDC4',
            warn: '#FFD166',
            error: '#FF6B6B',
            debug: '#607D8B',
        };

        const style = `color: ${colors[level] || '#fff'}; font-weight: bold`;

        if (data) {
            console.log(`%c${prefix} ${message}`, style, data);
        } else {
            console.log(`%c${prefix} ${message}`, style);
        }
    }

    /* ---------------- STYLES ---------------- */

    _injectStyles() {
        if (document.getElementById('rfm-stage-styles')) return;

        const style = document.createElement('style');
        style.id = 'rfm-stage-styles';
        style.textContent = `
            .rfm-stage {
                position: relative;
                width: 100%;
                aspect-ratio: 16 / 9;
                background: #000;
                overflow: hidden;
            }
            .rfm-stage-visual {
                position: absolute;
                inset: 0;
            }
            .rfm-stage-video,
            .rfm-stage-canvas {
                position: absolute;
                inset: 0;
                width: 100%;
                height: 100%;
                object-fit: cover;
                transition: opacity ${this.timings.transitionDuration}ms ease;
            }
            .rfm-stage-canvas {
                pointer-events: none;
            }
            .rfm-stage--video .rfm-stage-canvas,
            .rfm-stage--canvas .rfm-stage-video,
            .rfm-stage--init .rfm-stage-canvas,
            .rfm-stage--init .rfm-stage-video {
                opacity: 0;
                visibility: hidden;
            }
            .rfm-stage--video .rfm-stage-video,
            .rfm-stage--canvas .rfm-stage-canvas {
                opacity: 1;
                visibility: visible;
            }
            .rfm-stage-audio {
                width: 100%;
                margin-top: 12px;
            }
        `;
        document.head.appendChild(style);
    }

    /* ---------------- MAIN FLOW ---------------- */

    async mount() {
        this._log('info', '📦 MOUNT');
        this._buildDOM();

        await this._waitForVisualizer();
        if (window.YamnetTagCloudVisualizer) {
            this.tagCloud = new window.YamnetTagCloudVisualizer(this.ctx, this.canvas);
            if (this.options.yamnetEndpoint) {
                this.tagCloud.streamEndpoint = this.options.yamnetEndpoint;
            }
            this.tagCloud.theme = 'light';
        }

        this._startPolling();
        this._pollPlaylist();
    }

    destroy() {
        this._log('info', '🧨 DESTROY');
        this._stopAllTimers();
        this.tagCloud?.deactivate?.();

        if (this.hls) {
            this.hls.destroy();
            this.hls = null;
        }

        if (this.video) {
            this.video.pause();
            this.video.removeAttribute('src');
            this.video.load();
        }

        if (this.audio) {
            this.audio.pause();
            this.audio.removeAttribute('src');
            this.audio.load();
        }

        this.rootEl.innerHTML = '';
    }

    /* ---------------- POLLING ---------------- */

    _startPolling() {
        if (this._timers.poll) clearInterval(this._timers.poll);
        this._timers.poll = setInterval(() => {
            if (this.state !== 'video') {
                this._pollPlaylist();
            }
        }, this.timings.pollInterval);

        if (this.debug.showMetrics) {
            this._timers.metrics = setInterval(() => this._logMetrics(), 1000);
        } else if (this._timers.metrics) {
            clearInterval(this._timers.metrics);
            this._timers.metrics = null;
        }
    }

    _pollPlaylist() {
        if (!this.options.videoUrl) return;
        if (Date.now() < this._mon.switching.cooldownUntil) return;

        const controller = new AbortController();
        const timeout = setTimeout(() => controller.abort(), this.timings.playlistTimeout);
        const start = Date.now();

        fetch(this.options.videoUrl, {
            method: 'GET',
            cache: 'no-cache',
            signal: controller.signal,
        })
            .then(response => {
                if (!response.ok) {
                    throw new Error(`HTTP ${response.status}`);
                }
                return response.text();
            })
            .then(text => {
                const latency = Date.now() - start;
                const info = this._parsePlaylistInfo(text);
                this._handlePlaylist(info, latency);
            })
            .catch(error => {
                this._handlePlaylistFailure(error.name || 'fetch error');
            })
            .finally(() => {
                clearTimeout(timeout);
            });
    }

    _parsePlaylistInfo(playlistText) {
        const lines = playlistText
            .split(/\r?\n/)
            .map(line => line.trim())
            .filter(Boolean);

        let mediaSequence = null;
        let lastSegmentUri = null;
        let endList = false;

        for (const line of lines) {
            if (line.startsWith('#EXT-X-MEDIA-SEQUENCE:')) {
                const value = Number(line.split(':')[1]);
                if (Number.isFinite(value)) {
                    mediaSequence = value;
                }
            }
            if (line === '#EXT-X-ENDLIST') {
                endList = true;
            }
        }

        for (let i = lines.length - 1; i >= 0; i -= 1) {
            const line = lines[i];
            if (!line.startsWith('#')) {
                lastSegmentUri = line;
                break;
            }
        }

        return { mediaSequence, lastSegmentUri, endList };
    }

    _handlePlaylist(info, latency) {
        if (info.endList || (!info.lastSegmentUri && info.mediaSequence === null)) {
            this._handlePlaylistFailure(info.endList ? 'endlist' : 'empty');
            return;
        }

        this._mon.playlist.consecutiveFails = 0;

        const signature = `${info.mediaSequence ?? 'na'}|${info.lastSegmentUri ?? 'na'}`;
        const now = Date.now();

        if (signature !== this._mon.playlist.lastSignature) {
            this._mon.playlist.lastSignature = signature;
            this._mon.playlist.lastAdvanceAt = now;

            if (this._playlistHasNewContent(info)) {
                this._mon.playlist.advanceCount += 1;
                this._log('debug', `📼 Playlist weiter (${this._mon.playlist.advanceCount})`, info);
            } else {
                this._mon.playlist.advanceCount = 0;
                this._log('debug', '📼 Playlist ok, aber keine neuen Segmente');
            }
        } else if (now - this._mon.playlist.lastAdvanceAt > this.timings.playlistStaleTimeout) {
            this._handlePlaylistFailure('stale playlist');
            return;
        }

        if (this.state !== 'video') {
            if (this._mon.playlist.advanceCount >= this.switching.playlistAdvanceRequired) {
                this._log('info', `🎬 Video verfügbar (${latency}ms, neue Segmente) → Video`);
                this._switchToVideo();
            }
        }
    }

    _handlePlaylistFailure(reason) {
        this._mon.playlist.consecutiveFails += 1;
        this._mon.playlist.advanceCount = 0;

        this._log('debug', `📡 Playlist fail #${this._mon.playlist.consecutiveFails}: ${reason}`);

        if (this.state === 'init' && this._mon.playlist.consecutiveFails >= this.switching.initFailsToCanvas) {
            this._log('info', '🎨 Kein Video beim Start → Canvas');
            this._switchToCanvas();
        }

        if (this.state === 'video') {
            this._log('warn', `❌ Playlist Probleme (${reason}) → Canvas`);
            this._switchToCanvas();
        }
    }

    _playlistHasNewContent(info) {
        const { mediaSequence, lastSegmentUri } = info;
        const { mediaSequence: playedSeq, segmentUri: playedUri } = this._mon.played;

        if (playedSeq === null && playedUri === null) return true;

        if (mediaSequence !== null && playedSeq !== null) {
            return mediaSequence >= playedSeq + this.switching.minPlaylistAdvance;
        }

        if (lastSegmentUri && playedUri) {
            return lastSegmentUri !== playedUri;
        }

        return true;
    }

    /* ---------------- VIDEO SETUP ---------------- */

    async _switchToVideo() {
        if (this.state === 'video' || !this._canSwitch('video')) return;

        this._log('info', '▶️ VIDEO MODE');
        this._setCooldown();
        this.state = 'video';
        this._mon.switching.lastSwitchAt = Date.now();
        this._mon.playlist.advanceCount = 0;

        this.stageEl.className = 'rfm-stage rfm-stage--video';
        this.tagCloud?.deactivate?.();

        try {
            await this._setupHlsPlayback();
            this._mon.video.lastFrameAt = performance.now();
            this._startFrameTracking();
            this._resetStallTimer();
        } catch (error) {
            this._log('error', `HLS Fehler: ${error.message}`);
            this._mon.video.hlsRetries += 1;
            this._switchToCanvas();
        }
    }

    async _setupHlsPlayback() {
        await this._ensureHls();

        if (!window.Hls?.isSupported()) {
            throw new Error('HLS nicht unterstützt');
        }

        if (this.hls) {
            this.hls.destroy();
            this.hls = null;
        }

        this.hls = new window.Hls({
            lowLatencyMode: true,
            maxBufferLength: 2,
            backBufferLength: 0,
            maxMaxBufferLength: 4,
            liveSyncDurationCount: 1,
            liveMaxLatencyDurationCount: 2,
        });

        this.hls.on(window.Hls.Events.ERROR, (_, data) => {
            if (data.fatal) {
                this._log('error', `HLS fatal: ${data.details}`);
                this._mon.video.hlsRetries += 1;
                this._switchToCanvas();
            }
        });

        this.hls.on(window.Hls.Events.FRAG_CHANGED, (_, data) => {
            if (data?.frag) {
                this._mon.played.mediaSequence = Number.isFinite(data.frag.sn) ? data.frag.sn : this._mon.played.mediaSequence;
                this._mon.played.segmentUri = data.frag.relurl || data.frag.url || this._mon.played.segmentUri;
            }
        });

        this.hls.attachMedia(this.video);
        this.hls.loadSource(this.options.videoUrl);
        this.hls.startLoad();

        await this._waitForVideoStart();
        await this.video.play().catch(error => {
            this._log('warn', `Playback failed: ${error.message}`);
        });
    }

    _waitForVideoStart() {
        return new Promise((resolve, reject) => {
            let settled = false;
            const timeout = setTimeout(() => {
                if (!settled) {
                    settled = true;
                    reject(new Error('Video Start Timeout'));
                }
            }, this.timings.videoStartTimeout);

            const onStarted = () => {
                if (settled) return;
                settled = true;
                clearTimeout(timeout);
                this._mon.video.lastFrameAt = performance.now();
                resolve();
            };

            if ('requestVideoFrameCallback' in this.video) {
                this.video.requestVideoFrameCallback(() => onStarted());
            } else {
                this.video.addEventListener('playing', onStarted, { once: true });
                this.video.addEventListener('loadeddata', onStarted, { once: true });
            }
        });
    }

    async _ensureHls() {
        if (window.Hls) return;

        return new Promise((resolve, reject) => {
            const script = document.createElement('script');
            script.src = this.options.hlsScriptUrl;
            script.onload = resolve;
            script.onerror = () => reject(new Error('HLS.js load failed'));
            document.head.appendChild(script);
        });
    }

    _startFrameTracking() {
        if (!('requestVideoFrameCallback' in this.video)) return;

        const track = (_, metadata) => {
            if (this.state !== 'video') return;
            this._mon.video.lastFrameAt = performance.now();
            this.video.requestVideoFrameCallback(track);
        };

        this.video.requestVideoFrameCallback(track);
    }

    _resetStallTimer() {
        if (this._timers.stall) clearTimeout(this._timers.stall);

        if (this.state !== 'video') return;

        this._timers.stall = setTimeout(() => {
            const lastFrameAt = this._mon.video.lastFrameAt;
            const sinceFrame = lastFrameAt ? performance.now() - lastFrameAt : Infinity;

            if (sinceFrame > this.timings.stallTimeout) {
                this._log('warn', `⏱️ Stall (${sinceFrame.toFixed(0)}ms keine Frames)`);
                this._switchToCanvas();
            } else {
                this._resetStallTimer();
            }
        }, this.timings.stallTimeout);
    }

    /* ---------------- CANVAS SETUP ---------------- */

    _switchToCanvas() {
        if (this.state === 'canvas' || !this._canSwitch('canvas')) return;

        this._log('info', '🎨 CANVAS MODE');
        this._setCooldown();
        this.state = 'canvas';
        this._mon.switching.lastSwitchAt = Date.now();

        this.stageEl.className = 'rfm-stage rfm-stage--canvas';
        this._stopVideo();
        this._startPolling();

        if (this.tagCloud && !this.tagCloud.isActive) {
            this.tagCloud.activate();
        }
    }

    _stopVideo() {
        if (this._timers.stall) {
            clearTimeout(this._timers.stall);
            this._timers.stall = null;
        }

        if (this.hls) {
            this.hls.stopLoad();
            this.hls.detachMedia();
            this.hls.destroy();
            this.hls = null;
        }

        if (this.video) {
            this.video.pause();
        }
        this._mon.video.lastFrameAt = 0;
    }

    _canSwitch(targetState) {
        const now = Date.now();

        if (now < this._mon.switching.cooldownUntil) {
            this._log('debug', `⏳ Switch cooldown (${this._mon.switching.cooldownUntil - now}ms)`);
            return false;
        }

        if (targetState === 'video' && this._mon.video.hlsRetries >= this.switching.maxHlsRetries) {
            this._log('warn', `🛑 Max video retries (${this._mon.video.hlsRetries}) - bleibe bei Canvas`);
            return false;
        }

        return true;
    }

    _setCooldown() {
        this._mon.switching.cooldownUntil = Date.now() + this.timings.switchCooldown;
    }

    /* ---------------- VISUALIZER ---------------- */

    async _waitForVisualizer() {
        return new Promise(resolve => {
            const check = () => {
                if (window.YamnetTagCloudVisualizer) {
                    resolve();
                } else {
                    setTimeout(check, 50);
                }
            };
            check();
        });
    }

    /* ---------------- DOM & UTILS ---------------- */

    _buildDOM() {
        this.rootEl.innerHTML = '';

        const stage = document.createElement('div');
        stage.className = 'rfm-stage rfm-stage--init';

        const visual = document.createElement('div');
        visual.className = 'rfm-stage-visual';

        const video = document.createElement('video');
        video.className = 'rfm-stage-video';
        video.muted = true;
        video.playsInline = true;
        video.autoplay = true;
        video.preload = 'auto';

        const canvas = document.createElement('canvas');
        canvas.className = 'rfm-stage-canvas';

        visual.append(video, canvas);
        stage.appendChild(visual);
        this.rootEl.appendChild(stage);

        if (this.options.audioUrl) {
            const audio = document.createElement('audio');
            audio.className = 'rfm-stage-audio';
            audio.controls = this.audioConfig.controls;
            audio.autoplay = this.audioConfig.autoplay;
            audio.muted = this.audioConfig.muted;
            audio.preload = this.audioConfig.preload;
            audio.src = this.options.audioUrl;
            this.rootEl.appendChild(audio);
            this.audio = audio;
        }

        this.stageEl = stage;
        this.video = video;
        this.canvas = canvas;
        this.ctx = canvas.getContext('2d');

        const resize = () => {
            const rect = visual.getBoundingClientRect();
            const dpr = window.devicePixelRatio || 1;
            canvas.width = rect.width * dpr;
            canvas.height = rect.height * dpr;
            this.ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
            this.tagCloud?.onResize?.();
        };

        resize();
        new ResizeObserver(resize).observe(visual);
    }

    _stopAllTimers() {
        if (this._timers.poll) clearInterval(this._timers.poll);
        if (this._timers.metrics) clearInterval(this._timers.metrics);
        if (this._timers.stall) clearTimeout(this._timers.stall);
        this._timers = {
            poll: null,
            stall: null,
            metrics: null,
        };
    }

    _logMetrics() {
        const metrics = {
            state: this.state,
            playlist: {
                advanceCount: this._mon.playlist.advanceCount,
                fails: this._mon.playlist.consecutiveFails,
            },
            played: this._mon.played,
            video: {
                lastFrame: Math.round(performance.now() - this._mon.video.lastFrameAt),
                retries: this._mon.video.hlsRetries,
            },
        };
        this._log('debug', '📊 METRICS', metrics);
    }

    /* ---------------- PUBLIC API ---------------- */

    getConfig() {
        return {
            timings: this.timings,
            switching: this.switching,
            debug: this.debug,
            state: this.state,
            monitoring: JSON.parse(JSON.stringify(this._mon)),
        };
    }

    updateConfig(newConfig) {
        if (newConfig.timings) {
            Object.assign(this.timings, newConfig.timings);
            this._log('info', '⚙️ Timings updated', this.timings);
        }
        if (newConfig.switching) {
            Object.assign(this.switching, newConfig.switching);
            this._log('info', '⚙️ Switching updated', this.switching);
        }
        if (newConfig.debug) {
            Object.assign(this.debug, newConfig.debug);
            this._log('info', '⚙️ Debug updated', this.debug);
        }

        if (this._timers.poll) {
            this._startPolling();
        }
    }

    forceVideo() {
        this._log('info', '🔄 MANUAL: Force Video');
        this._switchToVideo();
    }

    forceCanvas() {
        this._log('info', '🔄 MANUAL: Force Canvas');
        this._switchToCanvas();
    }
}
