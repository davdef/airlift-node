// public/modules/rfm-stage/rfm-stage.js
import '/visualizer/visualizers/yamnet-tagcloud.js';

export class RfmStage {
    constructor(rootEl, options = {}) {
        if (!rootEl) throw new Error('RfmStage: rootEl fehlt');

        this.rootEl = rootEl;
        this.options = {
            videoUrl: null,
            yamnetEndpoint: null,
            hlsScriptUrl: 'https://cdn.jsdelivr.net/npm/hls.js@1.5.18/dist/hls.min.js',
            
            // ⚡ STRAFFE TIMINGS (alles in ms)
            timings: {
                videoCheckInterval: 1000,      // Jede Sekunde Video prüfen
                videoCheckTimeout: 800,        // HEAD request timeout
                videoStallTimeout: 1200,       // Keine Frames → Wechsel nach 1.2s
                videoRecoverDelay: 500,        // Nach Fehler warten bis retry
                switchCooldown: 300,           // Mindestzeit zwischen Switches
                transitionDuration: 400,       // Fade-Animation
                pollingWhenCanvas: 1500,       // Im Canvas alle 1.5s prüfen
                pollingWhenVideo: 2000,        // Im Video alle 2s prüfen
            },
            
            // 🎯 SWITCHING LOGIK
            switching: {
                consecutiveFailsToSwitch: 2,   // 2 Fehler → Wechsel
                immediateSwitchToVideo: true,  // Video sofort wenn verfügbar
                requireVideoFrames: true,      // Muss Frames bekommen sonst Fehler
                maxVideoRetries: 3,            // Max Versuche bevor Canvas bleibt
                minMediaTimeAdvance: 0.15,     // Mindest-Fortschritt (s) für "neue" Frames
            },
            
            // 🐛 DEBUG
            debug: {
                logLevel: 'info',  // 'error' | 'warn' | 'info' | 'debug'
                showMetrics: false,
                exposeGlobal: true,
            },
            
            ...options
        };

        // 🔧 Normalisiere Timings
        this.timings = { ...this.options.timings };
        this.switching = { ...this.options.switching };
        this.debug = { ...this.options.debug };
        
        if (this.debug.exposeGlobal) {
            window.rfmStage = this;
        }

        this.state = 'init'; // init | video | canvas
        this.video = null;
        this.canvas = null;
        this.ctx = null;
        this.tagCloud = null;
        this.hls = null;

        // 📊 MONITORING
        this._mon = {
            video: {
                lastFrame: 0,
                lastMediaTime: -Infinity,
                lastMediaTimeAtSwitch: -Infinity,
                consecutiveFails: 0,
                consecutiveSuccess: 0,
                totalRetries: 0,
                buffering: false,
                bufferStart: 0,
                lastCheck: 0,
                score: 0,
            },
            viz: {
                lastData: 0,
                dataRate: 0,
                score: 100, // Start mit hohem Score
            },
            switching: {
                lastSwitch: 0,
                switchCount: 0,
                cooldownUntil: 0,
            }
        };
        
        this._timers = {};
        
        this._injectStyles();
        this._log('info', '🚀 RFM Stage initialisiert', this.timings);
    }

    /* ---------------- LOGGING ---------------- */
    
