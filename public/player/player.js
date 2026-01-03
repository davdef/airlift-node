/**
 * AircheckPlayer - Optimierte Version
 * Modular, performant, mobiltauglich
 */

// ============================================================================
//  KONFIGURATION
// ============================================================================
const CONFIG = {
    // Viewport
    MIN_VISIBLE_DURATION: 5_000,           // 5 Sekunden
    MAX_VISIBLE_DURATION: 7 * 24 * 60 * 60_000, // 7 Tage
    DEFAULT_VISIBLE_DURATION: 30_000,      // 30 Sekunden
    
    // History
    HISTORY_BUFFER_FACTOR: 2.0,           // Wie viel mehr History als sichtbar laden
    MIN_HISTORY_SPAN: 1_000,              // Mindestspanne für History-Request
    HISTORY_RATE_LIMIT: 150,              // ms zwischen History-Requests
    
    // Rendering
    WAVEFORM_RESOLUTION_FACTOR: 2.0,      // Max Punkte pro Pixel
    SMOOTHING_THRESHOLD: 300_000,         // >5 Minuten = Glättung
    SMOOTHING_WINDOW: 0.01,               // 1% der Punkte als Fenster
    
    // WebSocket
    WS_RECONNECT_BASE_DELAY: 1_000,
    WS_RECONNECT_MAX_DELAY: 30_000,
    WS_HEARTBEAT_INTERVAL: 30_000,
    
    // Audio
    AUDIO_LOAD_TIMEOUT: 5_000,
    AUDIO_RETRY_COUNT: 3,
    
    // Timeline
    TICK_STEPS: [
        { duration: 30_000, step: 1_000, format: 'ss' },      // <30s: Sekunden
        { duration: 120_000, step: 5_000, format: 'mm:ss' },  // <2min
        { duration: 300_000, step: 10_000, format: 'mm:ss' }, // <5min
        { duration: 900_000, step: 30_000, format: 'HH:mm' }, // <15min
        { duration: 3_600_000, step: 60_000, format: 'HH:mm' }, // <1h
        { duration: 18_000_000, step: 300_000, format: 'HH:mm' }, // <5h
        { duration: Infinity, step: 3_600_000, format: 'HH:mm' }  // >=5h
    ],
    
    // Farben
    COLORS: {
        background: '#111',
        waveform: '#5aa0ff',
        waveformFill: 'rgba(90, 160, 255, 0.25)',
        timeline: '#888',
        grid: '#333',
        playheadLive: '#ff4d4d',
        playheadTimeshift: '#ffd92c',
        bufferRange: 'rgba(255, 255, 255, 0.1)',
        error: '#ff6b6b',
        success: '#4ecdc4'
    }
};

// ============================================================================
//  UTILITY FUNCTIONS
// ============================================================================
class TimeUtils {
    static formatTime(ts, format = 'HH:mm:ss') {
        if (!Number.isFinite(ts)) return '--:--:--';
        const d = new Date(ts);
        
        switch(format) {
            case 'ss': return d.getSeconds().toString().padStart(2, '0');
            case 'mm:ss': 
                return `${d.getMinutes().toString().padStart(2, '0')}:${d.getSeconds().toString().padStart(2, '0')}`;
            case 'HH:mm':
                return `${d.getHours().toString().padStart(2, '0')}:${d.getMinutes().toString().padStart(2, '0')}`;
            case 'HH:mm:ss':
            default:
                return `${d.getHours().toString().padStart(2, '0')}:${d.getMinutes().toString().padStart(2, '0')}:${d.getSeconds().toString().padStart(2, '0')}`;
        }
    }
    
    static getTickConfig(duration) {
        return CONFIG.TICK_STEPS.find(step => duration <= step.duration) || CONFIG.TICK_STEPS[CONFIG.TICK_STEPS.length - 1];
    }
    
    static calculateOptimalTickSpacing(canvasWidth, viewportDuration) {
        // Adaptive Tick-Spacing basierend auf Canvas-Breite und Zeitspanne
        const targetTicks = Math.max(3, Math.min(10, canvasWidth / 100)); // 3-10 Ticks
        
        // Versuche passenden Step aus Config zu finden
        const config = TimeUtils.getTickConfig(viewportDuration);
        const stepMs = config.step;
        const expectedTicks = viewportDuration / stepMs;
        
        // Wenn zu viele Ticks, größeren Step nehmen
        if (expectedTicks > targetTicks * 1.5) {
            let adjustedStep = stepMs;
            while (viewportDuration / adjustedStep > targetTicks * 1.5 && adjustedStep < viewportDuration / 2) {
                adjustedStep *= 2;
            }
            return { step: adjustedStep, format: config.format };
        }
        
        // Wenn zu wenige Ticks, kleineren Step nehmen
        if (expectedTicks < targetTicks * 0.5 && stepMs > 1000) {
            let adjustedStep = stepMs;
            while (viewportDuration / adjustedStep < targetTicks * 0.5 && adjustedStep > 1000) {
                adjustedStep /= 2;
            }
            return { step: adjustedStep, format: config.format };
        }
        
        return { step: stepMs, format: config.format };
    }
}

// ============================================================================
//  WEBSOCKET MANAGER
// ============================================================================
class WebSocketManager {
    constructor(onMessage, onStatus) {
        this.onMessage = onMessage;
        this.onStatus = onStatus;
        this.ws = null;
        this.reconnectAttempts = 0;
        this.reconnectTimer = null;
        this.heartbeatTimer = null;
        this.lastMessageTime = null;
        this.isConnected = false;
    }
    
