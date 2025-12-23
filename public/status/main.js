// Haupt-State
let currentStatus = null;
let latestStudioTime = null;
let websocket = null;
const rateHistory = new Map();
let wsMessageCount = 0;
let autoRefreshInterval = null;
const panelIds = [
    'mainContent',
    'pipelinePanel',
    'ringbufferPanel',
    'activeModulesPanel',
    'inactiveModulesPanel',
    'fileOutPanel',
    'controlsPanel',
    'codecsPanel',
    'systemStatusPanel'
];

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
        if (!statusResponse.ok) {
            throw new Error('Status API nicht erreichbar');
        }
        
        const status = await statusResponse.json();
        const codecs = codecsResponse.ok ? await codecsResponse.json() : [];
        
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
        showOfflineView();
    }
}

function updateUI(status, codecs = []) {
    // Update timestamp
    document.getElementById('updateTime').textContent = formatTime(Date.now());
    
    const viewState = determineViewState(status);
    applyViewState(viewState, status);
    updateConnectionState(true);
    
    if (viewState !== 'normal') {
        return;
    }
    
    // Render all components
    renderAudioPipeline(status);
    renderModulesTable(status);
    renderInactiveModules(status);
    renderControls(status);
    renderCodecs(codecs);
    
    // Update system status
    updateSystemStatus(status);
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
        } else {
            updateConnectionState(false);
            showOfflineView();
        }
    } catch (error) {
        updateConnectionState(false);
        showOfflineView();
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
document.addEventListener('DOMContentLoaded', () => {
    const minimalConfigButton = document.getElementById('generateMinimalConfig');
    if (minimalConfigButton) {
        minimalConfigButton.addEventListener('click', () => {
            showMessage('Minimal-Konfig-Generator folgt (UI vorbereitet).', 'info');
        });
    }
    initializeAll();
});

function determineViewState(status) {
    if (!status) {
        return 'offline';
    }
    
    const configurationIssues = status.configuration_issues || [];
    const hasInputs = (status.graph?.nodes || []).some(node => node.kind === 'input');
    const hasOutputs = (status.graph?.nodes || []).some(node => node.kind === 'output');
    const hasConfigIssues = status.configuration_required || configurationIssues.length > 0;
    const isEmptyConfig = !hasInputs && !hasOutputs;
    
    if (hasConfigIssues || isEmptyConfig) {
        return 'setup';
    }
    
    return 'normal';
}

function issueExample(issueKey) {
    if (issueKey === 'graph') {
        return `[inputs.source]\n` +
            `type = "srt"\n` +
            `enabled = true\n` +
            `buffer = "main"\n\n` +
            `[outputs.srt_out]\n` +
            `type = "srt_out"\n` +
            `enabled = true\n` +
            `input = "source"\n` +
            `buffer = "main"\n` +
            `codec_id = "pcm_s16le"`;
    }

    if (issueKey === 'ringbuffer_id') {
        return `[ringbuffers.main]\n` +
            `slots = 6000\n` +
            `chunk_ms = 100\n` +
            `prealloc_samples = 9600`;
    }

    if (issueKey.startsWith('outputs.')) {
        return `[outputs.stream_out]\n` +
            `type = "icecast_out"\n` +
            `enabled = true\n` +
            `input = "source"\n` +
            `buffer = "main"\n` +
            `codec_id = "opus_ogg"\n` +
            `host = "icecast.local"\n` +
            `port = 8000\n` +
            `mount = "/airlift"\n\n` +
            `[codecs.opus_ogg]\n` +
            `type = "opus_ogg"\n` +
            `sample_rate = 48000\n` +
            `channels = 2`;
    }

    if (issueKey.startsWith('services.')) {
        return `[services.audio_http]\n` +
            `type = "audio_http"\n` +
            `enabled = true\n` +
            `codec_id = "pcm_s16le"\n\n` +
            `[codecs.pcm_s16le]\n` +
            `type = "pcm"\n` +
            `sample_rate = 48000\n` +
            `channels = 2`;
    }

    return `[codecs.pcm_s16le]\n` +
        `type = "pcm"\n` +
        `sample_rate = 48000\n` +
        `channels = 2`;
}

function issueExplanation(issueKey) {
    if (issueKey === 'graph') {
        return 'Definiere mindestens einen Input und einen Output, damit die Graph-Pipeline starten kann.';
    }
    if (issueKey === 'ringbuffer_id') {
        return 'Ringbuffer benötigt eine ID, die Inputs/Outputs referenzieren können.';
    }
    if (issueKey.startsWith('outputs.')) {
        return 'Outputs benötigen ein gültiges codec_id und eine Verbindung zu Input/Buffer.';
    }
    if (issueKey.startsWith('services.')) {
        return 'Services mit Audio-Output brauchen ein codec_id, das in [codecs] definiert ist.';
    }
    return 'Lege die fehlenden Konfigurationsfelder mit minimalen Defaults an.';
}

function applyViewState(viewState, status) {
    if (viewState === 'offline') {
        showOfflineView();
        return;
    }
    
    if (viewState === 'setup') {
        showSetupView(status);
        return;
    }
    
    showFullView();
}

function showOfflineView() {
    togglePanels(false);
    const offlinePanel = document.getElementById('offlinePanel');
    const setupPanel = document.getElementById('setupPanel');
    offlinePanel.classList.remove('hidden');
    setupPanel.classList.add('hidden');
}

function showSetupView(status) {
    togglePanels(false);
    document.getElementById('offlinePanel').classList.add('hidden');
    const setupPanel = document.getElementById('setupPanel');
    setupPanel.classList.remove('hidden');
    renderSetupGuide(status);
}

function showFullView() {
    togglePanels(true);
    document.getElementById('offlinePanel').classList.add('hidden');
    document.getElementById('setupPanel').classList.add('hidden');
}

function togglePanels(show) {
    panelIds.forEach(id => {
        const panel = document.getElementById(id);
        if (panel) {
            panel.classList.toggle('hidden', !show);
        }
    });
}

function renderSetupGuide(status) {
    const stepsElement = document.getElementById('setupSteps');
    const issuesElement = document.getElementById('setupIssues');
    const issuesSection = document.getElementById('setupIssuesSection');
    const nextStepElement = document.getElementById('setupNextStep');
    const configurationIssues = status?.configuration_issues || [];
    const hasInputs = (status?.graph?.nodes || []).some(node => node.kind === 'input');
    const hasOutputs = (status?.graph?.nodes || []).some(node => node.kind === 'output');
    
    let nextStep = 'Konfiguration prüfen';
    if (!hasInputs) {
        nextStep = 'Input konfigurieren';
    } else if (!hasOutputs) {
        nextStep = 'Output konfigurieren';
    } else if (configurationIssues.length > 0) {
        nextStep = 'Pflichtfelder ergänzen';
    }
    
    nextStepElement.textContent = nextStep;
    
    const steps = [];
    if (!hasInputs) {
        steps.push('Mindestens einen Input mit Buffer definieren (z. B. SRT oder HTTP Stream).');
    }
    if (!hasOutputs) {
        steps.push('Mindestens einen Output mit codec_id konfigurieren.');
    }
    if (configurationIssues.length > 0) {
        steps.push('Offene Pflichtfelder in der Konfiguration beheben.');
    }
    if (steps.length === 0) {
        steps.push('Konfiguration speichern und die API neu laden.');
    }
    
    stepsElement.innerHTML = steps.map(step => `<li>${step}</li>`).join('');
    
    if (configurationIssues.length > 0) {
        issuesSection.classList.remove('hidden');
        issuesElement.innerHTML = configurationIssues.map(issue => `
            <div class="setup-issue-card">
                <div class="setup-issue-header">
                    <span>${issueExplanation(issue.key)}</span>
                    <span class="setup-issue-key">${issue.key}</span>
                </div>
                <div class="setup-issue-message">${issue.message}</div>
                <div class="setup-issue-example">${issueExample(issue.key)}</div>
            </div>
        `).join('');
    } else {
        issuesSection.classList.add('hidden');
        issuesElement.innerHTML = '';
    }
}