    _log(level, message, data = null) {
        const levels = { error: 0, warn: 1, info: 2, debug: 3 };
        const currentLevel = levels[this.debug.logLevel] || 1;
        const messageLevel = levels[level] || 1;
        
        if (messageLevel > currentLevel) return;
        
        const ts = performance.now().toFixed(1);
        const prefix = `[${ts}ms]`;
        
        const colors = {
            info: '#4ECDC4',
            warn: '#FFD166', 
            error: '#FF6B6B',
            debug: '#607D8B'
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
                aspect-ratio: 16/9;
                background: #000;
                overflow: hidden;
            }
            .rfm-stage-visual {
                position: absolute;
                inset: 0;
            }
            .rfm-stage-visual video,
            .rfm-stage-visual canvas {
                position: absolute;
                inset: 0;
                width: 100%;
                height: 100%;
                object-fit: cover;
                transition: opacity ${this.timings.transitionDuration}ms ease;
            }
            .rfm-stage--video canvas {
                opacity: 0;
                pointer-events: none;
            }
            .rfm-stage--canvas video {
                opacity: 0;
                pointer-events: none;
            }
            .rfm-stage--init video,
            .rfm-stage--init canvas {
                opacity: 0;
            }
        `;
        document.head.appendChild(style);
    }

    /* ---------------- MAIN FLOW ---------------- */

    async mount() {
        this._log('info', '📦 MOUNT');
        this._buildDOM();
        
        // Visualizer vorbereiten
        await this._waitForVisualizer();
        if (window.YamnetTagCloudVisualizer) {
            this.tagCloud = new window.YamnetTagCloudVisualizer(this.ctx, this.canvas);
            if (this.options.yamnetEndpoint) {
                this.tagCloud.streamEndpoint = this.options.yamnetEndpoint;
            }
            this.tagCloud.theme = 'light';
        }
        
        // Starte Monitoring
        this._startMonitoring();
        
        // Initialer Check
        this._checkVideoAvailability();
    }

    destroy() {
        this._log('info', '🧨 DESTROY');
        this._clearAllTimers();
        this.tagCloud?.deactivate?.();
        
        if (this.hls) {
            this.hls.destroy();
            this.hls = null;
        }
        
        if (this.video) {
            this.video.pause();
            this.video.src = '';
        }
        
        this.rootEl.innerHTML = '';
    }

    /* ---------------- MONITORING ---------------- */

    _startMonitoring() {
        this._clearAllTimers();
        
        // Video Frame Monitoring
        if ('requestVideoFrameCallback' in this.video) {
            const trackFrames = (_, metadata) => {
                const now = performance.now();
                this._mon.video.lastFrame = now;
                if (typeof metadata?.mediaTime === 'number') {
                    this._mon.video.lastMediaTime = metadata.mediaTime;
                } else if (typeof this.video?.currentTime === 'number') {
                    this._mon.video.lastMediaTime = this.video.currentTime;
                }
                
                // Buffering detection
                if (this.video.readyState < 3) {
                    if (!this._mon.video.buffering) {
                        this._mon.video.buffering = true;
                        this._mon.video.bufferStart = now;
                        this._log('debug', '📦 Video buffering start');
                    }
                } else if (this._mon.video.buffering) {
                    this._mon.video.buffering = false;
                    const duration = now - this._mon.video.bufferStart;
                    this._log('debug', `📦 Video buffering end (${duration.toFixed(0)}ms)`);
                }
                
                if (this.state === 'video') {
                    this.video.requestVideoFrameCallback(trackFrames);
                }
            };
            this.video.requestVideoFrameCallback(trackFrames);
        }
        
        // Video Availability Polling
        this._timers.videoPoll = setInterval(() => {
            this._checkVideoAvailability();
        }, this.state === 'video' ? this.timings.pollingWhenVideo : this.timings.pollingWhenCanvas);
        
        // Stall Detection (nur im Video-Modus)
        this._resetStallTimer();
        
        // Debug Metrics
        if (this.debug.showMetrics) {
            this._timers.metrics = setInterval(() => this._logMetrics(), 1000);
        }
    }
    
    _checkVideoAvailability() {
        if (!this.options.videoUrl || Date.now() < this._mon.switching.cooldownUntil) {
            return;
        }
        
        this._mon.video.lastCheck = Date.now();
        
        fetch(this.options.videoUrl, {
            method: 'HEAD',
            cache: 'no-cache',
            signal: AbortSignal.timeout(this.timings.videoCheckTimeout)
        })
        .then(response => {
            const available = response.ok;
            const latency = Date.now() - this._mon.video.lastCheck;
            
            if (available) {
                this._onVideoAvailable(latency);
            } else {
                this._onVideoUnavailable(response.status);
            }
        })
        .catch(error => {
            this._onVideoUnavailable(error.name);
        });
    }
    
    _onVideoAvailable(latency) {
        this._mon.video.consecutiveFails = 0;
        this._mon.video.consecutiveSuccess++;
        this._mon.video.score = Math.max(0, 100 - latency / 10);
        
        // 💡 VIDEO VERFÜGBAR - Entscheidung
        if (this.state !== 'video') {
            if (this.switching.immediateSwitchToVideo) {
                this._log('info', `🎬 Video verfügbar (${latency}ms) → SOFORT`);
                this._switchToVideo();
            } else if (this._mon.video.consecutiveSuccess >= 2) {
                this._log('info', `🎬 Video stabil (${latency}ms, ${this._mon.video.consecutiveSuccess}x) → WECHSEL`);
                this._switchToVideo();
            }
        } else {
            // Im Video-Modus: Bestätige Verfügbarkeit
            this._resetStallTimer();
        }
    }
    
    _onVideoUnavailable(reason) {
        this._mon.video.consecutiveFails++;
        this._mon.video.consecutiveSuccess = 0;
        this._mon.video.score = 0;
        // ⚠️ totalRetries wird NUR bei tatsächlichen HLS-Fehlern erhöht, nicht hier!
        
        this._log('debug', `📡 Video fail #${this._mon.video.consecutiveFails}: ${reason}`);
        
        // 💡 VIDEO NICHT VERFÜGBAR - Entscheidung
        if (this.state === 'video') {
            if (this._mon.video.consecutiveFails >= this.switching.consecutiveFailsToSwitch) {
                this._log('warn', `❌ Video down (${reason}) → CANVAS`);
                this._switchToCanvas();
            }
        } else if (this.state === 'init' && this._mon.video.consecutiveFails >= 2) {
            this._log('info', '🎨 Kein Video beim Start → CANVAS');
            this._switchToCanvas();
        }
    }
    