    connect() {
        if (this.reconnectTimer) {
            clearTimeout(this.reconnectTimer);
            this.reconnectTimer = null;
        }
        
        const proto = location.protocol === 'https:' ? 'wss://' : 'ws://';
        const wsUrl = proto + window.location.host + '/ws';
        
        if (this.ws) {
            this.ws.onopen = null;
            this.ws.onerror = null;
            this.ws.onclose = null;
            this.ws.onmessage = null;
            try { this.ws.close(); } catch {}
        }
        
        this.ws = new WebSocket(wsUrl);
        
        this.ws.onopen = () => {
            console.log('[WS] Connected');
            this.isConnected = true;
            this.reconnectAttempts = 0;
            this.startHeartbeat();
            this.onStatus('connected', 'WebSocket verbunden');
        };
        
        this.ws.onerror = (err) => {
            console.error('[WS] Error:', err);
            this.onStatus('error', 'WebSocket-Fehler');
        };
        
        this.ws.onclose = () => {
            console.warn('[WS] Closed');
            this.isConnected = false;
            this.stopHeartbeat();
            this.onStatus('disconnected', 'WebSocket getrennt');
            this.scheduleReconnect();
        };
        
        this.ws.onmessage = (e) => {
            this.lastMessageTime = Date.now();
            try {
                const data = JSON.parse(e.data);
                if (data && Array.isArray(data.peaks) && typeof data.timestamp === 'number') {
                    this.onMessage(data);
                }
            } catch (err) {
                console.warn('[WS] Parse error:', err);
            }
        };
    }
    
    startHeartbeat() {
        if (this.heartbeatTimer) clearInterval(this.heartbeatTimer);
        this.heartbeatTimer = setInterval(() => {
            if (this.isConnected && this.ws.readyState === WebSocket.OPEN) {
                // Send ping if supported, otherwise just check connection
                if (Date.now() - this.lastMessageTime > CONFIG.WS_HEARTBEAT_INTERVAL * 2) {
                    console.warn('[WS] No messages received, reconnecting...');
                    this.ws.close();
                }
            }
        }, CONFIG.WS_HEARTBEAT_INTERVAL);
    }
    
    stopHeartbeat() {
        if (this.heartbeatTimer) {
            clearInterval(this.heartbeatTimer);
            this.heartbeatTimer = null;
        }
    }
    
    scheduleReconnect() {
        if (this.reconnectTimer) return;
        
        const delay = Math.min(
            CONFIG.WS_RECONNECT_BASE_DELAY * Math.pow(2, this.reconnectAttempts),
            CONFIG.WS_RECONNECT_MAX_DELAY
        );
        
        this.reconnectAttempts++;
        console.warn(`[WS] Reconnecting in ${delay}ms (attempt ${this.reconnectAttempts})`);
        
        this.reconnectTimer = setTimeout(() => {
            this.reconnectTimer = null;
            this.connect();
        }, delay);
    }
    
    disconnect() {
        this.stopHeartbeat();
        if (this.reconnectTimer) {
            clearTimeout(this.reconnectTimer);
            this.reconnectTimer = null;
        }
        if (this.ws) {
            this.ws.close();
        }
    }
}

function normalizeEventTimestamp(timestamp) {
    if (!Number.isFinite(timestamp)) {
        return null;
    }
    if (timestamp > 1e15) {
        return Math.round(timestamp / 1_000_000);
    }
    if (timestamp > 1e12) {
        return Math.round(timestamp / 1_000);
    }
    return timestamp;
}

// ============================================================================
//  HISTORY MANAGER
// ============================================================================
class HistoryManager {
    constructor(onHistoryUpdate) {
        this.history = [];
        this.onHistoryUpdate = onHistoryUpdate;
        this.loading = false;
        this.pendingRequest = null;
        this.lastRequestTime = 0;
        this.cache = new Map(); // Cache für bereits geladene Bereiche
    }
    
    async loadWindow(from, to) {
        // Validierung
        const span = to - from;
        if (span <= 0) {
            console.log('[History] Invalid span:', span);
            return;
        }
        
        // Mindestspanne
        const actualTo = Math.max(to, from + CONFIG.MIN_HISTORY_SPAN);
        
        // Rate Limiting
        const now = performance.now();
        if (now - this.lastRequestTime < CONFIG.HISTORY_RATE_LIMIT) {
            if (!this.pendingRequest) {
                this.pendingRequest = { from, to: actualTo };
            } else {
                // Merge mit pending request
                this.pendingRequest.from = Math.min(this.pendingRequest.from, from);
                this.pendingRequest.to = Math.max(this.pendingRequest.to, actualTo);
            }
            return;
        }
        
        // Bereits gecached?
        const cacheKey = `${Math.floor(from)}-${Math.floor(to)}`;
        if (this.cache.has(cacheKey)) {
            this.mergeData(this.cache.get(cacheKey));
            return;
        }
        
        try {
            this.loading = true;
            this.lastRequestTime = now;
            
            const url = `/api/history?from=${Math.floor(from)}&to=${Math.floor(actualTo)}`;
            console.log('[History] Loading:', url);
            
            const response = await fetch(url);
            if (!response.ok) throw new Error(`HTTP ${response.status}`);
            
            const data = await response.json();
            if (!Array.isArray(data)) throw new Error('Invalid response format');
            
            // Konvertierung und Caching
            const converted = data.map(point => ({
                ts: point.ts,
                peaks: [point.peak_l, point.peak_r],
                amp: (point.peak_l + point.peak_r) / 2,
                silence: point.silence || false
            }));
            
            this.cache.set(cacheKey, converted);
            this.mergeData(converted);
            console.log(`[History] Loaded ${converted.length} points, total: ${this.history.length}`);
            
        } catch (error) {
            console.error('[History] Load error:', error);
            throw error;
        } finally {
            this.loading = false;
            
            // Pending Request verarbeiten
            if (this.pendingRequest) {
                const pending = this.pendingRequest;
                this.pendingRequest = null;
                setTimeout(() => this.loadWindow(pending.from, pending.to), 50);
            }
        }
    }
    
    mergeData(newData) {
        // Effizienter Merge mit Map
        const map = new Map(this.history.map(item => [item.ts, item]));
        newData.forEach(item => map.set(item.ts, item));
        
        this.history = Array.from(map.values()).sort((a, b) => a.ts - b.ts);
        this.onHistoryUpdate(this.history);
    }
    
