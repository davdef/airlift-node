// Haupt-State
let currentStatus = null;
let latestStudioTime = null;
let websocket = null;
const rateHistory = new Map();
let wsMessageCount = 0;
let autoRefreshInterval = null;
const inputDeviceState = {
    available: false,
    devices: [],
    loaded: false,
    error: null
};
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
const auxiliaryPanelIds = [
    'ringbufferPanel',
    'activeModulesPanel',
    'inactiveModulesPanel',
    'fileOutPanel',
    'controlsPanel',
    'codecsPanel',
    'systemStatusPanel'
];

let pipelineValidationOk = false;

const pipelineCatalog = [
    {
        group: 'Inputs',
        items: [
            {
                id: 'input-mic',
                label: 'Microphone Input',
                type: 'input',
                requiresCodec: false,
                producesSignal: 'audio',
                consumesSignal: null,
                configSchema: {
                    fields: [
                        { key: 'device', label: 'Device', type: 'select', placeholder: 'default', options: [] }
                    ]
                }
            },
            {
                id: 'input-srt',
                label: 'SRT Receiver',
                type: 'input',
                requiresCodec: true,
                producesSignal: 'encoded',
                consumesSignal: null,
                configSchema: {
                    fields: [
                        { key: 'listen', label: 'Listen URI', type: 'text', placeholder: 'srt://:9000' }
                    ]
                }
            },
            {
                id: 'input-file',
                label: 'File Input',
                type: 'input',
                requiresCodec: true,
                producesSignal: 'encoded',
                consumesSignal: null,
                configSchema: {
                    fields: [
                        { key: 'path', label: 'File Path', type: 'text', placeholder: '/path/to/audio.mp3' }
                    ]
                }
            }
        ]
    },
    {
        group: 'Processors',
        items: [
            {
                id: 'proc-mixer',
                label: 'Mixer',
                type: 'processor',
                requiresCodec: false,
                producesSignal: 'audio',
                consumesSignal: 'audio',
                configSchema: {
                    fields: [
                        { key: 'channels', label: 'Channels', type: 'number', placeholder: '2' }
                    ]
                }
            },
            {
                id: 'proc-resampler',
                label: 'Resampler',
                type: 'processor',
                requiresCodec: false,
                producesSignal: 'audio',
                consumesSignal: 'audio',
                configSchema: {
                    fields: [
                        { key: 'rate', label: 'Sample Rate', type: 'number', placeholder: '48000' }
                    ]
                }
            }
        ]
    },
    {
        group: 'Outputs',
        items: [
            {
                id: 'output-stream',
                label: 'Stream Output',
                type: 'output',
                requiresCodec: true,
                producesSignal: null,
                consumesSignal: 'encoded',
                configSchema: {
                    fields: [
                        { key: 'target', label: 'Target URI', type: 'text', placeholder: 'srt://target' }
                    ]
                }
            },
            {
                id: 'output-file',
                label: 'File Output',
                type: 'output',
                requiresCodec: true,
                producesSignal: null,
                consumesSignal: 'encoded',
                configSchema: {
                    fields: [
                        { key: 'path', label: 'File Path', type: 'text', placeholder: '/path/to/output.mp3' }
                    ]
                }
            }
        ]
    },
    {
        group: 'Services',
        items: [
            {
                id: 'service-health',
                label: 'Health Reporter',
                type: 'service',
                requiresCodec: false,
                producesSignal: null,
                consumesSignal: null,
                configSchema: {
                    fields: [
                        { key: 'interval', label: 'Interval (s)', type: 'number', placeholder: '30' }
                    ]
                }
            },
            {
                id: 'service-metadata',
                label: 'Metadata Sync',
                type: 'service',
                requiresCodec: false,
                producesSignal: null,
                consumesSignal: null,
                configSchema: {
                    fields: [
                        { key: 'endpoint', label: 'Endpoint', type: 'text', placeholder: 'https://api.example' }
                    ]
                }
            }
        ]
    },
    {
        group: 'Codecs',
        items: [
            {
                id: 'codec-decoder-aac',
                label: 'AAC Decoder',
                type: 'decoder',
                requiresCodec: false,
                producesSignal: 'audio',
                consumesSignal: 'encoded',
                configSchema: {
                    fields: [
                        { key: 'profile', label: 'Profile', type: 'text', placeholder: 'LC' }
                    ]
                }
            },
            {
                id: 'codec-encoder-aac',
                label: 'AAC Encoder',
                type: 'encoder',
                requiresCodec: false,
                producesSignal: 'encoded',
                consumesSignal: 'audio',
                configSchema: {
                    fields: [
                        { key: 'bitrate', label: 'Bitrate (kbps)', type: 'number', placeholder: '192' }
                    ]
                }
            }
        ]
    }
];

