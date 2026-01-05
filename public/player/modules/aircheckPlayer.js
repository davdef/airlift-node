import { CONFIG } from './config.js';
import { TimeUtils } from './timeUtils.js';
import { WebSocketManager } from './webSocketManager.js';
import { HistoryManager } from './historyManager.js';
import { ViewportController } from './viewportController.js';
import { AudioController } from './audioController.js';
import { Renderer } from './renderer.js';
import { UIManager } from './uiManager.js';
import { PlayerInputHandler } from './playerInputHandler.js';

export class AircheckPlayer {
    constructor() {
        this.canvas = document.getElementById('waveform');
        if (!this.canvas) {
            console.error('Canvas #waveform nicht gefunden');
            return;
        }
        
        this.ui = new UIManager(this);
        this.viewport = new ViewportController(null, null);
        this.history = new HistoryManager(() => this.cacheValid = false);
        this.audio = new AudioController(
            (state) => this.onAudioStateChange(state),
            (error, details) => this.ui.showError(error, details)
        );
        this.renderer = new Renderer(this.canvas);
        
        this.serverTime = null;
        this.lastWsUpdate = null;
        this.bufferInfo = { start: null, end: null };
        this.cacheValid = false;
        this.selectedSource = null;
        this.sourceOptions = [];
        
        this.animationFrame = null;
        this.lastRenderTime = 0;
        this.lastHistoryCheck = 0;
        this.historyCheckInterval = 1000;
        this.isInitialized = false;
        this.offlineMode = false;
        this.initTimeout = null;
        this.initializationError = null;
        
        this.input = new PlayerInputHandler(this, this.canvas);
        this.input.attach();

        this.sourceSelect = document.getElementById('sourceSelect');
        if (this.sourceSelect) {
            this.sourceSelect.addEventListener('change', (event) => {
                const next = event.target.value || null;
                this.setActiveSource(next, { persist: true });
            });
        }
        
        console.log('🎵 Aircheck Player gestartet');
        this.ui.updateStatus('Initialisiere...', 'info');
        
        this.initializeAsync();
    }
    
    async initializeAsync() {
        try {
            this.initTimeout = setTimeout(() => {
                throw new Error('Server nicht erreichbar');
            }, 10000);
            
            await Promise.race([
                this.initializeCore(),
                new Promise((_, reject) => {
                    setTimeout(() => reject(new Error('Timeout')), 10000);
                })
            ]);
            
            clearTimeout(this.initTimeout);
            this.isInitialized = true;
            this.ui.setIdleState({ active: false });
            this.ui.updateStatus('Bereit', 'success');
            this.startRenderLoop();
            
        } catch (error) {
            if (this.initTimeout) clearTimeout(this.initTimeout);
            this.initializationError = error;
            this.enterOfflineMode(error);
        }
    }
    
    async initializeCore() {
        await this.fetchPipelineState();
        await this.fetchBufferInfo();
        await this.initWebSocket();
        return true;
    }
    
    enterOfflineMode(error) {
        this.offlineMode = true;
        this.history.setOfflineMode(true);
        
        const now = Date.now();
        this.bufferInfo = { start: now - 30000, end: now };
        this.viewport.updateBuffer(this.bufferInfo.start, this.bufferInfo.end);
        this.viewport.setLive(this.bufferInfo.end);
        
        let title = 'Kein Kontakt zur API';
        let subtitle = 'Bitte API starten und Netzwerk prüfen.';
        
        if (error.message.includes('502')) {
            title = 'API nicht verfügbar';
            subtitle = 'Der Backend-Service antwortet nicht (HTTP 502).';
        }
        
        this.ui.setIdleState({
            active: true,
            title: title,
            subtitle: subtitle
        });
        
        this.ui.updateStatus('Offline', 'warning');
    }
    
    async fetchBufferInfo() {
        try {
            const flowParam = this.selectedSource ? `?flow=${encodeURIComponent(this.selectedSource)}` : '';
            const response = await fetch(`/api/peaks${flowParam}`);
            if (!response.ok) throw new Error(`HTTP ${response.status}`);
            
            const data = await response.json();
            if (data?.ok) {
                this.bufferInfo = { start: data.start, end: data.end };
                this.viewport.updateBuffer(data.start, data.end);
                this.viewport.setLive(data.end);
                
                if (!this.offlineMode) {
                    try {
                        await this.history.loadWindow(this.viewport.left, this.viewport.right);
                    } catch (e) {}
                }
                
                this.ui.updateStatus(`Buffer geladen`, 'success');
                return true;
            }
            throw new Error('Keine Buffer-Daten');
        } catch (error) {
            console.error('Buffer-Info Fehler:', error.message);
            throw error;
        }
    }
    
    async fetchPipelineState() {
        try {
            const response = await fetch('/api/status');
            if (!response.ok) throw new Error(`HTTP ${response.status}`);
            
            const status = await response.json();
            const hasPipeline = (status?.flows || []).length > 0 || (status?.producers || []).length > 0;
            this.updateSourceOptions(status);
            if (!hasPipeline) this.ui.updateStatus('Keine Pipeline', 'warning');
            return hasPipeline;
        } catch (error) {
            console.warn('Pipeline check failed:', error.message);
            throw error;
        }
    }
    
    initWebSocket() {
        this.wsManager = new WebSocketManager(
            (data) => this.handleWsMessage(data),
            (state, message) => {
                if (state === 'connected' && this.offlineMode) {
                    this.offlineMode = false;
                    this.history.setOfflineMode(false);
                    this.ui.setIdleState({ active: false });
                    this.ui.updateStatus('Verbunden', 'success');
                    this.startRenderLoop();
                }
                this.ui.updateStatus(message, state === 'error' ? 'error' : 'info');
            }
        );
        this.wsManager.connect();
    }

