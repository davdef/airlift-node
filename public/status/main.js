/**
 * @typedef {Object} StatusResponse
 * @property {boolean} running
 * @property {number} uptime_seconds
 * @property {Array<ProducerInfo>} producers
 * @property {Array<FlowInfo>} flows
 * @property {RingBufferInfo} ringbuffer
 * @property {Array<ModuleInfo>} modules
 * @property {Array<InactiveModule>} inactive_modules
 * @property {Array<ConfigurationIssue>} configuration_issues
 * @property {number} timestamp_ms
 */

/**
 * @typedef {Object} ProducerInfo
 * @property {string} name
 * @property {boolean} running
 * @property {boolean} connected
 * @property {number} samples_processed
 * @property {number} errors
 */

/**
 * @typedef {Object} FlowInfo
 * @property {string} name
 * @property {boolean} running
 * @property {Array<number>} input_buffer_levels
 * @property {Array<number>} processor_buffer_levels
 * @property {number} output_buffer_level
 */

/**
 * @typedef {Object} RingBufferInfo
 * @property {number} fill
 * @property {number} capacity
 */

/**
 * @typedef {Object} ModuleInfo
 * @property {string} id
 * @property {string} label
 * @property {string} module_type
 * @property {ModuleRuntime} runtime
 * @property {Array<ModuleControl>} controls
 */

/**
 * @typedef {Object} ModuleRuntime
 * @property {boolean} enabled
 * @property {boolean} running
 * @property {boolean | null} connected
 * @property {ModuleCounters} counters
 * @property {number} last_activity_ms
 */

/**
 * @typedef {Object} ModuleCounters
 * @property {number} rx
 * @property {number} tx
 * @property {number} errors
 */

/**
 * @typedef {Object} ModuleControl
 * @property {string} action
 * @property {string} label
 * @property {boolean} enabled
 * @property {string | null} reason
 */

/**
 * @typedef {Object} InactiveModule
 * @property {string} id
 * @property {string} label
 * @property {string} module_type
 * @property {string} reason
 */

/**
 * @typedef {Object} ConfigurationIssue
 * @property {string} key
 * @property {string} message
 */

/**
 * @typedef {Object} CatalogResponse
 * @property {Array<CatalogItem>} inputs
 * @property {Array<CatalogItem>} buffers
 * @property {Array<CatalogItem>} processing
 * @property {Array<CatalogItem>} services
 * @property {Array<CatalogItem>} outputs
 */

/**
 * @typedef {Object} CatalogItem
 * @property {string} name
 * @property {string} type
 * @property {string | undefined} flow
 */

const API_ENDPOINTS = {
    status: '/api/status',
    catalog: '/api/catalog',
    control: '/api/control',
    ws: '/ws'
};

// Haupt-State
let currentStatus = null;
let latestStudioTime = null;
let websocket = null;
const rateHistory = new Map();
let wsMessageCount = 0;
let autoRefreshInterval = null;
let moduleDisplayFilter = 'all';
let currentViewState = 'normal';
const MODULE_FILTER_STORAGE_KEY = 'moduleDisplayFilter';
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

function loadModuleFilterState() {
    const stored = localStorage.getItem(MODULE_FILTER_STORAGE_KEY);
    if (stored === 'active' || stored === 'configured' || stored === 'all') {
        moduleDisplayFilter = stored;
    }
}

function saveModuleFilterState() {
    localStorage.setItem(MODULE_FILTER_STORAGE_KEY, moduleDisplayFilter);
}

function updateModuleFilterUI() {
    document.querySelectorAll('[data-module-filter]').forEach(button => {
        const isActive = button.dataset.moduleFilter === moduleDisplayFilter;
        button.classList.toggle('active', isActive);
    });
    const configuredBadge = document.getElementById('configuredFilterBadge');
    if (configuredBadge) {
        configuredBadge.classList.toggle('hidden', moduleDisplayFilter !== 'configured');
    }
}

function applyModuleFilterVisibility() {
    if (currentViewState === 'offline' || currentViewState === 'idle') {
        return;
    }
    const activePanel = document.getElementById('activeModulesPanel');
    const inactivePanel = document.getElementById('inactiveModulesPanel');
    const showActive = moduleDisplayFilter !== 'configured';
    const showInactive = moduleDisplayFilter !== 'active';

    if (activePanel) {
        activePanel.classList.toggle('hidden', !showActive);
    }
    if (inactivePanel) {
        inactivePanel.classList.toggle('hidden', !showInactive);
    }
}

function initializeModuleFilterControls() {
    loadModuleFilterState();
    updateModuleFilterUI();
    applyModuleFilterVisibility();
    document.querySelectorAll('[data-module-filter]').forEach(button => {
        button.addEventListener('click', () => {
            moduleDisplayFilter = button.dataset.moduleFilter;
            saveModuleFilterState();
            updateModuleFilterUI();
            applyModuleFilterVisibility();
        });
    });
}

function updateModuleFilterAvailability(status, localState) {
    const modules = status?.modules || [];
    const inactive = status?.inactive_modules || [];
    const hasModuleDefinitions = modules.length > 0 || inactive.length > 0;
    const resolvedState = localState || getLocalPipelineState(status);
    const hasPipeline = resolvedState.nodesCount > 0;
    const enableAllFilters = hasPipeline;
    const shouldDefaultToConfigured = !hasModuleDefinitions;

    if (shouldDefaultToConfigured && moduleDisplayFilter !== 'configured') {
        moduleDisplayFilter = 'configured';
        saveModuleFilterState();
    }

    if (!enableAllFilters && moduleDisplayFilter !== 'configured') {
        moduleDisplayFilter = 'configured';
        saveModuleFilterState();
    }

    document.querySelectorAll('[data-module-filter]').forEach(button => {
        const filter = button.dataset.moduleFilter;
        const shouldDisable = !enableAllFilters && (filter === 'active' || filter === 'all');
        button.disabled = shouldDisable;
        button.classList.toggle('disabled', shouldDisable);
    });

    updateModuleFilterUI();
    applyModuleFilterVisibility();
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
        const statusResponse = await fetch(API_ENDPOINTS.status);
        if (!statusResponse.ok) {
            throw new Error('Status API nicht erreichbar');
        }

        const status = normalizeStatusResponse(await statusResponse.json());
        latestStudioTime = status.timestamp_ms || Date.now();

        await loadPipelineCatalog();
        
        // Update global state
        currentStatus = status;
        
        // Update UI
        updateUI(status, []);
        
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

async function loadPipelineCatalog() {
    try {
        const response = await fetch(API_ENDPOINTS.catalog);
        if (!response.ok) {
            return;
        }
        const catalog = /** @type {CatalogResponse} */ (await response.json());
        pipelineCatalog = normalizePipelineCatalog(catalog);
    } catch (error) {
        // Keep fallback catalog
    }
}

function updateUI(status, codecs = []) {
    // Update timestamp
    const statusTimestamp = status?.timestamp_ms || Date.now();
    document.getElementById('updateTime').textContent = formatTime(statusTimestamp);

    const localState = getLocalPipelineState(status);
    const viewState = determineViewState(status, localState);
    setViewState(viewState, status, localState);
    updateModuleFilterAvailability(status, localState);
    applyModuleFilterVisibility();
    updateConnectionState(true);

    if (viewState === 'offline' || viewState === 'idle') {
        return;
    }

    renderAudioPipeline(status);

    if (viewState !== 'normal') {
        return;
    }

    // Render all components
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
    if (status.ringbuffer) {
        const fill = status.ringbuffer.fill || 0;
        const capacity = status.ringbuffer.capacity || 0;
        document.getElementById('systemAudioBuffer').textContent = `${Math.round((capacity || 6000) * 0.1)}s`;
        document.getElementById('ringbufferInfo').textContent = `Füllstand: ${fill}/${capacity || '–'}`;
    }
}

// WebSocket
function setupWebSocket() {
    const proto = location.protocol === 'https:' ? 'wss://' : 'ws://';
    const wsUrl = proto + window.location.host + API_ENDPOINTS.ws;
    
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
            const eventTimestamp = normalizeEventTimestamp(data?.timestamp);
            if (data && typeof eventTimestamp === 'number') {
                wsMessageCount += 1;
                latestStudioTime = eventTimestamp;
                
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
                    updateRingbufferPoint(eventTimestamp, peaks || [0, 0], silence);
                    updateFileOutPoint(eventTimestamp, peaks, silence);
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
        const statusResponse = await fetch(API_ENDPOINTS.status);
        if (statusResponse.ok) {
            currentStatus = normalizeStatusResponse(await statusResponse.json());
            latestStudioTime = currentStatus.timestamp_ms || latestStudioTime;
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
        const response = await fetch(API_ENDPOINTS.control, {
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
        minimalConfigButton.addEventListener('click', async () => {
            const result = window.renderPipelineConfigPreview?.();
            if (!result?.config) {
                showMessage('Konfiguration konnte nicht generiert werden.', 'error');
                return;
            }
            const tomlPayload = formatConfigAsToml(result.config);
            const importResult = await importPipelineConfig(tomlPayload);
            if (importResult.ok) {
                showMessage(importResult.message || 'Konfiguration importiert.', 'success');
            } else {
                showMessage(importResult.message || 'Import fehlgeschlagen.', 'error');
            }
        });
    }
    initializeModuleFilterControls();
    initializeAll();
});

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

function normalizeStatusResponse(payload) {
    if (!payload || typeof payload !== 'object') {
        return {
            running: false,
            uptime_seconds: 0,
            producers: [],
            flows: [],
            ringbuffer: { fill: 0, capacity: 0 },
            modules: [],
            inactive_modules: [],
            configuration_issues: [],
            timestamp_ms: Date.now()
        };
    }
    return {
        running: Boolean(payload.running),
        uptime_seconds: Number(payload.uptime_seconds || 0),
        producers: Array.isArray(payload.producers) ? payload.producers : [],
        flows: Array.isArray(payload.flows) ? payload.flows : [],
        ringbuffer: payload.ringbuffer || { fill: 0, capacity: 0 },
        modules: Array.isArray(payload.modules) ? payload.modules : [],
        inactive_modules: Array.isArray(payload.inactive_modules) ? payload.inactive_modules : [],
        configuration_issues: Array.isArray(payload.configuration_issues)
            ? payload.configuration_issues
            : [],
        timestamp_ms: Number(payload.timestamp_ms || Date.now())
    };
}

function normalizePipelineCatalog(catalog) {
    if (!catalog || typeof catalog !== 'object') {
        return [];
    }
    const buildGroup = (kind, title, items) => ({
        kind,
        title,
        items: (items || []).map((item) => {
            const typeValue = item.type || pipelineDefaultTypes[kind] || 'unknown';
            return {
                label: item.name,
                type: typeValue,
                backendType: typeValue,
                supported: true,
                configFields: []
            };
        })
    });
    return [
        buildGroup('input', 'Inputs', catalog.inputs),
        buildGroup('buffer', 'Buffers', catalog.buffers),
        buildGroup('processing', 'Processing', catalog.processing),
        buildGroup('service', 'Services', catalog.services),
        buildGroup('output', 'Outputs', catalog.outputs)
    ];
}

async function importPipelineConfig(tomlPayload) {
    try {
        const response = await fetch(API_ENDPOINTS.control, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
                action: 'config.import',
                parameters: { toml: tomlPayload }
            })
        });
        const data = await response.json();
        return {
            ok: response.ok && data?.ok !== false,
            message: data?.message
        };
    } catch (error) {
        return {
            ok: false,
            message: error.message
        };
    }
}