const pipelineState = {
    nodes: [],
    selectedNodeId: null,
    nodeCounter: 0
};

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

        initPipelineBuilder();
        
        await fetchDeviceList();

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

function initPipelineBuilder() {
    renderPipelinePalette();
    setupPipelineCanvas();
    renderPipelineNodes();
    renderPipelineInspector();
}

function renderPipelinePalette() {
    const palette = document.getElementById('pipelinePalette');
    if (!palette) {
        return;
    }
    palette.innerHTML = '';
    pipelineCatalog.forEach(group => {
        const groupWrapper = document.createElement('div');
        groupWrapper.className = 'pipeline-palette-group';

        const groupTitle = document.createElement('div');
        groupTitle.className = 'pipeline-palette-group-title';
        groupTitle.textContent = group.group;
        groupWrapper.appendChild(groupTitle);

        group.items.forEach(item => {
            const entry = document.createElement('div');
            entry.className = 'pipeline-palette-item';
            entry.draggable = true;
            entry.dataset.paletteId = item.id;
            entry.dataset.type = item.type;
            entry.dataset.requiresCodec = item.requiresCodec;
            entry.dataset.producesSignal = item.producesSignal || '';
            entry.dataset.consumesSignal = item.consumesSignal || '';
            entry.innerHTML = `
                <span>${item.label}</span>
                <span class="palette-badge">${item.type}</span>
            `;
            entry.addEventListener('dragstart', event => {
                event.dataTransfer.setData('text/plain', item.id);
                event.dataTransfer.effectAllowed = 'copy';
            });
            groupWrapper.appendChild(entry);
        });

        palette.appendChild(groupWrapper);
    });
}

function setupPipelineCanvas() {
    const canvas = document.getElementById('pipelineCanvas');
    if (!canvas) {
        return;
    }
    canvas.addEventListener('dragover', event => {
        event.preventDefault();
        event.dataTransfer.dropEffect = 'copy';
    });
    canvas.addEventListener('drop', event => {
        event.preventDefault();
        const paletteId = event.dataTransfer.getData('text/plain');
        const item = findCatalogItem(paletteId);
        if (!item) {
            return;
        }
        const rect = canvas.getBoundingClientRect();
        const position = {
            x: event.clientX - rect.left - 75,
            y: event.clientY - rect.top - 25
        };
        addPipelineNodeFromItem(item, position);
    });
    canvas.addEventListener('click', event => {
        if (event.target.closest('.pipeline-node')) {
            return;
        }
        pipelineState.selectedNodeId = null;
        renderPipelineNodes();
        renderPipelineInspector();
    });
}

function findCatalogItem(id) {
    for (const group of pipelineCatalog) {
        const match = group.items.find(item => item.id === id);
        if (match) {
            return match;
        }
    }
    return null;
}