    _resetStallTimer() {
        if (this._timers.stall) clearTimeout(this._timers.stall);
        
        if (this.state === 'video') {
            this._timers.stall = setTimeout(() => {
                const timeSinceFrame = performance.now() - this._mon.video.lastFrame;
                if (timeSinceFrame > this.timings.videoStallTimeout) {
                    this._log('warn', `⏱️ Stall (${timeSinceFrame.toFixed(0)}ms keine Frames)`);
                    this._switchToCanvas();
                }
            }, this.timings.videoStallTimeout);
        }
    }

    /* ---------------- VIDEO SETUP ---------------- */

    async _switchToVideo() {
        if (this.state === 'video' || !this._canSwitch()) return;
        
        this._log('info', '▶️ VIDEO MODE');
        this._setCooldown();
        
        this.state = 'video';
        this._mon.switching.lastSwitch = Date.now();
        this._mon.switching.switchCount++;
        
        this.stageEl.className = 'rfm-stage rfm-stage--video';
        this._startMonitoring();
        
        // Visualizer stoppen
        this.tagCloud?.deactivate?.();
        
        // HLS starten (asynchron)
        setTimeout(async () => {
            try {
                await this._setupHls();
                this._mon.video.lastFrame = performance.now();
                this._resetStallTimer();
                this._log('debug', 'HLS gestartet');
            } catch (error) {
                this._log('error', `HLS Fehler: ${error.message}`);
                this._switchToCanvas();
            }
        }, 10);
    }
    