    trimHistory(viewportLeft, viewportRight, visibleDuration) {
        if (!this.history.length) return;
        
        const buffer = visibleDuration * CONFIG.HISTORY_BUFFER_FACTOR;
        const min = viewportLeft - buffer;
        const max = viewportRight + buffer;
        
        const firstVisible = this.history.findIndex(p => p.ts >= min);
        const lastVisible = this.history.findIndex(p => p.ts > max);
        
        if (firstVisible > 0 || lastVisible < this.history.length) {
            this.history = this.history.slice(
                Math.max(0, firstVisible),
                lastVisible === -1 ? this.history.length : lastVisible
            );
        }
    }
    
    getVisiblePoints(viewportLeft, viewportRight, canvasWidth) {
        if (!this.history.length) return [];
        
        // Binäre Suche für effiziente Filterung
        let start = 0;
        let end = this.history.length - 1;
        
        while (start <= end) {
            const mid = Math.floor((start + end) / 2);
            if (this.history[mid].ts < viewportLeft) {
                start = mid + 1;
            } else {
                end = mid - 1;
            }
        }
        
        const firstIndex = start;
        
        start = 0;
        end = this.history.length - 1;
        while (start <= end) {
            const mid = Math.floor((start + end) / 2);
            if (this.history[mid].ts <= viewportRight) {
                start = mid + 1;
            } else {
                end = mid - 1;
            }
        }
        
        const lastIndex = end;
        
        if (firstIndex > lastIndex) return [];
        
        let visible = this.history.slice(firstIndex, lastIndex + 1);
        
        // Downsampling für Performance
        const maxPoints = canvasWidth * CONFIG.WAVEFORM_RESOLUTION_FACTOR;
        if (visible.length > maxPoints) {
            const step = visible.length / maxPoints;
            const downsampled = [];
            for (let i = 0; i < maxPoints; i++) {
                const idx = Math.floor(i * step);
                if (idx < visible.length) {
                    downsampled.push(visible[idx]);
                }
            }
            visible = downsampled;
        }
        
        // Glättung für große Zeiträume
        if ((viewportRight - viewportLeft) > CONFIG.SMOOTHING_THRESHOLD && visible.length > 10) {
            const windowSize = Math.max(1, Math.floor(visible.length * CONFIG.SMOOTHING_WINDOW));
            const smoothed = [];
            
            for (let i = 0; i < visible.length; i++) {
                const start = Math.max(0, i - windowSize);
                const end = Math.min(visible.length - 1, i + windowSize);
                
                let sum = 0;
                let count = 0;
                
                for (let j = start; j <= end; j++) {
                    sum += visible[j].amp || 
                           (Array.isArray(visible[j].peaks) ? 
                            visible[j].peaks.reduce((a, b) => a + b, 0) / visible[j].peaks.length : 0);
                    count++;
                }
                
                smoothed.push({
                    ...visible[i],
                    smoothedAmp: sum / count
                });
            }
            visible = smoothed;
        }
        
        return visible;
    }
    
    clear() {
        this.history = [];
        this.cache.clear();
    }
}

// ============================================================================
//  VIEWPORT CONTROLLER
// ============================================================================
class ViewportController {
    constructor(bufferStart, bufferEnd) {
        this.bufferStart = bufferStart;
        this.bufferEnd = bufferEnd;
        
        this.left = 0;
        this.right = 0;
        this.duration = CONFIG.DEFAULT_VISIBLE_DURATION;
        this.followLive = true;
        
        // Pinch-Zoom State
        this.pinchStart = null;
        this.pinchStartDuration = null;
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
    
    zoom(factor, centerX, canvasWidth) {
        this.followLive = false;
        
        const centerTime = this.left + (centerX / canvasWidth) * this.duration;
        const newDuration = this.duration * factor;
        
        // Limits
        const bufferSpan = this.bufferEnd - this.bufferStart;
        const maxDuration = Math.min(CONFIG.MAX_VISIBLE_DURATION, bufferSpan || CONFIG.MAX_VISIBLE_DURATION);
        const clampedDuration = Math.max(CONFIG.MIN_VISIBLE_DURATION, Math.min(maxDuration, newDuration));
        
        this.duration = clampedDuration;
        this.left = centerTime - (centerX / canvasWidth) * this.duration;
        this.right = this.left + this.duration;
        
        this.clampToBuffer();
    }
    
    pan(deltaPixels, canvasWidth) {
        if (this.duration <= 0) return;
        
        const msPerPixel = this.duration / canvasWidth;
        const deltaMs = deltaPixels * msPerPixel;
        
        this.left -= deltaMs;
        this.right -= deltaMs;
        
        this.clampToBuffer();
    }
    
    clampToBuffer() {
        if (this.bufferStart == null || this.bufferEnd == null) return;
        
        // Viewport komplett außerhalb?
        if (this.left > this.bufferEnd || this.right < this.bufferStart) {
            // Zurück zum Live-Bereich
            this.right = this.bufferEnd;
            this.left = this.right - this.duration;
        }
        
        // Nach links begrenzen
        if (this.left < this.bufferStart) {
            this.left = this.bufferStart;
            this.right = this.left + this.duration;
        }
        
        // Nach rechts begrenzen
        if (this.right > this.bufferEnd) {
            this.right = this.bufferEnd;
            this.left = this.right - this.duration;
            
            // Falls zu klein wird (nahe Buffer-Ende)
            if (this.left < this.bufferStart) {
                this.left = this.bufferStart;
                this.duration = this.right - this.left;
            }
        }
        
        // Sicherstellen, dass duration konsistent bleibt
        this.duration = this.right - this.left;
    }
    
    startPinch(distance) {
        this.pinchStart = distance;
        this.pinchStartDuration = this.duration;
    }
    
    updatePinch(distance) {
        if (!this.pinchStart || !this.pinchStartDuration) return;
        
        const scale = this.pinchStart / distance; // Umgekehrt für intuitives Zoom
        const newDuration = this.pinchStartDuration * scale;
        
        // Limits anwenden
        const bufferSpan = this.bufferEnd - this.bufferStart;
        const maxDuration = Math.min(CONFIG.MAX_VISIBLE_DURATION, bufferSpan || CONFIG.MAX_VISIBLE_DURATION);
        const clampedDuration = Math.max(CONFIG.MIN_VISIBLE_DURATION, Math.min(maxDuration, newDuration));
        
        // Viewport um Mittelpunkt zoomen
        const center = (this.left + this.right) / 2;
        this.duration = clampedDuration;
        this.left = center - this.duration / 2;
        this.right = center + this.duration / 2;
        
        this.clampToBuffer();
    }
    
    endPinch() {
        this.pinchStart = null;
        this.pinchStartDuration = null;
    }
    
    get visibleRange() {
        return { left: this.left, right: this.right, duration: this.duration };
    }
}

// ============================================================================
//  AUDIO CONTROLLER
// ============================================================================
class AudioController {
    constructor(onStateChange, onError) {
        this.audio = new Audio();
        this.audio.crossOrigin = 'anonymous';
        this.audio.preload = 'none';
        
        this.isLive = true;
        this.playbackStartTime = null;
        this.currentRetry = 0;
        this.loadTimeout = null;
        this.onStateChange = onStateChange;
        this.onError = onError;
        
        this.setupEvents();
    }
    