function determineViewState(status, localState) {
    if (!status) {
        return 'offline';
    }
    const resolvedState = localState || getLocalPipelineState(status);
    const hasLocalIssues = (resolvedState.issues || []).length > 0;
    const isEmptyConfig = resolvedState.isEmptyGraph;

    if (isEmptyConfig) {
        return 'idle';
    }

    if (hasLocalIssues) {
        return 'setup';
    }

    return 'normal';
}

function setViewState(viewState, status, localState) {
    currentViewState = viewState;
    applyViewState(viewState, status, localState);
}

function getLocalPipelineState(status) {
    if (pipelineEditorState.nodes.length === 0 && (status?.graph?.nodes || []).length > 0) {
        seedPipelineModel(status);
    }
    const model = getPipelineGraphModel();
    const { issues } = buildPipelineGraphConfig(model);
    const isEmptyGraph = model.nodes.length === 0;
    return {
        nodesCount: model.nodes.length,
        issues,
        isEmptyGraph,
        hasInputs: model.nodes.some(node => node.kind === 'input'),
        hasOutputs: model.nodes.some(node => node.kind === 'output'),
        hasBuffers: model.nodes.some(node => node.kind === 'buffer'),
        hasCodecs: model.nodes.some(node => node.kind === 'processing')
    };
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

function applyViewState(viewState, status, localState) {
    if (viewState === 'offline') {
        showOfflineView();
        return;
    }

    if (viewState === 'idle') {
        showIdleView(status, localState);
        return;
    }
    
    if (viewState === 'setup') {
        showSetupView(status, localState);
        return;
    }
    
    showFullView();
}

function showOfflineView() {
    currentViewState = 'offline';
    togglePanels(false);
    const offlinePanel = document.getElementById('offlinePanel');
    const setupPanel = document.getElementById('setupPanel');
    const idlePanel = document.getElementById('idlePanel');
    offlinePanel.classList.remove('hidden');
    setupPanel.classList.add('hidden');
    if (idlePanel) {
        idlePanel.classList.add('hidden');
    }
}

function showIdleView(status, localState) {
    currentViewState = 'idle';
    togglePanels(false);
    const offlinePanel = document.getElementById('offlinePanel');
    const setupPanel = document.getElementById('setupPanel');
    const idlePanel = document.getElementById('idlePanel');
    if (offlinePanel) {
        offlinePanel.classList.add('hidden');
    }
    if (setupPanel) {
        setupPanel.classList.add('hidden');
    }
    if (idlePanel) {
        idlePanel.classList.remove('hidden');
    }
}

function showSetupView(status, localState) {
    currentViewState = 'setup';
    document.getElementById('offlinePanel').classList.add('hidden');
    const idlePanel = document.getElementById('idlePanel');
    if (idlePanel) {
        idlePanel.classList.add('hidden');
    }
    const setupPanel = document.getElementById('setupPanel');
    togglePanels(true);
    setupPanel.classList.remove('hidden');
    renderSetupGuide(status, localState);
    const validation = window.renderPipelineConfigPreview?.();
    updatePipelineValidationState(validation?.issues || []);
}

function showFullView() {
    currentViewState = 'normal';
    togglePanels(true);
    document.getElementById('offlinePanel').classList.add('hidden');
    const idlePanel = document.getElementById('idlePanel');
    if (idlePanel) {
        idlePanel.classList.add('hidden');
    }
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

function renderSetupGuide(status, localState) {
    const contextStepsElement = document.getElementById('pipelineContextHints');
    const contextNextStep = document.getElementById('pipelineContextNextStep');
    const issuesElement = document.getElementById('setupIssues');
    const issuesSection = document.getElementById('setupIssuesSection');
    const nextStepElement = document.getElementById('setupNextStep');
    const configurationIssues = status?.configuration_issues || [];
    const resolvedState = localState || getLocalPipelineState(status);
    const localIssues = resolvedState.issues || [];
    const hasInputs = resolvedState.hasInputs;
    const hasOutputs = resolvedState.hasOutputs;
    const hasCodecIssues = localIssues.some(issue => issue.type === 'codec-missing');
    const hasBufferIssues = localIssues.some(issue => issue.type === 'output-buffer-missing');
    const hasConnectionIssues = localIssues.some(issue => ['output-connection', 'service-connection', 'signal-compat', 'unconnected', 'input-start'].includes(issue.type));

    let nextStep = 'Konfiguration prüfen';
    if (!hasInputs) {
        nextStep = 'Input konfigurieren';
    } else if (!hasOutputs) {
        nextStep = 'Output konfigurieren';
    } else if (hasCodecIssues) {
        nextStep = 'Codec ergänzen';
    } else if (hasBufferIssues) {
        nextStep = 'Buffer verbinden';
    } else if (hasConnectionIssues) {
        nextStep = 'Verbindungen prüfen';
    } else if (configurationIssues.length > 0) {
        nextStep = 'Backend-Sync prüfen';
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
    if (hasCodecIssues) {
        steps.push('Codec-Nodes ergänzen und Outputs/Services damit verbinden.');
    }
    if (hasBufferIssues) {
        steps.push('Output mit einem Buffer verbinden.');
    }
    if (hasConnectionIssues) {
        steps.push('Verbindungen zwischen Inputs, Buffern und Outputs prüfen.');
    }
    if (configurationIssues.length > 0 && localIssues.length === 0) {
        steps.push('Backend-Konfiguration synchronisieren und Service neu laden.');
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
// UI Rendering Functions
const pipelineEditorState = {
    nodes: [],
    edges: [],
    nextNodeId: 1,
    selectedNodeId: null,
    selectedEdgeId: null,
    draggingNodeId: null,
    dragOffset: { x: 0, y: 0 },
    connectingFrom: null,
    tempEdgeId: null,
    eventsBound: false
};

let pipelineCatalog = [];
const pipelineDefaultTypes = {
    input: 'producer',
    buffer: 'buffer',
    processing: 'processor',
    service: 'flow',
    output: 'consumer'
};

const pipelineSignalTypes = {
    raw_pcm: {
        key: 'raw_pcm',
        label: 'Raw PCM'
    },
    encoded_ogg: {
        key: 'encoded_ogg',
        label: 'Encoded OGG'
    },
    peak_event: {
        key: 'peak_event',
        label: 'Peak Event'
    }
};

function renderAudioPipeline(status) {
    renderPipelineEditor(status);
}

function renderPipelineEditor(status) {
    const paletteContainer = document.getElementById('pipelinePalette');
    const nodesLayer = document.getElementById('pipelineNodes');
    const edgesLayer = document.getElementById('pipelineEdges');
    const hint = document.getElementById('pipelineCanvasHint');
    const emptyOverlay = document.getElementById('pipelineEmptyOverlay');

    if (!paletteContainer || !nodesLayer || !edgesLayer) {
        return;
    }

    seedPipelineModel(status);
    renderPalette(paletteContainer);
    renderNodes(nodesLayer);
    renderEdges(edgesLayer);
    updatePipelineInspector();
    updatePipelinePreview();

    const isEmpty = pipelineEditorState.nodes.length === 0;
    if (hint) {
        hint.textContent = isEmpty ? 'Drag input here' : '';
        hint.style.display = emptyOverlay ? 'none' : (isEmpty ? 'block' : 'none');
    }
    if (emptyOverlay) {
        emptyOverlay.classList.toggle('hidden', !isEmpty);
    }
    attachPipelineEvents();
}

function ensureNodeMetadata(node, options = {}) {
    if (!node) {
        return node;
    }
    const { allowInference = false } = options;
    if (node.type === '') {
        node.type = null;
    }
    if (node.backendType === '') {
        node.backendType = null;
    }
    if (!node.type && node.backendType) {
        node.type = node.backendType;
    }
    if (!node.backendType && node.type) {
        node.backendType = node.type;
    }
    if (!node.backendType && !node.type && pipelineDefaultTypes[node.kind]) {
        node.type = pipelineDefaultTypes[node.kind];
        node.backendType = pipelineDefaultTypes[node.kind];
    }
    if (allowInference && !node.backendType && !node.type) {
        const inferred = inferModuleType(node.kind, node.label);
        if (inferred) {
            node.type = inferred;
            node.backendType = inferred;
        }
    }
    return node;
}

function seedPipelineModel(status) {
    if (pipelineEditorState.nodes.length > 0) {
        return;
    }
    const graphNodes = status?.graph?.nodes || [];
    const graphEdges = status?.graph?.edges || [];

    if (graphNodes.length === 0) {
        return;
    }

    const columnX = {
        input: 40,
        buffer: 240,
        processing: 440,
        service: 440,
        output: 640
    };
    const rowCounters = {};

    pipelineEditorState.nodes = graphNodes.map((node) => {
        const row = rowCounters[node.kind] ?? 0;
        rowCounters[node.kind] = row + 1;
        return ensureNodeMetadata({
            id: node.id,
            label: node.label || node.id,
            kind: node.kind,
            type: node.type ?? null,
            backendType: node.backendType ?? null,
            x: columnX[node.kind] ?? 40,
            y: 40 + row * 120
        }, { allowInference: true });
    });

    pipelineEditorState.edges = graphEdges.map((edge, index) => ({
        id: `edge-${index}`,
        from: edge.from,
        to: edge.to
    }));
    pipelineEditorState.nextNodeId = graphNodes.length + 1;
}

function renderPalette(container) {
    container.innerHTML = pipelineCatalog.map(group => {
        const items = group.items.map(item => {
            const planned = item.supported === false;
            const plannedBadge = planned ? '<span class="palette-badge planned">Planned</span>' : '';
            return `
                <div class="pipeline-palette-item ${planned ? 'planned' : ''}"
                     draggable="${planned ? 'false' : 'true'}"
                     data-kind="${group.kind}"
                     data-type="${item.type ?? ''}"
                     data-backend-type="${item.backendType ?? item.type ?? ''}"
                     data-label="${item.label}"
                     data-planned="${planned}">
                    <span class="palette-badge">${item.type ?? group.kind}</span>
                    ${plannedBadge}
                    <span>${item.label}</span>
                </div>
            `;
        }).join('');
        return `
            <div class="pipeline-palette-group">
                <div class="pipeline-palette-group-title">${group.title}</div>
                ${items}
            </div>
        `;
    }).join('');
}

function renderNodes(container) {
    container.innerHTML = pipelineEditorState.nodes.map(node => `
        <div class="pipeline-node ${pipelineEditorState.selectedNodeId === node.id ? 'selected' : ''}"
             data-node-id="${node.id}"
             style="left:${node.x}px; top:${node.y}px;">
            <div class="pipeline-node-title">${node.label}</div>
            <div class="pipeline-node-meta">${node.kind}${node.type ? ` · ${node.type}` : ''}</div>
            <div class="pipeline-node-handles">
                <div class="node-handle input" data-handle="input" data-node-id="${node.id}"></div>
                <div class="node-handle output" data-handle="output" data-node-id="${node.id}"></div>
            </div>
        </div>
    `).join('');
}

function renderEdges(svg) {
    const canvas = document.getElementById('pipelineCanvas');
    if (!canvas) {
        return;
    }
    const canvasRect = canvas.getBoundingClientRect();
    svg.setAttribute('viewBox', `0 0 ${canvasRect.width} ${canvasRect.height}`);
    svg.setAttribute('width', canvasRect.width);
    svg.setAttribute('height', canvasRect.height);

    const nodesById = new Map(pipelineEditorState.nodes.map(node => [node.id, node]));
    const outgoing = pipelineEditorState.edges.reduce((map, edge) => {
        if (!map.has(edge.from)) {
            map.set(edge.from, []);
        }
        map.get(edge.from).push(edge);
        return map;
    }, new Map());

    const lines = pipelineEditorState.edges.map(edge => {
        const from = getNodeAnchor(edge.from, 'output', canvasRect);
        const to = getNodeAnchor(edge.to, 'input', canvasRect);
        if (!from || !to) {
            return '';
        }
        const signalType = getEdgeSignalType(edge, nodesById);
        const isCompatible = isEdgeCompatible(edge, nodesById);
        const signalClass = signalType ? `signal-${signalType}` : '';
        const invalidClass = isCompatible ? '' : 'invalid';
        const siblingEdges = outgoing.get(edge.from) || [];
        const needsBranching = siblingEdges.length > 1;
        const branchOffset = 40;
        let path = '';
        if (needsBranching) {
            const branchX = from.x + branchOffset;
            path = `M ${from.x} ${from.y} L ${branchX} ${from.y} L ${branchX} ${to.y} L ${to.x} ${to.y}`;
        } else {
            const midX = (from.x + to.x) / 2;
            path = `M ${from.x} ${from.y} C ${midX} ${from.y} ${midX} ${to.y} ${to.x} ${to.y}`;
        }
        const selectedClass = pipelineEditorState.selectedEdgeId === edge.id ? 'selected' : '';
        return `<path class="pipeline-edge ${signalClass} ${invalidClass} ${selectedClass}"
            data-edge-id="${edge.id}"
            data-edge-from="${edge.from}"
            data-edge-to="${edge.to}"
            d="${path}" />`;
    }).join('');

    const tempLine = pipelineEditorState.tempEdgeId
        ? `<line class="pipeline-edge temp" id="${pipelineEditorState.tempEdgeId}" />`
        : '';

    svg.innerHTML = lines + tempLine;
}

function getNodeAnchor(nodeId, handleType, canvasRect) {
    const nodeEl = document.querySelector(`.pipeline-node[data-node-id="${nodeId}"]`);
    if (!nodeEl) {
        return null;
    }
    const rect = nodeEl.getBoundingClientRect();
    const x = handleType === 'output' ? rect.right : rect.left;
    const y = rect.top + rect.height / 2;
    return {
        x: x - canvasRect.left,
        y: y - canvasRect.top
    };
}

function getNodeOutputSignalType(node) {
    if (!node) {
        return null;
    }
    switch (node.kind) {
        case 'input':
        case 'buffer':
            return pipelineSignalTypes.raw_pcm.key;
        case 'processing':
            return pipelineSignalTypes.encoded_ogg.key;
        case 'service':
            return pipelineSignalTypes.peak_event.key;
        default:
            return pipelineSignalTypes.raw_pcm.key;
    }
}

function getNodeInputSignalTypes(node) {
    if (!node) {
        return [];
    }
    switch (node.kind) {
        case 'buffer':
        case 'processing':
            return [pipelineSignalTypes.raw_pcm.key];
        case 'service':
            return [pipelineSignalTypes.raw_pcm.key, pipelineSignalTypes.encoded_ogg.key];
        case 'output':
            return [pipelineSignalTypes.encoded_ogg.key];
        default:
            return [];
    }
}

function getEdgeSignalType(edge, nodesById) {
    const fromNode = nodesById.get(edge.from);
    return getNodeOutputSignalType(fromNode);
}

function isEdgeCompatible(edge, nodesById) {
    const toNode = nodesById.get(edge.to);
    const expectedSignals = getNodeInputSignalTypes(toNode);
    if (!toNode || expectedSignals.length === 0) {
        return true;
    }
    const signalType = getEdgeSignalType(edge, nodesById);
    return expectedSignals.includes(signalType);
}

function attachPipelineEvents() {
    const paletteItems = document.querySelectorAll('.pipeline-palette-item');
    const canvas = document.getElementById('pipelineCanvas');
    const nodesLayer = document.getElementById('pipelineNodes');
    const edgesLayer = document.getElementById('pipelineEdges');

    paletteItems.forEach(item => {
        item.addEventListener('dragstart', (event) => {
            if (item.dataset.planned === 'true') {
                event.preventDefault();
                return;
            }
            event.dataTransfer.setData('text/plain', JSON.stringify({
                kind: item.dataset.kind,
                label: item.dataset.label,
                type: item.dataset.type || null,
                backendType: item.dataset.backendType || null
            }));
        });
    });

    if (!pipelineEditorState.eventsBound) {
        if (canvas) {
            canvas.addEventListener('dragover', (event) => {
                event.preventDefault();
            });

            canvas.addEventListener('drop', (event) => {
                event.preventDefault();
                const payload = event.dataTransfer.getData('text/plain');
                if (!payload) {
                    return;
                }
                const { kind, label, type, backendType } = JSON.parse(payload);
                const rect = canvas.getBoundingClientRect();
                addPipelineNode({
                    kind,
                    label: `Neues ${label}`,
                    type,
                    backendType,
                    x: event.clientX - rect.left - 60,
                    y: event.clientY - rect.top - 20
                });
            });

            canvas.addEventListener('pointerdown', (event) => {
                if (event.target.closest('.pipeline-node') || event.target.closest('.pipeline-edge')) {
                    return;
                }
                pipelineEditorState.selectedNodeId = null;
                pipelineEditorState.selectedEdgeId = null;
                renderNodes(document.getElementById('pipelineNodes'));
                renderEdges(document.getElementById('pipelineEdges'));
                updatePipelineInspector();
            });

            canvas.addEventListener('mousemove', (event) => {
                if (!pipelineEditorState.tempEdgeId || !pipelineEditorState.connectingFrom) {
                    return;
                }
                const rect = canvas.getBoundingClientRect();
                const from = getNodeAnchor(pipelineEditorState.connectingFrom, 'output', rect);
                const tempLine = document.getElementById(pipelineEditorState.tempEdgeId);
                if (from && tempLine) {
                    tempLine.setAttribute('x1', from.x);
                    tempLine.setAttribute('y1', from.y);
                    tempLine.setAttribute('x2', event.clientX - rect.left);
                    tempLine.setAttribute('y2', event.clientY - rect.top);
                }
            });
        }

        if (nodesLayer) {
            nodesLayer.addEventListener('pointerdown', handleNodePointerDown);
            nodesLayer.addEventListener('contextmenu', handleNodeContextMenu);
        }

        if (edgesLayer) {
            edgesLayer.addEventListener('pointerdown', handleEdgePointerDown);
            edgesLayer.addEventListener('contextmenu', handleEdgeContextMenu);
        }

        document.addEventListener('pointermove', handlePointerMove);
        document.addEventListener('pointerup', handlePointerUp);
        document.addEventListener('keydown', handlePipelineKeydown);
        document.addEventListener('pointerdown', handlePipelineContextClose);
        document.addEventListener('scroll', closePipelineContextMenu, true);
        pipelineEditorState.eventsBound = true;
    }
}

function handleNodePointerDown(event) {
    const handle = event.target.closest('.node-handle');
    const nodeElement = event.target.closest('.pipeline-node');

    if (handle) {
        const nodeId = handle.dataset.nodeId;
        const handleType = handle.dataset.handle;
        event.stopPropagation();
        if (handleType === 'output') {
            pipelineEditorState.connectingFrom = nodeId;
            pipelineEditorState.tempEdgeId = 'temp-edge';
            renderEdges(document.getElementById('pipelineEdges'));
        } else if (handleType === 'input' && pipelineEditorState.connectingFrom) {
            addPipelineEdge(pipelineEditorState.connectingFrom, nodeId);
        }
        return;
    }

    if (nodeElement) {
        const nodeId = nodeElement.dataset.nodeId;
        pipelineEditorState.selectedNodeId = nodeId;
        pipelineEditorState.selectedEdgeId = null;
        const rect = nodeElement.getBoundingClientRect();
        pipelineEditorState.draggingNodeId = nodeId;
        pipelineEditorState.dragOffset = {
            x: event.clientX - rect.left,
            y: event.clientY - rect.top
        };
        renderNodes(document.getElementById('pipelineNodes'));
        updatePipelineInspector();
    }
}

function handleEdgePointerDown(event) {
    const edgeElement = event.target.closest('.pipeline-edge');
    const edgeId = edgeElement?.dataset?.edgeId;
    if (!edgeId) {
        return;
    }
    event.stopPropagation();
    pipelineEditorState.selectedEdgeId = edgeId;
    pipelineEditorState.selectedNodeId = null;
    renderNodes(document.getElementById('pipelineNodes'));
    renderEdges(document.getElementById('pipelineEdges'));
    updatePipelineInspector();
}

function handleNodeContextMenu(event) {
    const nodeElement = event.target.closest('.pipeline-node');
    if (!nodeElement) {
        return;
    }
    event.preventDefault();
    const nodeId = nodeElement.dataset.nodeId;
    pipelineEditorState.selectedNodeId = nodeId;
    pipelineEditorState.selectedEdgeId = null;
    renderNodes(document.getElementById('pipelineNodes'));
    renderEdges(document.getElementById('pipelineEdges'));
    updatePipelineInspector();
    showPipelineContextMenu(event.clientX, event.clientY, [
        {
            label: 'Node entfernen',
            action: () => removePipelineNode(nodeId)
        }
    ]);
}

function handleEdgeContextMenu(event) {
    const edgeElement = event.target.closest('.pipeline-edge');
    const edgeId = edgeElement?.dataset?.edgeId;
    if (!edgeId) {
        return;
    }
    event.preventDefault();
    pipelineEditorState.selectedEdgeId = edgeId;
    pipelineEditorState.selectedNodeId = null;
    renderNodes(document.getElementById('pipelineNodes'));
    renderEdges(document.getElementById('pipelineEdges'));
    updatePipelineInspector();
    showPipelineContextMenu(event.clientX, event.clientY, [
        {
            label: 'Kante entfernen',
            action: () => removePipelineEdge(edgeId)
        }
    ]);
}

function handlePipelineKeydown(event) {
    if (event.key !== 'Delete' && event.key !== 'Backspace') {
        return;
    }
    const target = event.target;
    const isEditable = target instanceof HTMLElement
        && (target.tagName === 'INPUT'
            || target.tagName === 'TEXTAREA'
            || target.isContentEditable);
    if (isEditable) {
        return;
    }
    if (pipelineEditorState.selectedNodeId) {
        event.preventDefault();
        removePipelineNode(pipelineEditorState.selectedNodeId);
        return;
    }
    if (pipelineEditorState.selectedEdgeId) {
        event.preventDefault();
        removePipelineEdge(pipelineEditorState.selectedEdgeId);
    }
}

function ensurePipelineContextMenu() {
    let menu = document.getElementById('pipelineContextMenu');
    if (!menu) {
        menu = document.createElement('div');
        menu.id = 'pipelineContextMenu';
        menu.className = 'pipeline-context-menu hidden';
        document.body.appendChild(menu);
    }
    return menu;
}

function showPipelineContextMenu(x, y, items) {
    const menu = ensurePipelineContextMenu();
    menu.innerHTML = items.map((item, index) => `
        <button type="button" class="pipeline-context-menu-item" data-index="${index}">
            ${item.label}
        </button>
    `).join('');
    menu.querySelectorAll('.pipeline-context-menu-item').forEach(button => {
        const index = Number(button.dataset.index);
        button.addEventListener('click', () => {
            items[index]?.action?.();
            closePipelineContextMenu();
        });
    });
    menu.classList.remove('hidden');
    const menuRect = menu.getBoundingClientRect();
    const maxLeft = Math.max(8, window.innerWidth - menuRect.width - 8);
    const maxTop = Math.max(8, window.innerHeight - menuRect.height - 8);
    menu.style.left = `${Math.min(x, maxLeft)}px`;
    menu.style.top = `${Math.min(y, maxTop)}px`;
}

function closePipelineContextMenu() {
    const menu = document.getElementById('pipelineContextMenu');
    if (!menu) {
        return;
    }
    menu.classList.add('hidden');
}

function handlePipelineContextClose(event) {
    const menu = document.getElementById('pipelineContextMenu');
    if (!menu || menu.classList.contains('hidden')) {
        return;
    }
    if (event.target.closest('.pipeline-context-menu')) {
        return;
    }
    closePipelineContextMenu();
}

function handlePointerMove(event) {
    if (!pipelineEditorState.draggingNodeId) {
        return;
    }
    const canvas = document.getElementById('pipelineCanvas');
    if (!canvas) {
        return;
    }
    const rect = canvas.getBoundingClientRect();
    const node = pipelineEditorState.nodes.find(item => item.id === pipelineEditorState.draggingNodeId);
    if (!node) {
        return;
    }
    node.x = event.clientX - rect.left - pipelineEditorState.dragOffset.x;
    node.y = event.clientY - rect.top - pipelineEditorState.dragOffset.y;
    const nodeElement = document.querySelector(`.pipeline-node[data-node-id="${node.id}"]`);
    if (nodeElement) {
        nodeElement.style.left = `${node.x}px`;
        nodeElement.style.top = `${node.y}px`;
    }
    renderEdges(document.getElementById('pipelineEdges'));
}

function handlePointerUp(event) {
    const handle = event?.target?.closest('.node-handle');
    if (handle && handle.dataset.handle === 'input' && pipelineEditorState.connectingFrom) {
        addPipelineEdge(pipelineEditorState.connectingFrom, handle.dataset.nodeId);
    }
    if (pipelineEditorState.draggingNodeId) {
        pipelineEditorState.draggingNodeId = null;
        updatePipelinePreview();
    }
    if (pipelineEditorState.connectingFrom) {
        pipelineEditorState.connectingFrom = null;
        pipelineEditorState.tempEdgeId = null;
        renderEdges(document.getElementById('pipelineEdges'));
    }
}

function addPipelineNode({ kind, label, type, backendType, x, y }) {
    const id = `local-${pipelineEditorState.nextNodeId++}`;
    pipelineEditorState.nodes.push(ensureNodeMetadata({
        id,
        kind,
        label,
        type: type ?? null,
        backendType: backendType ?? null,
        x: Math.max(20, x),
        y: Math.max(20, y)
    }));
    pipelineEditorState.selectedNodeId = id;
    pipelineEditorState.selectedEdgeId = null;
    renderNodes(document.getElementById('pipelineNodes'));
    renderEdges(document.getElementById('pipelineEdges'));
    updatePipelineInspector();
    updatePipelinePreview();
}

function addPipelineEdge(from, to) {
    if (from === to) {
        return;
    }
    const exists = pipelineEditorState.edges.some(edge => edge.from === from && edge.to === to);
    if (exists) {
        return;
    }
    const edgeId = `edge-${Date.now()}`;
    pipelineEditorState.edges.push({
        id: edgeId,
        from,
        to
    });
    pipelineEditorState.selectedEdgeId = edgeId;
    pipelineEditorState.selectedNodeId = null;
    pipelineEditorState.connectingFrom = null;
    pipelineEditorState.tempEdgeId = null;
    renderEdges(document.getElementById('pipelineEdges'));
    updatePipelinePreview();
}

function removePipelineNode(nodeId) {
    const index = pipelineEditorState.nodes.findIndex(node => node.id === nodeId);
    if (index === -1) {
        return;
    }
    pipelineEditorState.nodes.splice(index, 1);
    pipelineEditorState.edges = pipelineEditorState.edges.filter(edge => edge.from !== nodeId && edge.to !== nodeId);
    if (pipelineEditorState.selectedNodeId === nodeId) {
        pipelineEditorState.selectedNodeId = null;
    }
    if (pipelineEditorState.selectedEdgeId) {
        const stillExists = pipelineEditorState.edges.some(edge => edge.id === pipelineEditorState.selectedEdgeId);
        if (!stillExists) {
            pipelineEditorState.selectedEdgeId = null;
        }
    }
    renderNodes(document.getElementById('pipelineNodes'));
    renderEdges(document.getElementById('pipelineEdges'));
    updatePipelineInspector();
    updatePipelinePreview();
}

function removePipelineEdge(edgeRef) {
    if (!edgeRef) {
        return;
    }
    const edges = pipelineEditorState.edges;
    let index = -1;
    if (typeof edgeRef === 'string') {
        index = edges.findIndex(edge => edge.id === edgeRef);
    } else if (typeof edgeRef === 'object') {
        if (edgeRef.id) {
            index = edges.findIndex(edge => edge.id === edgeRef.id);
        } else if (edgeRef.from && edgeRef.to) {
            index = edges.findIndex(edge => edge.from === edgeRef.from && edge.to === edgeRef.to);
        }
    }
    if (index === -1) {
        return;
    }
    const [removed] = edges.splice(index, 1);
    if (removed && pipelineEditorState.selectedEdgeId === removed.id) {
        pipelineEditorState.selectedEdgeId = null;
    }
    renderEdges(document.getElementById('pipelineEdges'));
    updatePipelinePreview();
}

function updatePipelineInspector() {
    const inspector = document.getElementById('pipelineInspector');
    if (!inspector) {
        return;
    }
    const selected = pipelineEditorState.nodes.find(node => node.id === pipelineEditorState.selectedNodeId);
    if (!selected) {
        inspector.innerHTML = `
            <div class="empty-state">
                <div class="icon">🧩</div>
                <div class="message">Wähle ein Modul aus.</div>
            </div>`;
        return;
    }
    const connectedEdges = pipelineEditorState.edges.filter(edge => edge.from === selected.id || edge.to === selected.id);
    ensureNodeMetadata(selected);
    const resolvedType = selected.type || selected.backendType;
    const missingBackendType = !resolvedType && selected.kind !== 'buffer';
    const catalogEntry = findCatalogEntry(selected.kind, resolvedType);
    const typeOptions = selected.kind === 'buffer' ? getCatalogTypesForKind('buffer') : [];
    const configFields = mergeConfigFields(
        baseConfigFieldsForKind(selected.kind),
        catalogEntry?.configFields ?? []
    );
    const statusLabel = !catalogEntry
        ? 'Unbekannt'
        : (catalogEntry.supported === false ? 'Planned' : 'Supported');
    const statusClass = !catalogEntry
        ? ''
        : (catalogEntry.supported === false ? 'planned' : 'supported');
    inspector.innerHTML = `
        <div>
            <div class="label">Label</div>
            <input class="pipeline-inspector-input" type="text" id="pipelineInspectorLabel" />
        </div>
        ${missingBackendType ? `
            <div class="pipeline-warning-banner">
                ⚠️ Dieser Node hat keinen Typ. Bitte wähle einen gültigen Typ in der Pipeline-Konfiguration.
            </div>
        ` : ''}
        ${typeOptions.length > 0 ? `
            <div>
                <div class="label">Typ</div>
                <select class="pipeline-inspector-input" id="pipelineInspectorType">
                    ${typeOptions.map(option => `
                        <option value="${option.type}">${option.label}</option>
                    `).join('')}
                </select>
            </div>
        ` : `
            <div>
                <div class="label">Typ</div>
                <div class="value">${selected.kind}${resolvedType ? ` · ${resolvedType}` : ''}</div>
            </div>
        `}
        <div>
            <div class="label">Status</div>
            <div class="value pipeline-config-status ${statusClass}">${statusLabel}</div>
        </div>
        <div>
            <div class="label">Node-ID</div>
            <div class="value">${selected.id}</div>
        </div>
        <div>
            <div class="label">Verbindungen</div>
            <div class="value">${connectedEdges.length}</div>
        </div>
        <div>
            <div class="label">Konfiguration</div>
            ${configFields.length === 0 ? '<div class="pipeline-inspector-hint">Keine spezifischen Felder.</div>' : `
                <div class="pipeline-config-fields">
                    ${configFields.map(field => `
                        <div class="pipeline-config-field">
                            <span class="pipeline-config-key">${field.key}</span>
                            <span class="pipeline-config-value">${field.example ?? '—'}</span>
                            ${field.required ? '<span class="pipeline-config-required">required</span>' : ''}
                        </div>
                    `).join('')}
                </div>
            `}
        </div>
    `;
    const labelInput = inspector.querySelector('#pipelineInspectorLabel');
    if (labelInput) {
        labelInput.value = selected.label;
        labelInput.oninput = (event) => {
            selected.label = event.target.value;
            renderNodes(document.getElementById('pipelineNodes'));
            updatePipelinePreview();
        };
    }
    const typeSelect = inspector.querySelector('#pipelineInspectorType');
    if (typeSelect) {
        typeSelect.value = resolvedType || '';
        typeSelect.onchange = (event) => {
            const nextType = event.target.value || null;
            selected.type = nextType;
            selected.backendType = nextType;
            ensureNodeMetadata(selected);
            renderNodes(document.getElementById('pipelineNodes'));
            updatePipelinePreview();
            updatePipelineInspector();
        };
    }
}

function updatePipelinePreview() {
    renderPipelineConfigPreview();
    refreshSetupFlowFromLocalState();
}

function refreshSetupFlowFromLocalState() {
    if (!currentStatus) {
        return;
    }
    const localState = getLocalPipelineState(currentStatus);
    const viewState = determineViewState(currentStatus, localState);
    if (viewState !== currentViewState) {
        setViewState(viewState, currentStatus, localState);
        return;
    }
    if (viewState === 'setup') {
        renderSetupGuide(currentStatus, localState);
    }
}

function renderPipelineConfigPreview() {
    const preview = document.getElementById('setupPreview');
    if (!preview) {
        return null;
    }
    const validation = document.getElementById('setupPreviewValidation');
    const model = getPipelineGraphModel();
    const { config, issues } = buildPipelineGraphConfig(model);
    preview.textContent = formatConfigOutput(config);
    renderPipelineValidation(validation, issues);
    window.updatePipelineValidationState?.(issues);
    return { config, issues };
}

function getPipelineGraphModel() {
    return {
        nodes: pipelineEditorState.nodes.map(node => {
            ensureNodeMetadata(node);
            return {
                id: node.id,
                label: node.label,
                kind: node.kind,
                type: node.type,
                backendType: node.backendType
            };
        }),
        edges: pipelineEditorState.edges.map(edge => ({
            from: edge.from,
            to: edge.to
        }))
    };
}

function resolveBackendType(node, fallback) {
    return node?.backendType || node?.type || fallback;
}

function buildPipelineGraphConfig(model) {
    const nodes = model?.nodes || [];
    const edges = model?.edges || [];
    const issues = [];
    const formatNodeLabel = (node) => {
        if (!node) {
            return 'Node';
        }
        const resolved = resolveBackendType(node, null);
        if (resolved) {
            return `${node.label} (${resolved})`;
        }
        return node.label;
    };

    if (nodes.length === 0) {
        return {
            config: {
                ringbuffers: {
                    main: {
                        slots: 6000,
                        prealloc_samples: 9600
                    }
                },
                inputs: {},
                outputs: {},
                services: {},
                codecs: {}
            },
            issues: [{ type: 'graph-empty', message: 'Keine Pipeline definiert.' }]
        };
    }
    const nodesById = new Map(nodes.map(node => [node.id, node]));
    const incoming = new Map();
    const outgoing = new Map();

    nodes.forEach(node => {
        incoming.set(node.id, []);
        outgoing.set(node.id, []);
    });

    edges.forEach(edge => {
        if (!incoming.has(edge.to)) {
            incoming.set(edge.to, []);
        }
        if (!outgoing.has(edge.from)) {
            outgoing.set(edge.from, []);
        }
        incoming.get(edge.to).push(edge.from);
        outgoing.get(edge.from).push(edge.to);
    });

    edges.forEach(edge => {
        const fromNode = nodesById.get(edge.from);
        const toNode = nodesById.get(edge.to);
        if (!fromNode || !toNode) {
            return;
        }
        const signalType = getEdgeSignalType(edge, nodesById);
        const isCompatible = isEdgeCompatible(edge, nodesById);
        if (!isCompatible) {
            issues.push({
                type: 'signal-compat',
                message: `Signal "${signalType}" passt nicht zu "${formatNodeLabel(toNode)}".`
            });
        }
    });

    const inputNodes = nodes.filter(node => node.kind === 'input');
    const bufferNodes = nodes.filter(node => node.kind === 'buffer');
    const outputNodes = nodes.filter(node => node.kind === 'output');
    const serviceNodes = nodes.filter(node => node.kind === 'service');
    const codecNodes = nodes.filter(node => node.kind === 'processing');

    if (inputNodes.length === 0) {
        issues.push({ type: 'input', message: 'Keine Input-Nodes vorhanden.' });
    }

    inputNodes.forEach(node => {
        if ((incoming.get(node.id) || []).length > 0) {
            issues.push({
                type: 'input-start',
                message: `Input "${formatNodeLabel(node)}" hat eingehende Verbindungen. Die Pipeline muss mit Input starten.`
            });
        }
    });

    nodes.forEach(node => {
        const inEdges = incoming.get(node.id) || [];
        const outEdges = outgoing.get(node.id) || [];
        if (inEdges.length === 0 && outEdges.length === 0) {
            issues.push({
                type: 'unconnected',
                message: `Node "${formatNodeLabel(node)}" ist nicht verbunden.`
            });
        }
    });

    const ringbufferDefaults = {
        slots: 6000,
        prealloc_samples: 9600
    };
    const ringbufferIdMap = allocateNodeIds(bufferNodes, 'buffer');
    const ringbufferEntries = {};
    bufferNodes.forEach(node => {
        ringbufferEntries[ringbufferIdMap.get(node.id)] = { ...ringbufferDefaults };
    });
    if (bufferNodes.length === 0) {
        ringbufferEntries.main = { ...ringbufferDefaults };
    }
    const primaryRingbufferId = bufferNodes.length > 0
        ? ringbufferIdMap.get(bufferNodes[0].id)
        : 'main';

    const inputIdMap = allocateNodeIds(inputNodes, 'input');
    const outputIdMap = allocateNodeIds(outputNodes, 'output');
    const serviceIdMap = allocateNodeIds(serviceNodes, 'service');
    const codecIdMap = allocateNodeIds(codecNodes, 'codec');

    const codecEntries = {};
    codecNodes.forEach(node => {
        const codecId = codecIdMap.get(node.id);
        const codecType = resolveBackendType(node, 'pcm');
        codecEntries[codecId] = buildCodecDefaults(codecType);
    });

    const inputs = {};
    inputNodes.forEach(node => {
        const inputId = inputIdMap.get(node.id);
        const inputType = resolveBackendType(node, 'srt');
        const bufferNode = findDownstreamNode(node.id, incoming, outgoing, nodesById, candidate => candidate.kind === 'buffer');
        const bufferId = bufferNode ? ringbufferIdMap.get(bufferNode.id) : primaryRingbufferId;
        inputs[inputId] = buildInputConfig(inputType, bufferId);
    });

    const outputs = {};
    outputNodes.forEach(node => {
        const outputId = outputIdMap.get(node.id);
        const outputType = resolveBackendType(node, 'srt_out');
        const upstreamInput = findUpstreamNode(node.id, incoming, nodesById, candidate => candidate.kind === 'input');
        const upstreamBuffer = findUpstreamNode(node.id, incoming, nodesById, candidate => candidate.kind === 'buffer');
        const upstreamCodec = findUpstreamNode(node.id, incoming, nodesById, candidate => candidate.kind === 'processing');
        const bufferId = upstreamBuffer ? ringbufferIdMap.get(upstreamBuffer.id) : primaryRingbufferId;
        if (!upstreamInput && !upstreamBuffer) {
            issues.push({
                type: 'output-connection',
                message: `Output "${formatNodeLabel(node)}" ist nicht mit Input oder Buffer verbunden.`
            });
        }
        if (!upstreamBuffer) {
            issues.push({
                type: 'output-buffer-missing',
                message: `Output "${formatNodeLabel(node)}" hat keinen Buffer.`
            });
        }
        if (!upstreamCodec) {
            issues.push({
                type: 'codec-missing',
                message: `Output "${formatNodeLabel(node)}" hat keine Codec-Zuweisung.`
            });
        }
        outputs[outputId] = buildOutputConfig({
            outputType,
            bufferId,
            inputId: upstreamInput ? inputIdMap.get(upstreamInput.id) : null,
            codecId: upstreamCodec ? codecIdMap.get(upstreamCodec.id) : null
        });
    });

    const services = {};
    serviceNodes.forEach(node => {
        const serviceId = serviceIdMap.get(node.id);
        const serviceType = resolveBackendType(node, 'audio_http');
        const upstreamInput = findUpstreamNode(node.id, incoming, nodesById, candidate => candidate.kind === 'input');
        const upstreamBuffer = findUpstreamNode(node.id, incoming, nodesById, candidate => candidate.kind === 'buffer');
        const upstreamCodec = findUpstreamNode(node.id, incoming, nodesById, candidate => candidate.kind === 'processing');
        if (!upstreamInput && !upstreamBuffer) {
            issues.push({
                type: 'service-connection',
                message: `Service "${formatNodeLabel(node)}" ist nicht mit Input oder Buffer verbunden.`
            });
        }
        if (!upstreamCodec) {
            issues.push({
                type: 'codec-missing',
                message: `Service "${formatNodeLabel(node)}" hat keine Codec-Zuweisung.`
            });
        }
        services[serviceId] = buildServiceConfig({
            serviceType,
            bufferId: upstreamBuffer ? ringbufferIdMap.get(upstreamBuffer.id) : primaryRingbufferId,
            inputId: upstreamInput ? inputIdMap.get(upstreamInput.id) : null,
            codecId: upstreamCodec ? codecIdMap.get(upstreamCodec.id) : null
        });
    });

    const config = {
        ringbuffers: ringbufferEntries,
        inputs,
        outputs,
        services,
        codecs: codecEntries
    };

    return { config, issues };
}

function allocateNodeIds(nodes, fallbackPrefix) {
    const used = new Set();
    const map = new Map();
    nodes.forEach((node, index) => {
        const base = slugify(node.label || node.id) || `${fallbackPrefix}_${index + 1}`;
        let candidate = base;
        let counter = 2;
        while (used.has(candidate)) {
            candidate = `${base}_${counter++}`;
        }
        used.add(candidate);
        map.set(node.id, candidate);
    });
    return map;
}

function slugify(value) {
    return (value || '')
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, '_')
        .replace(/^_+|_+$/g, '');
}

function findUpstreamNode(startId, incoming, nodesById, predicate) {
    const visited = new Set();
    const queue = [...(incoming.get(startId) || [])];
    while (queue.length > 0) {
        const current = queue.shift();
        if (visited.has(current)) {
            continue;
        }
        visited.add(current);
        const node = nodesById.get(current);
        if (node && predicate(node)) {
            return node;
        }
        (incoming.get(current) || []).forEach(next => queue.push(next));
    }
    return null;
}

function findDownstreamNode(startId, incoming, outgoing, nodesById, predicate) {
    const visited = new Set();
    const queue = [...(outgoing.get(startId) || [])];
    while (queue.length > 0) {
        const current = queue.shift();
        if (visited.has(current)) {
            continue;
        }
        visited.add(current);
        const node = nodesById.get(current);
        if (node && predicate(node)) {
            return node;
        }
        (outgoing.get(current) || []).forEach(next => queue.push(next));
    }
    return null;
}

function buildInputConfig(inputType, bufferId) {
    const config = {
        type: inputType,
        enabled: true,
        buffer: bufferId
    };
    if (inputType === 'srt') {
        config.listen = '0.0.0.0:9000';
        config.latency_ms = 200;
        config.streamid = 'airlift';
    } else if (inputType === 'http_stream' || inputType === 'icecast') {
        config.url = 'https://example.com/stream.ogg';
    } else if (inputType === 'alsa') {
        config.device = 'hw:0,0';
    } else if (inputType === 'file_in') {
        config.path = '/path/to/audio.wav';
    }
    return config;
}

function buildOutputConfig({ outputType, bufferId, inputId, codecId }) {
    const config = {
        type: outputType,
        enabled: true,
        buffer: bufferId
    };
    if (inputId) {
        config.input = inputId;
    }
    if (codecId) {
        config.codec_id = codecId;
    }
    if (outputType === 'srt_out') {
        config.target = 'example.com:9000';
        config.latency_ms = 200;
    } else if (outputType === 'icecast_out') {
        config.host = 'icecast.local';
        config.port = 8000;
        config.mount = '/airlift';
        config.user = 'source';
        config.password = 'hackme';
        config.bitrate = 128000;
        config.name = 'Airlift Node';
        config.description = 'Live-Stream aus dem Ringbuffer';
        config.genre = 'news';
        config.public = false;
    } else if (outputType === 'file_out') {
        config.wav_dir = '/opt/rfm/airlift-node/aircheck/wav';
        config.retention_days = 7;
    }
    return config;
}

function buildServiceConfig({ serviceType, bufferId, inputId, codecId }) {
    const config = {
        type: serviceType,
        enabled: true
    };
    if (bufferId) {
        config.buffer = bufferId;
    }
    if (inputId) {
        config.input = inputId;
    }
    if (codecId) {
        config.codec_id = codecId;
    }
    if (serviceType === 'audio_http') {
        config.buffer = bufferId;
        config.codec_id = codecId;
    } else if (serviceType === 'monitor' || serviceType === 'monitoring') {
        // no additional fields
    } else if (serviceType === 'peak_analyzer') {
        config.buffer = bufferId;
        config.interval_ms = 30000;
    } else if (serviceType === 'influx_out') {
        config.url = 'https://influx.example';
        config.db = 'airlift';
        config.interval_ms = 30000;
    } else if (serviceType === 'broadcast_http') {
        config.url = 'https://example.com/notify';
        config.interval_ms = 30000;
    } else if (serviceType === 'file_out') {
        config.codec_id = codecId;
        config.wav_dir = '/opt/rfm/airlift-node/aircheck/wav';
        config.retention_days = 7;
    }
    return config;
}

function buildCodecDefaults(codecType) {
    const config = {
        type: codecType,
        sample_rate: 48000,
        channels: 2
    };
    if (codecType === 'opus_ogg') {
        config.frame_size_ms = 20;
        config.bitrate = 128000;
        config.application = 'audio';
    } else if (codecType === 'opus_webrtc') {
        config.bitrate = 128000;
        config.application = 'audio';
    } else if (codecType === 'mp3') {
        config.bitrate = 128000;
    } else if (codecType === 'vorbis') {
        config.quality = 0.4;
    }
    return config;
}

function getCatalogTypesForKind(kind) {
    const group = pipelineCatalog.find(entry => entry.kind === kind);
    if (!group || !Array.isArray(group.items)) {
        return [];
    }
    return group.items
        .map(item => {
            const type = item.type ?? item.backendType;
            if (!type) {
                return null;
            }
            return {
                type,
                label: item.label || type
            };
        })
        .filter(Boolean);
}

function inferModuleType(kind, label) {
    const lower = (label || '').toLowerCase();
    if (kind === 'input') {
        if (lower.includes('srt')) return 'srt';
        if (lower.includes('icecast')) return 'icecast';
        if (lower.includes('http')) return 'http_stream';
        if (lower.includes('alsa')) return 'alsa';
        if (lower.includes('file')) return 'file_in';
    }
    if (kind === 'output') {
        if (lower.includes('srt')) return 'srt_out';
        if (lower.includes('icecast')) return 'icecast_out';
        if (lower.includes('udp')) return 'udp_out';
        if (lower.includes('file')) return 'file_out';
    }
    if (kind === 'service') {
        if (lower.includes('audio')) return 'audio_http';
        if (lower.includes('monitoring')) return 'monitoring';
        if (lower.includes('monitor')) return 'monitor';
        if (lower.includes('peak')) return 'peak_analyzer';
        if (lower.includes('influx')) return 'influx_out';
        if (lower.includes('broadcast')) return 'broadcast_http';
        if (lower.includes('file')) return 'file_out';
    }
    if (kind === 'processing') {
        if (lower.includes('webrtc')) return 'opus_webrtc';
        if (lower.includes('opus')) return 'opus_ogg';
        if (lower.includes('mp3')) return 'mp3';
        if (lower.includes('vorbis')) return 'vorbis';
        if (lower.includes('aac')) return 'aac_lc';
        if (lower.includes('flac')) return 'flac';
        if (lower.includes('pcm')) return 'pcm';
    }
    return null;
}

function findCatalogEntry(kind, moduleType) {
    if (kind === 'buffer') {
        return pipelineCatalog
            .find(group => group.kind === 'buffer')
            ?.items[0] ?? null;
    }
    return pipelineCatalog
        .flatMap(group => group.items.map(item => ({ ...item, kind: group.kind })))
        .find(item => item.kind === kind && item.type === moduleType) || null;
}

function baseConfigFieldsForKind(kind) {
    if (kind === 'input') {
        return [
            { key: 'type', required: true, example: 'srt' },
            { key: 'enabled', required: true, example: 'true' },
            { key: 'buffer', required: true, example: 'main' }
        ];
    }
    if (kind === 'output') {
        return [
            { key: 'type', required: true, example: 'srt_out' },
            { key: 'enabled', required: true, example: 'true' },
            { key: 'input', required: true, example: 'input_main' },
            { key: 'buffer', required: true, example: 'main' },
            { key: 'codec_id', required: true, example: 'codec_opus_ogg' }
        ];
    }
    if (kind === 'service') {
        return [
            { key: 'type', required: true, example: 'audio_http' },
            { key: 'enabled', required: true, example: 'true' },
            { key: 'input', required: false, example: 'input_main' },
            { key: 'buffer', required: false, example: 'main' },
            { key: 'codec_id', required: false, example: 'codec_opus_ogg' }
        ];
    }
    if (kind === 'processing') {
        return [
            { key: 'type', required: true, example: 'opus_ogg' },
            { key: 'sample_rate', required: false, example: '48000' },
            { key: 'channels', required: false, example: '2' }
        ];
    }
    if (kind === 'buffer') {
        return [
            { key: 'slots', required: true, example: '6000' },
            { key: 'prealloc_samples', required: true, example: '9600' }
        ];
    }
    return [];
}

function mergeConfigFields(baseFields, specificFields) {
    const merged = [];
    const seen = new Set();
    const addFields = (fields) => {
        fields.forEach(field => {
            if (seen.has(field.key)) {
                return;
            }
            seen.add(field.key);
            merged.push(field);
        });
    };
    addFields(baseFields);
    addFields(specificFields);
    return merged;
}

function formatConfigOutput(config) {
    const json = JSON.stringify(config, null, 2);
    const toml = formatConfigAsToml(config);
    return `JSON\n${json}\n\nTOML\n${toml}`;
}

function formatConfigAsToml(config) {
    const sections = [];
    const addSection = (title, entries) => {
        Object.entries(entries || {}).forEach(([key, values]) => {
            sections.push(`[${title}.${key}]`);
            Object.entries(values).forEach(([field, value]) => {
                if (value === undefined || value === null) {
                    return;
                }
                sections.push(`${field} = ${tomlValue(value)}`);
            });
            sections.push('');
        });
    };

    addSection('ringbuffers', config.ringbuffers);
    addSection('inputs', config.inputs);
    addSection('outputs', config.outputs);
    addSection('services', config.services);
    addSection('codecs', config.codecs);

    return sections.join('\n').trim();
}

function tomlValue(value) {
    if (typeof value === 'string') {
        return `"${value.replace(/"/g, '\\"')}"`;
    }
    if (typeof value === 'boolean' || typeof value === 'number') {
        return String(value);
    }
    if (Array.isArray(value)) {
        return `[${value.map(item => tomlValue(item)).join(', ')}]`;
    }
    return `"${String(value)}"`;
}

function renderPipelineValidation(container, issues) {
    if (!container) {
        return;
    }
    if (!issues || issues.length === 0) {
        container.innerHTML = '<div class="validation-item ok">✅ Keine Validierungsfehler gefunden.</div>';
        return;
    }
    container.innerHTML = issues.map(issue => `
        <div class="validation-item error">⚠️ ${issue.message}</div>
    `).join('');
}

window.renderPipelineConfigPreview = renderPipelineConfigPreview;

function renderPipelineNode(node, modulesById, showStats = true) {
    const moduleSnapshot = modulesById.get(node.id);
    const statusInfo = getNodeStatus(node, moduleSnapshot);
    const type = node.kind;
    
    let statsHTML = '';
    if (showStats && moduleSnapshot) {
        statsHTML = `
            <div class="module-stats">
                <div class="module-stat">
                    <div class="stat-label">RX</div>
                    <div class="stat-value">${formatCompactNumber(moduleSnapshot.runtime.counters.rx)}</div>
                </div>
                <div class="module-stat">
                    <div class="stat-label">TX</div>
                    <div class="stat-value">${formatCompactNumber(moduleSnapshot.runtime.counters.tx)}</div>
                </div>
                <div class="module-stat">
                    <div class="stat-label">Aktiv</div>
                    <div class="stat-value">${formatDurationCompact(Date.now() - moduleSnapshot.runtime.last_activity_ms)}</div>
                </div>
                <div class="module-stat">
                    <div class="stat-label">Errors</div>
                    <div class="stat-value">${moduleSnapshot.runtime.counters.errors}</div>
                </div>
            </div>
        `;
    }
    
    return `
        <div class="module-card ${type}">
            <div class="module-header">
                <span class="module-type type-${type}">${type}</span>
                <span class="module-status">
                    <span class="status-dot ${statusInfo.class}"></span>
                    <span>${statusInfo.label}</span>
                </span>
            </div>
            <div class="module-title" title="${node.label}">${node.label}</div>
            <div class="module-id">${formatNodeId(node.id)}</div>
            ${statsHTML}
        </div>
    `;
}

function getModuleStatus(runtime) {
    if (!runtime.enabled) return { class: 'inactive', label: 'Aus', text: 'Deaktiviert' };
    if (!runtime.running) return { class: 'standby', label: 'Bereit', text: 'Bereit' };
    if (runtime.connected === false) return { class: 'ready', label: 'Bereit', text: 'Getrennt' };
    return { class: 'active', label: 'Aktiv', text: 'Aktiv' };
}

function getNodeStatus(node, moduleSnapshot) {
    if (moduleSnapshot) {
        return getModuleStatus(moduleSnapshot.runtime);
    }
    if (node.kind === 'service') {
        return { class: 'active', label: 'Service', text: 'Service' };
    }
    if (node.kind === 'processing') {
        return { class: 'standby', label: 'Codec', text: 'Codec' };
    }
    return { class: 'standby', label: 'Aktiv', text: 'Aktiv' };
}

function formatNodeId(nodeId) {
    if (nodeId.startsWith('service:')) return nodeId.replace('service:', '');
    if (nodeId.startsWith('codec:')) return nodeId.replace('codec:', '');
    if (nodeId.length > 20) {
        return nodeId.substring(0, 17) + '...';
    }
    return nodeId;
}

function formatCompactNumber(num) {
    if (num >= 1000000) return (num / 1000000).toFixed(1) + 'M';
    if (num >= 1000) return (num / 1000).toFixed(1) + 'k';
    return num.toString();
}

function formatDurationCompact(ms) {
    const seconds = Math.floor(ms / 1000);
    if (seconds < 60) return seconds + 's';
    if (seconds < 3600) return Math.floor(seconds / 60) + 'm';
    return Math.floor(seconds / 3600) + 'h';
}

// Modules Tables
function renderModulesTable(status) {
    const table = document.getElementById('modulesTable');
    const countElement = document.getElementById('activeModulesCount');
    
    const modules = status.modules || [];
    const inactiveModules = status.inactive_modules || [];
    const hasNoDefinitions = modules.length === 0 && inactiveModules.length === 0;
    countElement.textContent = modules.length;
    
    if (hasNoDefinitions) {
        table.innerHTML = `
            <tbody>
                <tr>
                    <td colspan="4" class="empty-state">
                        <div class="icon">🧩</div>
                        <div class="message">Keine Module definiert</div>
                    </td>
                </tr>
            </tbody>`;
        return;
    }

    if (modules.length === 0) {
        table.innerHTML = `
            <tbody>
                <tr>
                    <td colspan="4" class="empty-state">
                        <div class="icon">📦</div>
                        <div class="message">Keine aktiven Module</div>
                    </td>
                </tr>
            </tbody>`;
        return;
    }
    
    let tableHTML = '<tbody>';
    
    modules.forEach(module => {
        const rate = calculateRate(module.id, module.runtime.counters, status.timestamp_ms);
        const statusInfo = getModuleStatus(module.runtime);
        const isMobile = window.innerWidth < 768;
        
        tableHTML += `
            <tr>
                <td>
                    <div style="font-weight: 600; font-size: ${isMobile ? '13px' : '14px'};">${module.label}</div>
                    <div style="font-family: monospace; font-size: 9px; color: #888;">${module.id}</div>
                </td>
                <td>
                    <span style="display: inline-block; padding: 2px 6px; border-radius: 4px; font-size: 9px; background: var(--${module.module_type}); color: white;">
                        ${module.module_type}
                    </span>
                </td>
                <td>
                    <div style="display: flex; align-items: center; gap: 4px;">
                        <span class="status-dot ${statusInfo.class}" style="width: 6px; height: 6px;"></span>
                        <span style="font-size: ${isMobile ? '11px' : '12px'};">${statusInfo.text}</span>
                    </div>
                </td>
                <td>
                    <div style="font-family: monospace; font-size: 10px;">
                        <div>RX: ${formatCompactNumber(rate.rx)}/min</div>
                        <div>TX: ${formatCompactNumber(rate.tx)}/min</div>
                    </div>
                </td>
            </tr>
        `;
    });
    
    tableHTML += '</tbody>';
    table.innerHTML = tableHTML;
}

function renderInactiveModules(status) {
    const table = document.getElementById('inactiveModulesTable');
    const countElement = document.getElementById('inactiveModulesCount');
    
    const inactive = status.inactive_modules || [];
    const activeModules = status.modules || [];
    const hasNoDefinitions = inactive.length === 0 && activeModules.length === 0;
    countElement.textContent = inactive.length;
    
    if (hasNoDefinitions) {
        table.innerHTML = `
            <tbody>
                <tr>
                    <td colspan="4" class="empty-state">
                        <div class="icon">🧩</div>
                        <div class="message">Keine Module definiert</div>
                    </td>
                </tr>
            </tbody>`;
        return;
    }

    if (inactive.length === 0) {
        table.innerHTML = `
            <tbody>
                <tr>
                    <td colspan="4" class="empty-state">
                        <div class="icon">🔌</div>
                        <div class="message">Keine inaktiven Module</div>
                    </td>
                </tr>
            </tbody>`;
        return;
    }
    
    let tableHTML = '<tbody>';
    
    inactive.forEach(module => {
        tableHTML += `
            <tr>
                <td>
                    <div style="font-weight: 600; font-size: 13px;">${module.label}</div>
                    <div style="font-family: monospace; font-size: 9px; color: #888;">${module.id}</div>
                </td>
                <td>
                    <span style="display: inline-block; padding: 2px 6px; border-radius: 4px; font-size: 9px; background: var(--${module.module_type}); color: white;">
                        ${module.module_type}
                    </span>
                </td>
                <td>
                    <div style="display: flex; align-items: center; gap: 4px;">
                        <span class="status-dot inactive" style="width: 6px; height: 6px;"></span>
                        <span style="font-size: 11px;">Inaktiv</span>
                    </div>
                </td>
                <td>
                    <span style="font-size: 10px; color: #888;">${module.reason}</span>
                </td>
            </tr>
        `;
    });
    
    tableHTML += '</tbody>';
    table.innerHTML = tableHTML;
}

// Controls
function renderControls(status) {
    const container = document.getElementById('controlsGrid');
    const isMobile = window.innerWidth < 768;
    
    const allControls = [];
    if (status.modules) {
        status.modules.forEach(module => {
            if (module.controls) {
                module.controls.forEach(control => {
                    if (!allControls.some(c => c.action === control.action)) {
                        allControls.push(control);
                    }
                });
            }
        });
    }
    
    if (allControls.length === 0) {
        container.innerHTML = '<button class="btn" disabled>Keine Steuerung</button>';
        return;
    }
    
    const visibleControls = isMobile ? allControls.slice(0, 4) : allControls;
    
    container.innerHTML = visibleControls.map(control => `
        <button class="btn ${control.enabled ? 'primary' : ''}"
                onclick="sendControl('${control.action}')"
                ${!control.enabled ? 'disabled' : ''}
                title="${control.reason || ''}"
                style="padding: ${isMobile ? '6px 8px' : '8px 12px'}; font-size: ${isMobile ? '11px' : '12px'}">
            ${control.label}
        </button>
    `).join('');
}

// Codecs
function renderCodecs(codecs) {
    const container = document.getElementById('codecsList');
    const isMobile = window.innerWidth < 768;
    
    if (!codecs || codecs.length === 0) {
        container.innerHTML = '<div class="empty-state">Keine Codecs verfügbar</div>';
        return;
    }
    
    container.innerHTML = codecs.map(codec => {
        const status = codec.runtime_state;
        const statusClass = status.enabled ? (status.running ? 'active' : 'standby') : 'inactive';
        
        return `
            <div class="codec-item" style="padding: ${isMobile ? '6px' : '8px'}">
                <div class="codec-status">
                    <span class="status-dot ${statusClass}" style="width: 6px; height: 6px;"></span>
                </div>
                <div class="codec-info">
                    <div class="codec-name" style="font-size: ${isMobile ? '11px' : '12px'}">${codec.id}</div>
                    <div class="codec-details" style="font-size: 9px">${codec.codec_type} @ ${codec.config.sample_rate}Hz</div>
                </div>
                <div style="font-size: 10px; color: #888;">
                    ${codec.metrics.frames}F
                </div>
            </div>
        `;
    }).join('');
}