    async _setupHls() {
        if (this.hls) {
            this.hls.destroy();
            this.hls = null;
        }
        
        await this._ensureHls();
        
        if (!window.Hls?.isSupported()) {
            throw new Error('HLS nicht unterstützt');
        }
        
        return new Promise((resolve, reject) => {
            let resolved = false;
            const timeout = setTimeout(() => {
                if (!resolved) {
                    reject(new Error('HLS Timeout'));
                }
            }, 5000);
            
            this.hls = new window.Hls({
                lowLatencyMode: true,
                maxBufferLength: 2,        // 🔥 KURZ!
                backBufferLength: 0,
                maxMaxBufferLength: 4,
                liveSyncDurationCount: 1,
                liveMaxLatencyDurationCount: 2,
            });
            
            this.hls.attachMedia(this.video);
            
            // Erfolg wenn erste Frames da sind (und tatsächlich "neu" sind)
            if ('requestVideoFrameCallback' in this.video) {
                const lastMediaTimeAtSwitch = this._mon.video.lastMediaTimeAtSwitch;
                const minAdvance = this.switching.minMediaTimeAdvance ?? 0;
                const waitForFreshFrame = (_, metadata) => {
                    const mediaTime = typeof metadata?.mediaTime === 'number'
                        ? metadata.mediaTime
                        : this.video.currentTime;
                    const isFresh = !this.switching.requireVideoFrames
                        || mediaTime > lastMediaTimeAtSwitch + minAdvance;
                    if (isFresh && !resolved) {
                        resolved = true;
                        clearTimeout(timeout);
                        resolve();
                        return;
                    }
                    if (!resolved) {
                        this.video.requestVideoFrameCallback(waitForFreshFrame);
                    }
                };
                this.video.requestVideoFrameCallback(waitForFreshFrame);
            } else {
                this.video.addEventListener('loadeddata', () => {
                    if (!resolved) {
                        resolved = true;
                        clearTimeout(timeout);
                        resolve();
                    }
                }, { once: true });
            }
            
            // Fehler-Handling
            this.hls.on(window.Hls.Events.ERROR, (_, data) => {
                if (data.fatal && !resolved) {
                    resolved = true;
                    clearTimeout(timeout);
                    reject(new Error(`HLS fatal: ${data.details}`));
                }
            });
            
            this.hls.loadSource(this.options.videoUrl);
            this.hls.startLoad();
            
        }).then(() => {
            return this.video.play().catch(err => {
                this._log('warn', `Playback failed: ${err.message}`);
            });
        }).catch(error => {
            // 🔴 NUR HIER totalRetries erhöhen!
            this._mon.video.totalRetries++;
            throw error;
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

    /* ---------------- CANVAS SETUP ---------------- */

    async _switchToCanvas() {
        if (this.state === 'canvas' || !this._canSwitch()) return;
        
        this._log('info', '🎨 CANVAS MODE');
        this._setCooldown();
        
        this.state = 'canvas';
        this._mon.switching.lastSwitch = Date.now();
        this._mon.switching.switchCount++;
        this._mon.video.lastMediaTimeAtSwitch = this._mon.video.lastMediaTime;
        
        this.stageEl.className = 'rfm-stage rfm-stage--canvas';
        
        // HLS stoppen
        if (this.hls) {
            this.hls.stopLoad();
            this.hls.detachMedia();
        }
        if (this.video) {
            this.video.pause();
        }
        this._startMonitoring();
        
        // Visualizer starten
        setTimeout(() => {
            if (this.tagCloud && !this.tagCloud.isActive) {
                this.tagCloud.activate();
            }
        }, 10);
        
        // Retry Counter zurücksetzen nach Wechsel zu Canvas
        this._mon.video.totalRetries = 0;
    }
    
    _canSwitch() {
        const now = Date.now();
        const sinceLastSwitch = now - this._mon.switching.lastSwitch;
        const inCooldown = now < this._mon.switching.cooldownUntil;
        
        if (inCooldown) {
            this._log('debug', `⏳ Switch cooldown (${this._mon.switching.cooldownUntil - now}ms)`);
            return false;
        }
        
        if (sinceLastSwitch < this.timings.switchCooldown) {
            this._log('debug', `⚡ Zu schneller Switch (${sinceLastSwitch}ms)`);
            return false;
        }
        
        // 🚨 KORREKTUR: Max retries NUR prüfen wenn wir ZU VIDEO wechseln wollen
        // Nicht wenn wir von Video ZU Canvas wechseln wollen!
        const targetState = this.state === 'video' ? 'canvas' : 'video';
        
        if (targetState === 'video' && this._mon.video.totalRetries >= this.switching.maxVideoRetries) {
            this._log('warn', `🛑 Max video retries (${this._mon.video.totalRetries}) - bleibe bei Canvas`);
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
        video.muted = true;
        video.playsInline = true;
        video.autoplay = true;
        video.preload = 'auto';
        
        const canvas = document.createElement('canvas');
        
        visual.append(video, canvas);
        stage.appendChild(visual);
        this.rootEl.appendChild(stage);
        
        this.stageEl = stage;
        this.visualEl = visual;
        this.video = video;
        this.canvas = canvas;
        this.ctx = canvas.getContext('2d');
        
        // Resize
        const resize = () => {
            const rect = this.visualEl.getBoundingClientRect();
            const dpr = window.devicePixelRatio || 1;
            this.canvas.width = rect.width * dpr;
            this.canvas.height = rect.height * dpr;
            this.ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
            this.tagCloud?.onResize?.();
        };
        
        resize();
        new ResizeObserver(resize).observe(this.visualEl);
    }
    
    _clearAllTimers() {
        Object.values(this._timers).forEach(timer => {
            if (timer) clearInterval(timer);
        });
        this._timers = {};
    }
    
    _logMetrics() {
        const metrics = {
            state: this.state,
            video: {
                score: this._mon.video.score,
                fails: this._mon.video.consecutiveFails,
                success: this._mon.video.consecutiveSuccess,
                retries: this._mon.video.totalRetries,
                lastFrame: Math.round(performance.now() - this._mon.video.lastFrame),
                buffering: this._mon.video.buffering,
            },
            switching: {
                count: this._mon.switching.switchCount,
                last: Date.now() - this._mon.switching.lastSwitch,
                cooldown: this._mon.switching.cooldownUntil - Date.now(),
            }
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
            monitoring: JSON.parse(JSON.stringify(this._mon))
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
        
        // Restart monitoring with new timings
        if (this._timers.videoPoll) {
            this._startMonitoring();
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