    handleWsMessage(data) {
        const eventTimestamp = this.normalizeEventTimestamp(data?.timestamp);
        if (!eventTimestamp) return;
        if (this.selectedSource && data?.flow !== this.selectedSource) return;
        if (!Array.isArray(data?.peaks) || data.peaks.length === 0) return;
        
        this.serverTime = eventTimestamp;
        this.lastWsUpdate = performance.now();
        
        if (eventTimestamp > (this.bufferInfo.end || 0)) {
            this.bufferInfo.end = eventTimestamp;
            this.viewport.updateBuffer(this.bufferInfo.start, this.bufferInfo.end);
        }
        
        const entry = {
            ts: eventTimestamp,
            peaks: [data.peaks[0], data.peaks[1]],
            amp: (data.peaks[0] + data.peaks[1]) / 2,
            silence: !!data.silence
        };
        
        this.history.mergeData([entry]);
        
        if (this.viewport.followLive) {
            this.viewport.setLive(eventTimestamp);
        }
    }
    
    normalizeEventTimestamp(timestamp) {
        if (!Number.isFinite(timestamp)) return null;
        if (timestamp > 1e15) return Math.round(timestamp / 1_000_000);
        if (timestamp > 1e12) return Math.round(timestamp / 1_000);
        return timestamp;
    }

    updateSourceOptions(status) {
        const flowNames = (status?.flows || []).map((flow) => flow.name);
        const producerNames = (status?.producers || []).map((producer) => producer.name);
        const unique = Array.from(new Set([...flowNames, ...producerNames]));
        this.sourceOptions = unique;

        if (!this.sourceSelect) return;

        this.sourceSelect.innerHTML = '';
        const allOption = document.createElement('option');
        allOption.value = '';
        allOption.textContent = 'Alle Quellen';
        this.sourceSelect.appendChild(allOption);

        unique.forEach((name) => {
            const option = document.createElement('option');
            option.value = name;
            option.textContent = name;
            this.sourceSelect.appendChild(option);
        });

        const stored = window.localStorage.getItem('aircheck.source');
        const preferred = stored && unique.includes(stored) ? stored : null;
        const next = preferred || (unique[0] ?? null);
        this.setActiveSource(next, { persist: false, updateSelect: true });
    }

    async setActiveSource(source, { persist = true, updateSelect = false } = {}) {
        if (this.selectedSource === source) return;
        this.selectedSource = source;
        this.history.setFlow(source);
        this.cacheValid = false;

        if (this.sourceSelect && updateSelect) {
            this.sourceSelect.value = source || '';
        }

        if (persist) {
            if (source) {
                window.localStorage.setItem('aircheck.source', source);
            } else {
                window.localStorage.removeItem('aircheck.source');
            }
        }

        if (!this.offlineMode) {
            await this.fetchBufferInfo();
        }
    }
    
    startRenderLoop() {
        const render = () => {
            if (this.offlineMode) return;
            
            const now = performance.now();
            
            if (!this.history.loading && (now - this.lastHistoryCheck > this.historyCheckInterval)) {
                this.lastHistoryCheck = now;
                
                const { left, right } = this.viewport.visibleRange;
                const buffer = this.viewport.duration * CONFIG.HISTORY_BUFFER_FACTOR;
                
                const historyStart = this.history.history[0]?.ts || Infinity;
                const historyEnd = this.history.history[this.history.history.length - 1]?.ts || -Infinity;
                
                if (historyStart !== Infinity && left - buffer < historyStart) {
                    this.history.loadWindow(left - buffer, historyStart).catch(() => {});
                }
                
                if (historyEnd !== -Infinity && right + buffer > historyEnd) {
                    this.history.loadWindow(historyEnd, right + buffer).catch(() => {});
                }
            }
            
            this.history.trimHistory(this.viewport.left, this.viewport.right, this.viewport.duration);
            
            const audioState = {
                currentTime: this.audio.getCurrentTime(this.getServerNow()),
                isLive: this.audio.isLive,
                buffering: this.audio.audio.readyState < 3
            };
            
            this.renderer.render(this.viewport, this.history, audioState, this.bufferInfo);
            
            this.animationFrame = requestAnimationFrame(render);
        };
        
        if (!this.offlineMode) {
            this.animationFrame = requestAnimationFrame(render);
        }
    }
    
    seekTo(serverTime) {
        if (serverTime < this.bufferInfo.start || serverTime > this.bufferInfo.end) {
            this.ui.showError('Zeitpunkt nicht verfügbar', { fatal: false });
            return;
        }
        
        this.audio.playTimeshift(serverTime);
        this.viewport.followLive = false;
        
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
            if (!state.isLive && this.audio.playbackStartTime === null) this.switchToLive();
            else this.audio.audio.play().catch(console.error);
        } else {
            this.audio.pause();
        }
    }
    
    onAudioStateChange(state) {
        switch(state) {
            case 'playing': this.ui.updateStatus('Wiedergabe', 'success'); break;
            case 'paused': this.ui.updateStatus('Pausiert', 'info'); break;
            case 'buffering': this.ui.updateStatus('Buffering...', 'warning'); break;
            case 'ready': this.ui.updateStatus('Bereit', 'success'); break;
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
    
    destroy() {
        if (this.animationFrame) cancelAnimationFrame(this.animationFrame);
        if (this.initTimeout) clearTimeout(this.initTimeout);
        this.wsManager?.disconnect();
        this.history?.clear();
        this.renderer?.resizeObserver?.disconnect();
        this.input?.detach();
    }
}