function addPipelineNodeFromItem(item, position) {
    const nodeId = `node-${Date.now()}-${pipelineState.nodeCounter++}`;
    const canvas = document.getElementById('pipelineCanvas');
    const bounds = canvas ? canvas.getBoundingClientRect() : { width: 500, height: 300 };
    const clampedPosition = {
        x: Math.max(10, Math.min(position.x, bounds.width - 170)),
        y: Math.max(10, Math.min(position.y, bounds.height - 80))
    };
    const node = {
        id: nodeId,
        label: item.label,
        type: item.type,
        metadata: {
            requiresCodec: item.requiresCodec,
            producesSignal: item.producesSignal,
            consumesSignal: item.consumesSignal
        },
        configSchema: item.configSchema,
        config: {},
        position: clampedPosition
    };
    pipelineState.nodes.push(node);
    pipelineState.selectedNodeId = nodeId;
    renderPipelineNodes();
    renderPipelineInspector();
}

function renderPipelineNodes() {
    const container = document.getElementById('pipelineNodes');
    const hint = document.getElementById('pipelineCanvasHint');
    if (!container) {
        return;
    }
    container.innerHTML = '';
    pipelineState.nodes.forEach(node => {
        const nodeElement = document.createElement('div');
        nodeElement.className = 'pipeline-node';
        if (pipelineState.selectedNodeId === node.id) {
            nodeElement.classList.add('selected');
        }
        nodeElement.style.left = `${node.position.x}px`;
        nodeElement.style.top = `${node.position.y}px`;

        const metaText = `${node.type}${node.metadata.requiresCodec ? ' · codec' : ''}`;
        nodeElement.innerHTML = `
            <div class="pipeline-node-title">${node.label}</div>
            <div class="pipeline-node-meta">${metaText}</div>
            <div class="pipeline-node-handles">
                ${node.metadata.consumesSignal ? '<div class="node-handle input"></div>' : '<div></div>'}
                ${node.metadata.producesSignal ? '<div class="node-handle output"></div>' : '<div></div>'}
            </div>
        `;
        nodeElement.addEventListener('click', event => {
            event.stopPropagation();
            pipelineState.selectedNodeId = node.id;
            renderPipelineNodes();
            renderPipelineInspector();
        });
        container.appendChild(nodeElement);
    });
    if (hint) {
        hint.classList.toggle('hidden', pipelineState.nodes.length > 0);
    }
}

