export class DebugMonitor {
    constructor(selectors) {
        this.elements = {};
        for (const [key, selector] of Object.entries(selectors)) {
            this.elements[key] = document.querySelector(selector);
        }
        this.updateInterval = null;
    }
    
    start(player, updateInterval = 1000) {
        this.player = player;
        this.updateInterval = setInterval(() => this.update(), updateInterval);
        this.update(); // Sofortiges Update
    }
    
    update() {
        if (!this.player) return;
        
        const viewport = this.player.viewport?.visibleRange || {};
        const audioState = this.player.audio?.getState() || {};
        const history = this.player.history?.history || [];
        
        this.set('status', this.player.ui?.elements.status?.textContent || '–');
        this.set('dbgMode', audioState.isLive ? 'LIVE' : 'TIMESHIFT');
        this.set('dbgViewport', `${this.formatTime(viewport.left)} – ${this.formatTime(viewport.right)}`);
        this.set('dbgPlayhead', this.formatTime(this.player.audio?.getCurrentTime(this.player.getServerNow()) || 0));
        this.set('dbgAudioTime', audioState.currentTime?.toFixed(1) || '–');
        this.set('dbgHistory', `${history.length} points`);
        this.set('dbgLastWs', this.player.lastWsUpdate ? `${Math.round(Date.now() - this.player.lastWsUpdate)}ms ago` : '–');
        
        if (this.player.bufferInfo) {
            const duration = this.player.bufferInfo.end - this.player.bufferInfo.start;
            this.set('bufferInfo', `${this.player.formatDuration(duration)} buffer`);
        }
    }
    
    formatTime(timestamp) {
        if (!timestamp || !Number.isFinite(timestamp)) return '–';
        const d = new Date(timestamp);
        return `${d.getHours().toString().padStart(2, '0')}:${d.getMinutes().toString().padStart(2, '0')}:${d.getSeconds().toString().padStart(2, '0')}`;
    }
    
    set(key, value) {
        if (this.elements[key]) {
            this.elements[key].textContent = value;
        }
    }
    
    stop() {
        if (this.updateInterval) {
            clearInterval(this.updateInterval);
            this.updateInterval = null;
        }
    }
}