    setupEvents() {
        const events = ['play', 'pause', 'playing', 'waiting', 'ended', 'error', 'loadedmetadata', 'canplay'];
        events.forEach(event => {
            this.audio.addEventListener(event, (e) => {
                console.log(`[Audio] ${event}:`, e.type, this.audio.readyState, this.audio.error);
                this.handleEvent(event);
            });
        });
    }
    
    handleEvent(event) {
        switch(event) {
            case 'play':
                this.onStateChange('playing');
                break;
            case 'pause':
                this.onStateChange('paused');
                break;
            case 'playing':
                this.currentRetry = 0;
                this.onStateChange('playing');
                break;
            case 'waiting':
                this.onStateChange('buffering');
                break;
            case 'error':
                this.handleAudioError();
                break;
            case 'canplay':
                this.onStateChange('ready');
                break;
        }
    }
    
    handleAudioError() {
        if (this.loadTimeout) {
            clearTimeout(this.loadTimeout);
            this.loadTimeout = null;
        }
        
        const error = this.audio.error;
        let message = 'Audio-Fehler';
        let details = {};
        
        if (error) {
            switch(error.code) {
                case MediaError.MEDIA_ERR_ABORTED:
                    message = 'Abgebrochen';
                    details = { code: error.code, userAction: true };
                    break;
                case MediaError.MEDIA_ERR_NETWORK:
                    message = 'Netzwerk-Fehler';
                    details = { code: error.code, retry: this.currentRetry < CONFIG.AUDIO_RETRY_COUNT };
                    break;
                case MediaError.MEDIA_ERR_DECODE:
                    message = 'Dekodierungsfehler';
                    details = { code: error.code, fatal: true };
                    break;
                case MediaError.MEDIA_ERR_SRC_NOT_SUPPORTED:
                    message = 'Format nicht unterstützt';
                    details = { code: error.code, fatal: true };
                    break;
                default:
                    message = `Unbekannter Fehler (${error.code})`;
                    details = { code: error.code };
            }
        }
        
        console.error('[Audio] Error:', message, details);
        this.onError(message, details);
        
        // Automatischer Retry für Netzwerkfehler
        if (error && error.code === MediaError.MEDIA_ERR_NETWORK && 
            this.currentRetry < CONFIG.AUDIO_RETRY_COUNT) {
            this.currentRetry++;
            console.log(`[Audio] Retry ${this.currentRetry}/${CONFIG.AUDIO_RETRY_COUNT}`);
            setTimeout(() => this.reload(), 1000 * this.currentRetry);
        }
    }
    
    playLive() {
        this.isLive = true;
        this.playbackStartTime = null;
        this.currentRetry = 0;
        
        const src = `/audio/live?_=${Date.now()}`;
        this.loadSource(src, true);
    }
    
    playTimeshift(serverTime) {
        this.isLive = false;
        this.playbackStartTime = serverTime;
        this.currentRetry = 0;
        
        const src = `/audio/at?ts=${Math.floor(serverTime)}&_=${Date.now()}`;
        this.loadSource(src, false);
    }
    
    loadSource(src, autoplay = true) {
        if (this.loadTimeout) {
            clearTimeout(this.loadTimeout);
        }
        
        this.audio.pause();
        this.audio.src = src;
        
        if (autoplay) {
            this.loadTimeout = setTimeout(() => {
                if (this.audio.readyState < 2) { // Noch keine Metadaten
                    this.onError('Audio-Ladezeit überschritten', { timeout: true });
                    this.loadTimeout = null;
                }
            }, CONFIG.AUDIO_LOAD_TIMEOUT);
            
            this.audio.load();
            this.audio.play().catch(err => {
                console.warn('[Audio] Autoplay prevented:', err);
                this.onStateChange('paused');
            });
        } else {
            this.audio.load();
        }
    }
    
    reload() {
        if (this.audio.src) {
            const currentSrc = this.audio.src;
            this.audio.src = '';
            this.audio.src = currentSrc + (currentSrc.includes('?') ? '&' : '?') + '_retry=' + Date.now();
            this.audio.load();
            this.audio.play().catch(console.warn);
        }
    }
    
    pause() {
        this.audio.pause();
    }
    
    getCurrentTime(referenceTime) {
        if (this.isLive) {
            return referenceTime;
        }
        
        if (this.playbackStartTime != null && !this.audio.paused) {
            return this.playbackStartTime + (this.audio.currentTime * 1000);
        }
        
        return this.playbackStartTime || referenceTime;
    }
    
    getState() {
        return {
            isLive: this.isLive,
            paused: this.audio.paused,
            currentTime: this.audio.currentTime,
            duration: this.audio.duration,
            readyState: this.audio.readyState,
            error: this.audio.error
        };
    }
}

// ============================================================================
//  RENDERER
// ============================================================================
class Renderer {
    constructor(canvas) {
        this.canvas = canvas;
        this.ctx = canvas.getContext('2d');
        this.pixelRatio = window.devicePixelRatio || 1;
        
        this.resizeObserver = new ResizeObserver(() => this.resize());
        this.resizeObserver.observe(canvas);
        
        // Performance-Optimierung
        this.lastRenderTime = 0;
        this.renderInterval = 1000 / 60; // 60 FPS
        this.cachedTimeline = null;
        this.cacheValid = false;
    }
    