function renderPipelineInspector() {
    const inspector = document.getElementById('pipelineInspector');
    if (!inspector) {
        return;
    }
    const node = pipelineState.nodes.find(entry => entry.id === pipelineState.selectedNodeId);
    if (!node) {
        inspector.innerHTML = `
            <div class="empty-state">
                <div class="icon">🧩</div>
                <div class="message">Wähle ein Modul aus.</div>
            </div>
        `;
        return;
    }
    inspector.innerHTML = '';

    const nameLabel = document.createElement('div');
    nameLabel.className = 'label';
    nameLabel.textContent = 'Name';
    inspector.appendChild(nameLabel);

    const nameInput = document.createElement('input');
    nameInput.className = 'pipeline-inspector-input';
    nameInput.type = 'text';
    nameInput.value = node.label;
    nameInput.addEventListener('input', event => {
        node.label = event.target.value;
        renderPipelineNodes();
    });
    inspector.appendChild(nameInput);

    if (node.configSchema?.fields?.length) {
        node.configSchema.fields.forEach(field => {
            const fieldLabel = document.createElement('div');
            fieldLabel.className = 'label';
            fieldLabel.textContent = field.label;
            inspector.appendChild(fieldLabel);

            let input;
            const isDeviceField = field.key === 'device';
            const hasDeviceOptions = isDeviceField && Array.isArray(field.options) && field.options.length > 0;
            if (field.type === 'select' && (hasDeviceOptions || !isDeviceField)) {
                input = document.createElement('select');
                input.className = 'pipeline-inspector-input';
                field.options?.forEach(option => {
                    const optionEl = document.createElement('option');
                    optionEl.value = option.value;
                    optionEl.textContent = option.label;
                    input.appendChild(optionEl);
                });
            } else {
                input = document.createElement('input');
                input.className = 'pipeline-inspector-input';
                input.type = field.type === 'number' ? 'number' : 'text';
                input.placeholder = field.placeholder || '';
            }
            if (isDeviceField && hasDeviceOptions && (node.config[field.key] === undefined || node.config[field.key] === '')) {
                node.config[field.key] = field.options[0]?.value || '';
            }
            input.value = node.config[field.key] ?? '';
            input.addEventListener('input', event => {
                node.config[field.key] = event.target.value;
            });
            inspector.appendChild(input);

            if (isDeviceField && !hasDeviceOptions) {
                const fallback = document.createElement('div');
                fallback.className = 'pipeline-inspector-hint';
                if (!inputDeviceState.loaded) {
                    fallback.textContent = 'ALSA-Geräte werden geladen …';
                } else if (inputDeviceState.error) {
                    fallback.textContent = 'Geräte nicht verfügbar. Bitte Device manuell eintragen.';
                } else {
                    fallback.textContent = 'Keine Geräte gefunden. Bitte Device manuell eintragen.';
                }
                inspector.appendChild(fallback);
            }
        });
    }

    const metaLabel = document.createElement('div');
    metaLabel.className = 'label';
    metaLabel.textContent = 'Meta';
    inspector.appendChild(metaLabel);

    const metaValue = document.createElement('div');
    metaValue.className = 'value';
    metaValue.textContent = `type=${node.type}, requiresCodec=${node.metadata.requiresCodec}`;
    inspector.appendChild(metaValue);
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
async function fetchDeviceList() {
    try {
        const response = await fetch('/api/devices');
        if (!response.ok) {
            throw new Error('Device API nicht erreichbar');
        }
        const data = await response.json();
        inputDeviceState.available = Boolean(data.available);
        inputDeviceState.devices = Array.isArray(data.devices) ? data.devices : [];
        inputDeviceState.loaded = true;
        inputDeviceState.error = null;
    } catch (error) {
        inputDeviceState.available = false;
        inputDeviceState.devices = [];
        inputDeviceState.loaded = true;
        inputDeviceState.error = error;
    }
    updateDeviceCatalogOptions();
    renderPipelineInspector();
}

function updateDeviceCatalogOptions() {
    const micItem = findCatalogItem('input-mic');
    if (!micItem?.configSchema?.fields) {
        return;
    }
    const deviceField = micItem.configSchema.fields.find(field => field.key === 'device');
    if (!deviceField) {
        return;
    }
    deviceField.options = inputDeviceState.devices.map(device => ({
        value: device.id,
        label: device.label
    }));
}

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
            const result = window.renderPipelineConfigPreview?.();
            if (result?.issues?.length) {
                showMessage('Konfiguration generiert (Validierungswarnungen vorhanden).', 'warning');
            } else {
                showMessage('Konfiguration generiert.', 'success');
            }
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
    document.getElementById('offlinePanel').classList.add('hidden');
    const setupPanel = document.getElementById('setupPanel');
    togglePanels(true);
    setupPanel.classList.remove('hidden');
    renderSetupGuide(status);
    const validation = window.renderPipelineConfigPreview?.();
    updatePipelineValidationState(validation?.issues || []);
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
    const contextStepsElement = document.getElementById('pipelineContextHints');
    const contextNextStep = document.getElementById('pipelineContextNextStep');
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
    if (contextNextStep) {
        contextNextStep.textContent = nextStep;
    }
    
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
    
    if (contextStepsElement) {
        contextStepsElement.innerHTML = steps.map(step => `<li>${step}</li>`).join('');
    }
    
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

function updatePipelineValidationState(issues) {
    const isValid = !issues || issues.length === 0;
    pipelineValidationOk = isValid;
    auxiliaryPanelIds.forEach(id => {
        const panel = document.getElementById(id);
        if (!panel) {
            return;
        }
        panel.classList.toggle('disabled', !isValid);
    });
}

window.updatePipelineValidationState = updatePipelineValidationState;
