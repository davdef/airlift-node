// Haupt-State
let currentStatus = null;
let latestStudioTime = null;
let websocket = null;
const rateHistory = new Map();
let wsMessageCount = 0;
let autoRefreshInterval = null;

// Canvas-State (wird von waveform.js verwaltet)
let ringbufferCanvas, ringbufferCtx, recorderCanvas, recorderCtx;

// Initialisierung
async function initializeAll() {
    try {
        // Canvas-Referenzen setzen
        ringbufferCanvas = document.getElementById('ringbufferCanvas');
        ringbufferCtx = ringbufferCanvas.getContext('2d');
        recorderCanvas = document.getElementById('recorderCanvas');
        recorderCtx = recorderCanvas.getContext('2d');
        
        // Initialize canvases
        initCanvases();
        
        // Load initial status
        await fetchAllData();
        
        // Setup WebSocket
        setupWebSocket();
        
        // Start auto-refresh
        startAutoRefresh();
        
        updateConnectionState(true);
        
    } catch (error) {
        updateConnectionState(false);
    }
}

// Canvas initialisieren
function initCanvases() {
    const ringbufferContainer = ringbufferCanvas.parentElement;
    ringbufferCanvas.width = ringbufferContainer.clientWidth;
    ringbufferCanvas.height = ringbufferContainer.clientHeight;
    
    const recorderContainer = recorderCanvas.parentElement;
    recorderCanvas.width = recorderContainer.clientWidth;
    recorderCanvas.height = recorderContainer.clientHeight;
}

window.addEventListener('resize', initCanvases);

// Helper functions
function formatTime(ms) {
    if (!ms) return '–';
    const date = new Date(ms);
    return date.toLocaleTimeString('de-DE', {
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit'
    });
}

function formatDuration(ms) {
    if (!ms) return '0s';
    const seconds = Math.floor(ms / 1000);
    if (seconds < 60) return `${seconds}s`;
    const minutes = Math.floor(seconds / 60);
    return `${minutes}m ${seconds % 60}s`;
}

function showMessage(text, type = 'info') {
    const bar = document.getElementById('messageBar');
    bar.textContent = text;
    bar.className = `message-bar show ${type}`;
    
    setTimeout(() => {
        bar.className = 'message-bar';
    }, 3000);
}

function updateConnectionState(connected) {
    const dot = document.querySelector('.conn-dot');
    const text = document.getElementById('connText');
    
    if (connected) {
        dot.className = 'conn-dot connected';
        text.textContent = 'Verbunden';
    } else {
        dot.className = 'conn-dot error';
        text.textContent = 'Getrennt';
    }
}

function calculateRate(moduleId, currentCounters, timestamp) {
    if (!rateHistory.has(moduleId)) {
        rateHistory.set(moduleId, {
            timestamp,
            counters: currentCounters
        });
        return { rx: '–', tx: '–' };
    }
    
    const prev = rateHistory.get(moduleId);
    const timeDiff = Math.max(1, timestamp - prev.timestamp);
    
    const rxRate = Math.round((currentCounters.rx - prev.counters.rx) * 60000 / timeDiff);
    const txRate = Math.round((currentCounters.tx - prev.counters.tx) * 60000 / timeDiff);
    
    rateHistory.set(moduleId, {
        timestamp,
        counters: currentCounters
    });
    
    return {
        rx: rxRate.toLocaleString(),
        tx: txRate.toLocaleString()
    };
}

// API Calls
async function fetchAllData() {
    try {
        const [statusResponse, codecsResponse] = await Promise.all([
            fetch('/api/status'),
            fetch('/api/codecs')
        ]);
        
        const status = await statusResponse.json();
        const codecs = await codecsResponse.json();
        
        // Update global state
        currentStatus = status;
        
        // Update UI
        updateUI(status, codecs);
        
        // Initialize waveforms with history
        await Promise.all([
            initializeRingbuffer(status),
            initializeFileOut(status)
        ]);
        
    } catch (error) {
        updateConnectionState(false);
    }
}