    resize() {
        const rect = this.canvas.getBoundingClientRect();
        this.canvas.width = rect.width * this.pixelRatio;
        this.canvas.height = rect.height * this.pixelRatio;
        this.ctx.scale(this.pixelRatio, this.pixelRatio);
        this.cacheValid = false;
    }
    
    render(viewport, history, audioState, bufferRange) {
        const now = performance.now();
        if (now - this.lastRenderTime < this.renderInterval) {
            return;
        }
        this.lastRenderTime = now;
        
        const w = this.canvas.width / this.pixelRatio;
        const h = this.canvas.height / this.pixelRatio;
        
        // Clear
        this.ctx.fillStyle = CONFIG.COLORS.background;
        this.ctx.fillRect(0, 0, w, h);
        
        // Buffer-Bereich visualisieren
        this.drawBufferRange(w, h, viewport, bufferRange);
        
        // Timeline (mit Caching)
        this.drawTimeline(w, h, viewport);
        
        // Waveform
        this.drawWaveform(w, h, viewport, history);
        
        // Playhead
        this.drawPlayhead(w, h, viewport, audioState);
        
        // Status-Overlay
        this.drawStatusOverlay(w, h, audioState);
    }
    
    drawBufferRange(w, h, viewport, bufferRange) {
        if (!bufferRange || bufferRange.start >= bufferRange.end) return;
        
        const { left, right, duration } = viewport.visibleRange;
        const pxPerMs = w / duration;
        
        const bufferLeft = Math.max(left, bufferRange.start);
        const bufferRight = Math.min(right, bufferRange.end);
        
        if (bufferRight <= bufferLeft) return;
        
        const x1 = ((bufferLeft - left) / duration) * w;
        const x2 = ((bufferRight - left) / duration) * w;
        const width = x2 - x1;
        
        this.ctx.fillStyle = CONFIG.COLORS.bufferRange;
        this.ctx.fillRect(x1, 40, width, h - 40);
        
        // Buffer-Kanten
        this.ctx.strokeStyle = 'rgba(255, 255, 255, 0.3)';
        this.ctx.lineWidth = 1;
        this.ctx.beginPath();
        this.ctx.moveTo(x1, 40);
        this.ctx.lineTo(x1, h);
        this.ctx.moveTo(x2, 40);
        this.ctx.lineTo(x2, h);
        this.ctx.stroke();
    }
    
    drawTimeline(w, h, viewport) {
        const { left, right, duration } = viewport.visibleRange;
        
        // Cache invalidation prüfen
        if (!this.cachedTimeline || !this.cacheValid || 
            this.cachedTimeline.left !== left || 
            this.cachedTimeline.right !== right ||
            this.cachedTimeline.width !== w) {
            
            this.cachedTimeline = {
                left, right, width: w,
                ticks: this.calculateTicks(w, left, right, duration)
            };
            this.cacheValid = true;
        }
        
        const { ticks } = this.cachedTimeline;
        
        // Gitterlinien
        this.ctx.strokeStyle = CONFIG.COLORS.grid;
        this.ctx.lineWidth = 1;
        this.ctx.beginPath();
        
        ticks.forEach(tick => {
            this.ctx.moveTo(tick.x, 40);
            this.ctx.lineTo(tick.x, h);
        });
        this.ctx.stroke();
        
        // Beschriftungen (mit Überlappungsprüfung)
        this.ctx.font = '11px sans-serif';
        this.ctx.fillStyle = CONFIG.COLORS.timeline;
        this.ctx.textBaseline = 'top';
        
        let lastX = -Infinity;
        const minSpacing = 40; // Mindestabstand zwischen Beschriftungen in Pixeln
        
        ticks.forEach(tick => {
            if (tick.x - lastX >= minSpacing) {
                this.ctx.fillText(tick.label, tick.x + 2, 42);
                lastX = tick.x;
            }
        });
        
        // Viewport-Zeitraum oben links
        this.ctx.fillStyle = '#fff';
        this.ctx.fillText(
            `${TimeUtils.formatTime(left)} – ${TimeUtils.formatTime(right)}`,
            10, 10
        );
    }
    
    calculateTicks(w, left, right, duration) {
        const tickConfig = TimeUtils.calculateOptimalTickSpacing(w, duration);
        const step = tickConfig.step;
        const format = tickConfig.format;
        
        const firstTick = Math.ceil(left / step) * step;
        const ticks = [];
        const pxPerMs = w / duration;
        
        for (let time = firstTick; time <= right; time += step) {
            const x = ((time - left) / duration) * w;
            ticks.push({
                x: Math.round(x),
                time: time,
                label: TimeUtils.formatTime(time, format)
            });
        }
        
        return ticks;
    }
    
    drawWaveform(w, h, viewport, history) {
        const points = history.getVisiblePoints(viewport.left, viewport.right, w);
        if (points.length < 2) return;
        
        const { left, duration } = viewport.visibleRange;
        const pxPerMs = w / duration;
        const midY = h / 2;
        const maxHeight = h * 0.45;
        
        this.ctx.beginPath();
        this.ctx.fillStyle = CONFIG.COLORS.waveformFill;
        this.ctx.strokeStyle = CONFIG.COLORS.waveform;
        this.ctx.lineWidth = 1;
        
        // Obere Linie
        for (let i = 0; i < points.length; i++) {
            const point = points[i];
            const x = (point.ts - left) * pxPerMs;
            const amp = point.smoothedAmp !== undefined ? point.smoothedAmp : 
                       point.amp || (Array.isArray(point.peaks) ? 
                       point.peaks.reduce((a, b) => a + b, 0) / point.peaks.length : 0);
            
            const height = Math.max(1, Math.min(1, amp)) * maxHeight;
            const y = midY - height;
            
            if (i === 0) {
                this.ctx.moveTo(x, y);
            } else {
                this.ctx.lineTo(x, y);
            }
        }
        
        // Untere Linie (rückwärts)
        for (let i = points.length - 1; i >= 0; i--) {
            const point = points[i];
            const x = (point.ts - left) * pxPerMs;
            const amp = point.smoothedAmp !== undefined ? point.smoothedAmp :
                       point.amp || (Array.isArray(point.peaks) ? 
                       point.peaks.reduce((a, b) => a + b, 0) / point.peaks.length : 0);
            
            const height = Math.max(1, Math.min(1, amp)) * maxHeight;
            const y = midY + height;
            
            this.ctx.lineTo(x, y);
        }
        
        this.ctx.closePath();
        this.ctx.fill();
        this.ctx.stroke();
    }
    
