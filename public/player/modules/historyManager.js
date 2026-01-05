import { CONFIG } from './config.js';

export class HistoryManager {
    constructor(onHistoryUpdate) {
        this.history = [];
        this.onHistoryUpdate = onHistoryUpdate;
        this.loading = false;
        this.pendingRequest = null;
        this.lastRequestTime = 0;
        this.cache = new Map();
        
        // NEUE PROPERTIES FÜR RETRY-LOGIK
        this.retryCount = 0;
        this.isOffline = false;
        this.lastErrorTime = 0;
    }
    
    async loadWindow(from, to) {
        // Wenn offline, keine Versuche unternehmen
        if (this.isOffline) {
            console.log('[History] Skipping load - offline mode');
            return;
        }
        
        const span = to - from;
        if (span <= 0) {
            console.log('[History] Invalid span:', span);
            return;
        }
        
        const actualTo = Math.max(to, from + CONFIG.MIN_HISTORY_SPAN);
        
        // Rate Limiting
        const now = performance.now();
        if (now - this.lastRequestTime < CONFIG.HISTORY_RATE_LIMIT) {
            if (!this.pendingRequest) {
                this.pendingRequest = { from, to: actualTo };
            } else {
                this.pendingRequest.from = Math.min(this.pendingRequest.from, from);
                this.pendingRequest.to = Math.max(this.pendingRequest.to, actualTo);
            }
            return;
        }
        
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
            if (!response.ok) {
                if (response.status === 502 || response.status >= 500) {
                    throw new Error(`HTTP ${response.status} - Server error`);
                }
                throw new Error(`HTTP ${response.status}`);
            }
            
            const data = await response.json();
            if (!Array.isArray(data)) throw new Error('Invalid response format');
            
            const converted = data.map(point => ({
                ts: point.ts,
                peaks: [point.peak_l, point.peak_r],
                amp: (point.peak_l + point.peak_r) / 2,
                silence: point.silence || false
            }));
            
            this.cache.set(cacheKey, converted);
            this.mergeData(converted);
            this.retryCount = 0;  // Reset bei Erfolg
            console.log(`[History] Loaded ${converted.length} points, total: ${this.history.length}`);
            
        } catch (error) {
            console.error('[History] Load error:', error);
            this.lastErrorTime = Date.now();
            this.retryCount++;
            
            // Bei zu vielen Fehlern, in Offline-Modus gehen
            if (this.retryCount >= CONFIG.HISTORY_MAX_RETRIES) {
                console.warn('[History] Max retries reached, entering offline mode');
                this.isOffline = true;
                throw new Error('History service unavailable');
            }
            
            throw error;
        } finally {
            this.loading = false;
            
            // VERBESSERTE RETRY-LOGIK MIT EXPONENTIAL BACKOFF
            if (this.pendingRequest && !this.isOffline) {
                const pending = this.pendingRequest;
                this.pendingRequest = null;
                
                const delay = Math.min(
                    CONFIG.HISTORY_RETRY_BASE_DELAY * Math.pow(2, this.retryCount),
                    30000
                );
                
                console.log(`[History] Will retry in ${delay}ms (attempt ${this.retryCount + 1}/${CONFIG.HISTORY_MAX_RETRIES})`);
                
                setTimeout(() => {
                    if (!this.isOffline) {
                        this.loadWindow(pending.from, pending.to);
                    }
                }, delay);
            }
        }
    }
    
    mergeData(newData) {
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
        
        // Binäre Suche
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
        
        // Downsampling
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
        
        // Glättung
        if ((viewportRight - viewportLeft) > CONFIG.SMOOTHING_THRESHOLD && visible.length > 10) {
            const windowSize = Math.max(1, Math.floor(visible.length * CONFIG.SMOOTHING_WINDOW));
            const smoothed = [];
            
            for (let i = 0; i < visible.length; i++) {
                const startIdx = Math.max(0, i - windowSize);
                const endIdx = Math.min(visible.length - 1, i + windowSize);
                
                let sum = 0;
                let count = 0;
                
                for (let j = startIdx; j <= endIdx; j++) {
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
    
    // NEUE METHODEN
    setOfflineMode(offline) {
        this.isOffline = offline;
        if (!offline) {
            this.retryCount = 0;  // Reset beim Wiederverbinden
        }
    }
    
    getStatus() {
        return {
            offline: this.isOffline,
            retryCount: this.retryCount,
            lastErrorTime: this.lastErrorTime,
            cacheSize: this.cache.size,
            historyPoints: this.history.length
        };
    }
}