function updateUI(status, codecs = []) {
    // Update timestamp
    document.getElementById('updateTime').textContent = formatTime(Date.now());
    
    // Render all components
    renderAudioPipeline(status);
    renderModulesTable(status);
    renderInactiveModules(status);
    renderControls(status);
    renderCodecs(codecs);
    
    // Update system status
    updateSystemStatus(status);
    
    updateConnectionState(true);
}

function updateSystemStatus(status) {
    // Update Studio-Zeit
    document.getElementById('systemStudioTime').textContent = formatTime(latestStudioTime);
    
    // Update Latency
    if (latestStudioTime) {
        const latency = Date.now() - latestStudioTime;
        document.getElementById('systemLatency').textContent = `${Math.max(0, latency)}ms`;
    }
    
    // Update Buffer stats
    if (status.ring) {
        const errorFills = status.ring.fill || 0;
        document.getElementById('systemAudioBuffer').textContent = `${Math.round((status.ring.capacity || 6000) * 0.1)}s`;
        document.getElementById('ringbufferInfo').textContent = `Error-Fills: ${errorFills}`;
    }
}

// WebSocket
function setupWebSocket() {
    const proto = location.protocol === 'https:' ? 'wss://' : 'ws://';
    const wsUrl = proto + window.location.host + '/ws';
    
    websocket = new WebSocket(wsUrl);
    
    websocket.onopen = () => {
        updateConnectionState(true);
    };
    
    websocket.onerror = (err) => {
        updateConnectionState(false);
    };
    
    websocket.onclose = () => {
        updateConnectionState(false);
        setTimeout(setupWebSocket, 5000);
    };
    
    websocket.onmessage = (e) => {
        try {
            const data = JSON.parse(e.data);
            if (data && typeof data.timestamp === 'number') {
                wsMessageCount += 1;
                latestStudioTime = data.timestamp;
                
                // Update Studio-Zeit im Header
                document.getElementById('studioTime').textContent = formatTime(latestStudioTime);
                
                // Update System Status
                updateSystemStatus(currentStatus);
                
                // Extract peaks if available
                let peaks = null;
                let silence = false;
                
                if (Array.isArray(data.peaks)) {
                    peaks = data.peaks;
                }
                
                if (data.silence !== undefined) {
                    silence = data.silence;
                }
                
                // Update waveforms with new peak
                if (peaks !== null || silence) {
                    updateRingbufferPoint(data.timestamp, peaks || [0, 0], silence);
                    updateFileOutPoint(data.timestamp, peaks, silence);
                }
                
                // Auto-refresh data every 10 WS messages
                if (wsMessageCount % 10 === 0) {
                    refreshStatusData();
                }
            }
        } catch (err) {
            // Silent error
        }
    };
}

// Auto-refresh
function startAutoRefresh() {
    if (autoRefreshInterval) clearInterval(autoRefreshInterval);
    // Refresh every 30 seconds
    autoRefreshInterval = setInterval(refreshStatusData, 30000);
}

async function refreshStatusData() {
    try {
        const statusResponse = await fetch('/api/status');
        if (statusResponse.ok) {
            currentStatus = await statusResponse.json();
            updateUI(currentStatus);
        }
    } catch (error) {
        // Silent error
    }
}

// Control actions
async function sendControl(action) {
    try {
        const response = await fetch('/api/control', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ action })
        });
        
        const data = await response.json();
        
        if (response.ok) {
            showMessage(data.message || 'Aktion erfolgreich', 'success');
        } else {
            showMessage(data.message || 'Fehler', 'error');
        }
        
        setTimeout(refreshStatusData, 500);
        
    } catch (error) {
        showMessage(`Netzwerkfehler: ${error.message}`, 'error');
    }
}

// Manuelles Refresh
async function refreshAllData() {
    await fetchAllData();
}

// Start everything
document.addEventListener('DOMContentLoaded', initializeAll);