    drawPlayhead(w, h, viewport, audioState) {
        const currentTime = audioState.currentTime;
        const { left, duration } = viewport.visibleRange;
        
        const x = ((currentTime - left) / duration) * w;
        if (x < 0 || x > w) return;
        
        const color = audioState.isLive ? CONFIG.COLORS.playheadLive : CONFIG.COLORS.playheadTimeshift;
        
        // Linie
        this.ctx.strokeStyle = color;
        this.ctx.lineWidth = 2;
        this.ctx.beginPath();
        this.ctx.moveTo(x, 35);
        this.ctx.lineTo(x, h);
        this.ctx.stroke();
        
        // Dreieck oben
        this.ctx.fillStyle = color;
        this.ctx.beginPath();
        this.ctx.moveTo(x, 30);
        this.ctx.lineTo(x - 6, 20);
        this.ctx.lineTo(x + 6, 20);
        this.ctx.closePath();
        this.ctx.fill();
    }
    
    drawStatusOverlay(w, h, audioState) {
        this.ctx.font = '12px sans-serif';
        this.ctx.fillStyle = audioState.isLive ? CONFIG.COLORS.playheadLive : CONFIG.COLORS.playheadTimeshift;
        this.ctx.textBaseline = 'bottom';
        
        const mode = audioState.isLive ? 'LIVE' : 'TIMESHIFT';
        const time = TimeUtils.formatTime(audioState.currentTime);
        
        this.ctx.fillText(`▶ ${time} [${mode}]`, 10, h - 10);
        
        // Buffer-Indicator
        if (audioState.buffering) {
            this.ctx.fillStyle = CONFIG.COLORS.error;
            this.ctx.fillText('Buffering...', w - 80, h - 10);
        }
    }
}

// ============================================================================
//  UI MANAGER
// ============================================================================
class UIManager {
    constructor(player) {
        this.player = player;
        this.elements = {};
        this.init();
    }
    
    init() {
        const idleOverlay = this.createIdleOverlay();
        this.elements = {
            canvas: document.getElementById('waveform'),
            liveBtn: document.getElementById('liveBtn'),
            playBtn: document.getElementById('playBtn'),
            status: document.getElementById('status'),
            debugPanel: document.querySelector('.debug-panel'),
            errorOverlay: this.createErrorOverlay(),
            idleOverlay: idleOverlay.overlay,
            idleTitle: idleOverlay.title,
            idleSubtitle: idleOverlay.subtitle
        };
        
        this.attachEvents();
    }
    
    createErrorOverlay() {
        const overlay = document.createElement('div');
        overlay.className = 'error-overlay';
        overlay.style.cssText = `
            position: fixed;
            top: 10px;
            right: 10px;
            background: ${CONFIG.COLORS.error};
            color: white;
            padding: 10px 15px;
            border-radius: 5px;
            display: none;
            z-index: 1000;
            max-width: 300px;
            box-shadow: 0 2px 10px rgba(0,0,0,0.3);
        `;
        document.body.appendChild(overlay);
        return overlay;
    }

    createIdleOverlay() {
        const overlay = document.createElement('div');
        overlay.className = 'idle-overlay';
        overlay.style.cssText = `
            position: fixed;
            inset: 0;
            background: rgba(0, 0, 0, 0.75);
            display: none;
            align-items: center;
            justify-content: center;
            z-index: 900;
            text-align: center;
            color: #fff;
            padding: 24px;
        `;

        const content = document.createElement('div');
        content.style.cssText = `
            display: flex;
            flex-direction: column;
            align-items: center;
            gap: 12px;
            max-width: 420px;
        `;

        const logo = document.createElement('img');
        logo.src = 'rfm-logo.png';
        logo.alt = 'RFM Logo';
        logo.style.cssText = 'width: 96px; opacity: 0.9;';

        const title = document.createElement('div');
        title.style.cssText = 'font-size: 18px; font-weight: 600;';
        title.textContent = 'Kein Kontakt zur API';

        const subtitle = document.createElement('div');
        subtitle.style.cssText = 'font-size: 14px; color: #cbd5e1;';
        subtitle.textContent = 'Bitte API starten und Pipeline prüfen.';

        content.appendChild(logo);
        content.appendChild(title);
        content.appendChild(subtitle);
        overlay.appendChild(content);
        document.body.appendChild(overlay);

        return { overlay, title, subtitle };
    }
    
    attachEvents() {
        if (this.elements.liveBtn) {
            this.elements.liveBtn.addEventListener('click', () => this.player.switchToLive());
        }
        
        if (this.elements.playBtn) {
            this.elements.playBtn.addEventListener('click', () => this.player.togglePlayback());
        }
        
        // Touch/Mouse Events werden direkt am Canvas behandelt
    }
    
    showError(message, details = {}) {
        this.elements.errorOverlay.textContent = message;
        this.elements.errorOverlay.style.display = 'block';
        
        if (details.fatal) {
            this.elements.errorOverlay.style.background = CONFIG.COLORS.error;
        } else if (details.retry) {
            this.elements.errorOverlay.style.background = '#ffa726';
        }
        
        setTimeout(() => {
            this.elements.errorOverlay.style.display = 'none';
        }, details.fatal ? 10000 : 5000);
    }
    
    updateStatus(text, type = 'info') {
        if (!this.elements.status) return;
        
        this.elements.status.textContent = text;
        this.elements.status.className = `status status-${type}`;
    }

    setIdleState({ active, title, subtitle } = {}) {
        if (!this.elements.idleOverlay) {
            return;
        }
        this.elements.idleOverlay.style.display = active ? 'flex' : 'none';
        if (title && this.elements.idleTitle) {
            this.elements.idleTitle.textContent = title;
        }
        if (subtitle && this.elements.idleSubtitle) {
            this.elements.idleSubtitle.textContent = subtitle;
        }
    }
    
    toggleDebug(show) {
        if (this.elements.debugPanel) {
            this.elements.debugPanel.style.display = show ? 'block' : 'none';
        }
    }
}

// ============================================================================
//  MAIN PLAYER CLASS
// ============================================================================
class AircheckPlayer {
    constructor() {
        this.canvas = document.getElementById('waveform');
        if (!this.canvas) {
            console.error('Canvas #waveform nicht gefunden');
            return;
        }
        
        // Module initialisieren
        this.ui = new UIManager(this);
        this.viewport = new ViewportController(null, null);
        this.history = new HistoryManager(() => this.cacheValid = false);
        this.audio = new AudioController(
            (state) => this.onAudioStateChange(state),
            (error, details) => this.ui.showError(error, details)
        );
        this.renderer = new Renderer(this.canvas);
        
        // State
        this.serverTime = null;
        this.lastWsUpdate = null;
        this.bufferInfo = { start: null, end: null };
        this.cacheValid = false;
        
        // Performance
        this.animationFrame = null;
        this.lastRenderTime = 0;
        
        // Initialisierung
        this.initWebSocket();
        this.input = new PlayerInputHandler(this, this.canvas);
        this.input.attach();
        this.fetchPipelineState();
        this.fetchBufferInfo();
        this.startRenderLoop();
        
        console.log('🎵 Aircheck Player gestartet (optimierte Version)');
        this.ui.updateStatus('Player initialisiert', 'success');
    }
    
    async fetchBufferInfo() {
        try {
            const response = await fetch('/api/peaks');
            const data = await response.json();
            
            if (data?.ok) {
                this.bufferInfo = { start: data.start, end: data.end };
                this.viewport.updateBuffer(data.start, data.end);
                
                // Initial viewport auf Live-Bereich
                this.viewport.setLive(data.end);
                
                // Initiale History laden
                await this.history.loadWindow(
                    this.viewport.left,
                    this.viewport.right
                );
                
                this.ui.setIdleState({ active: false });
                this.ui.updateStatus(`Buffer geladen (${this.formatDuration(data.end - data.start)})`, 'success');
                await this.fetchPipelineState();
            } else {
                throw new Error('Buffer-API liefert keine Daten');
            }
        } catch (error) {
            console.error('Buffer-Info Fehler:', error);
            this.ui.setIdleState({
                active: true,
                title: 'Kein Kontakt zur API',
                subtitle: 'Bitte API starten und Netzwerk prüfen.'
            });
            this.ui.updateStatus('Kein Kontakt zur API', 'error');
        }
    }

    async fetchPipelineState() {
        try {
            const response = await fetch('/api/status');
            if (!response.ok) {
                throw new Error('Status API nicht erreichbar');
            }
            const status = await response.json();
            const hasPipeline = (status?.flows || []).length > 0 || (status?.producers || []).length > 0;
            if (!hasPipeline) {
                this.ui.setIdleState({
                    active: true,
                    title: 'Keine Pipeline vorhanden',
                    subtitle: 'Bitte config.toml anlegen oder Pipeline konfigurieren.'
                });
                this.ui.updateStatus('Keine Pipeline vorhanden', 'warning');
                return;
            }
            this.ui.setIdleState({ active: false });
        } catch (error) {
            this.ui.setIdleState({
                active: true,
                title: 'Kein Kontakt zur API',
                subtitle: 'Bitte API starten und Netzwerk prüfen.'
            });
            this.ui.updateStatus('Kein Kontakt zur API', 'error');
        }
    }
    
    initWebSocket() {
        this.wsManager = new WebSocketManager(
            (data) => this.handleWsMessage(data),
            (state, message) => this.ui.updateStatus(message, state === 'error' ? 'error' : 'info')
        );
        this.wsManager.connect();
    }
    
    handleWsMessage(data) {
        const eventTimestamp = normalizeEventTimestamp(data?.timestamp);
        if (!eventTimestamp) {
            return;
        }
        this.serverTime = eventTimestamp;
        this.lastWsUpdate = performance.now();
        
        // Buffer-Ende aktualisieren
        if (eventTimestamp > (this.bufferInfo.end || 0)) {
            this.bufferInfo.end = eventTimestamp;
            this.viewport.updateBuffer(this.bufferInfo.start, this.bufferInfo.end);
        }
        
        // History-Eintrag hinzufügen
        const entry = {
            ts: eventTimestamp,
            peaks: [data.peaks[0], data.peaks[1]],
            amp: (data.peaks[0] + data.peaks[1]) / 2,
            silence: !!data.silence
        };
        
        this.history.mergeData([entry]);
        
        // Viewport aktualisieren wenn Live-Modus
        if (this.viewport.followLive) {
            this.viewport.setLive(eventTimestamp);
        }
    }
    
    seekTo(serverTime) {
        // Prüfen ob Zeit im Buffer liegt
        if (serverTime < this.bufferInfo.start || serverTime > this.bufferInfo.end) {
            this.ui.showError('Zeitpunkt nicht verfügbar', { fatal: false });
            return;
        }
        
        this.audio.playTimeshift(serverTime);
        this.viewport.followLive = false;
        
        // Viewport um Seek-Position zentrieren
        const center = serverTime;
        this.viewport.left = center - this.viewport.duration / 2;
        this.viewport.right = center + this.viewport.duration / 2;
        this.viewport.clampToBuffer();
        
        this.cacheValid = false;
    }
    
    seekRelative(ms) {
        const currentTime = this.audio.getCurrentTime(this.getServerNow());
        this.seekTo(currentTime + ms);
    }
    
    switchToLive() {
        this.audio.playLive();
        this.viewport.followLive = true;
        this.viewport.setLive(this.getServerNow());
        this.cacheValid = false;
    }
    
    togglePlayback() {
        const state = this.audio.getState();
        if (state.paused) {
            if (!state.isLive && this.audio.playbackStartTime === null) {
                this.switchToLive();
            } else {
                this.audio.audio.play().catch(console.error);
            }
        } else {
            this.audio.pause();
        }
    }
    
    onAudioStateChange(state) {
        // UI-Updates basierend auf Audio-State
        switch(state) {
            case 'playing':
                this.ui.updateStatus('Wiedergabe', 'success');
                break;
            case 'paused':
                this.ui.updateStatus('Pausiert', 'info');
                break;
            case 'buffering':
                this.ui.updateStatus('Buffering...', 'warning');
                break;
            case 'ready':
                this.ui.updateStatus('Bereit', 'success');
                break;
        }
    }
    
    getServerNow() {
        if (this.serverTime && this.lastWsUpdate) {
            return this.serverTime + (performance.now() - this.lastWsUpdate);
        }
        return this.bufferInfo.end || Date.now();
    }
    
    formatDuration(ms) {
        if (ms < 60000) return `${(ms / 1000).toFixed(0)}s`;
        if (ms < 3600000) return `${(ms / 60000).toFixed(1)}min`;
        if (ms < 86400000) return `${(ms / 3600000).toFixed(1)}h`;
        return `${(ms / 86400000).toFixed(1)}d`;
    }
    
    startRenderLoop() {
        const render = () => {
            // History nachladen wenn nötig
            if (!this.history.loading) {
                const { left, right } = this.viewport.visibleRange;
                const buffer = this.viewport.duration * CONFIG.HISTORY_BUFFER_FACTOR;
                
                const historyStart = this.history.history[0]?.ts || Infinity;
                const historyEnd = this.history.history[this.history.history.length - 1]?.ts || -Infinity;
                
                if (left - buffer < historyStart) {
                    this.history.loadWindow(left - buffer, historyStart);
                }
                
                if (right + buffer > historyEnd) {
                    this.history.loadWindow(historyEnd, right + buffer);
                }
            }
            
            // Trimmen
            this.history.trimHistory(
                this.viewport.left,
                this.viewport.right,
                this.viewport.duration
            );
            
            // Rendern
            const audioState = {
                currentTime: this.audio.getCurrentTime(this.getServerNow()),
                isLive: this.audio.isLive,
                buffering: this.audio.audio.readyState < 3
            };
            
            this.renderer.render(
                this.viewport,
                this.history,
                audioState,
                this.bufferInfo
            );
            
            this.animationFrame = requestAnimationFrame(render);
        };
        
        this.animationFrame = requestAnimationFrame(render);
    }
    
    destroy() {
        if (this.animationFrame) {
            cancelAnimationFrame(this.animationFrame);
        }

        this.wsManager?.disconnect();
        this.history?.clear();
        this.renderer?.resizeObserver?.disconnect();
        this.input?.detach();
        
        // Event-Listener entfernen
        // (müsste für jedes Modul implementiert werden)
    }
}

// ============================================================================
//  STYLES (als CSS-Inline, besser in separate Datei)
// ============================================================================
const styles = `
/* Mobile-first Responsive Design */
:root {
    --vh: 1vh;
    --color-primary: #5aa0ff;
    --color-danger: #ff4d4d;
    --color-warning: #ffd92c;
}

.player-container {
    width: 100%;
    height: calc(var(--vh, 1vh) * 100);
    display: flex;
    flex-direction: column;
    background: #111;
    touch-action: none;
    user-select: none;
}

.waveform-container {
    flex: 1;
    position: relative;
    overflow: hidden;
}

#waveform {
    width: 100%;
    height: 100%;
    display: block;
}

.controls {
    padding: 10px;
    background: rgba(0, 0, 0, 0.8);
    display: flex;
    gap: 10px;
    align-items: center;
}

.controls button {
    padding: 8px 16px;
    border: none;
    border-radius: 4px;
    background: var(--color-primary);
    color: white;
    font-weight: bold;
    cursor: pointer;
    transition: opacity 0.2s;
}

.controls button:hover {
    opacity: 0.9;
}

.controls button:active {
    transform: scale(0.98);
}

#liveBtn {
    background: var(--color-danger);
}

.status {
    margin-left: auto;
    padding: 5px 10px;
    background: rgba(255, 255, 255, 0.1);
    border-radius: 3px;
    font-size: 12px;
    color: #fff;
}

.status-error { background: rgba(255, 77, 77, 0.3); }
.status-warning { background: rgba(255, 217, 44, 0.3); }
.status-success { background: rgba(78, 205, 196, 0.3); }

/* Mobile Optimierungen */
@media (max-width: 768px) {
    .controls {
        padding: 8px;
    }
    
    .controls button {
        padding: 10px;
        flex: 1;
        font-size: 14px;
    }
    
    .status {
        display: none; /* Auf mobilen Geräten Status im Overlay anzeigen */
    }
}

/* Debug Panel (nur im Entwicklungsmodus) */
.debug-panel {
    position: fixed;
    bottom: 0;
    left: 0;
    right: 0;
    background: rgba(0, 0, 0, 0.9);
    color: #fff;
    padding: 10px;
    font-size: 11px;
    font-family: monospace;
    display: none;
}

.debug-row {
    display: flex;
    justify-content: space-between;
    margin-bottom: 2px;
}

.debug-label {
    color: #888;
}

/* Touch-friendly Vergrößerung */
@media (hover: none) and (pointer: coarse) {
    .controls button {
        min-height: 44px; /* Apple Human Interface Guidelines */
        min-width: 44px;
    }
    
    #waveform {
        cursor: pointer;
    }
}
`;

// Styles injecten
const styleElement = document.createElement('style');
styleElement.textContent = styles;
document.head.appendChild(styleElement);

// ============================================================================
//  INITIALISIERUNG
// ============================================================================
window.addEventListener('load', () => {
    // Viewport Height für Mobile
    const setVH = () => {
        const vh = window.visualViewport?.height || window.innerHeight;
        document.documentElement.style.setProperty('--vh', `${vh * 0.01}px`);
    };
    
    setVH();
    window.addEventListener('resize', setVH);
    if (window.visualViewport) {
        window.visualViewport.addEventListener('resize', setVH);
    }
    
    // Player starten
    window.player = new AircheckPlayer();
});
