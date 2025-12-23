// UI Rendering Functions
const pipelineEditorState = {
    nodes: [],
    edges: [],
    nextNodeId: 1,
    selectedNodeId: null,
    draggingNodeId: null,
    dragOffset: { x: 0, y: 0 },
    connectingFrom: null,
    tempEdgeId: null,
    eventsBound: false
};

const pipelinePaletteDefinitions = [
    { kind: 'input', label: 'Input' },
    { kind: 'buffer', label: 'Buffer' },
    { kind: 'processing', label: 'Codec' },
    { kind: 'service', label: 'Service' },
    { kind: 'output', label: 'Output' }
];

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

    if (!paletteContainer || !nodesLayer || !edgesLayer) {
        return;
    }

    seedPipelineModel(status);
    renderPalette(paletteContainer);
    renderNodes(nodesLayer);
    renderEdges(edgesLayer);
    updatePipelineInspector();
    updatePipelinePreview();

    hint.style.display = pipelineEditorState.nodes.length === 0 ? 'block' : 'none';
    attachPipelineEvents();
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
        return {
            id: node.id,
            label: node.label || node.id,
            kind: node.kind,
            x: columnX[node.kind] ?? 40,
            y: 40 + row * 120
        };
    });

    pipelineEditorState.edges = graphEdges.map((edge, index) => ({
        id: `edge-${index}`,
        from: edge.from,
        to: edge.to
    }));
    pipelineEditorState.nextNodeId = graphNodes.length + 1;
}

function renderPalette(container) {
    container.innerHTML = pipelinePaletteDefinitions.map(item => `
        <div class="pipeline-palette-item" draggable="true" data-kind="${item.kind}" data-label="${item.label}">
            <span class="palette-badge">${item.kind}</span>
            <span>${item.label}</span>
        </div>
    `).join('');
}

function renderNodes(container) {
    container.innerHTML = pipelineEditorState.nodes.map(node => `
        <div class="pipeline-node ${pipelineEditorState.selectedNodeId === node.id ? 'selected' : ''}"
             data-node-id="${node.id}"
             style="left:${node.x}px; top:${node.y}px;">
            <div class="pipeline-node-title">${node.label}</div>
            <div class="pipeline-node-meta">${node.kind}</div>
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
        return `<path class="pipeline-edge ${signalClass} ${invalidClass}" d="${path}" />`;
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

    paletteItems.forEach(item => {
        item.addEventListener('dragstart', (event) => {
            event.dataTransfer.setData('text/plain', JSON.stringify({
                kind: item.dataset.kind,
                label: item.dataset.label
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
                const { kind, label } = JSON.parse(payload);
                const rect = canvas.getBoundingClientRect();
                addPipelineNode({
                    kind,
                    label: `Neues ${label}`,
                    x: event.clientX - rect.left - 60,
                    y: event.clientY - rect.top - 20
                });
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
        }

        document.addEventListener('pointermove', handlePointerMove);
        document.addEventListener('pointerup', handlePointerUp);
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

function handlePointerUp() {
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

function addPipelineNode({ kind, label, x, y }) {
    const id = `local-${pipelineEditorState.nextNodeId++}`;
    pipelineEditorState.nodes.push({
        id,
        kind,
        label,
        x: Math.max(20, x),
        y: Math.max(20, y)
    });
    pipelineEditorState.selectedNodeId = id;
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
    pipelineEditorState.edges.push({
        id: `edge-${Date.now()}`,
        from,
        to
    });
    pipelineEditorState.connectingFrom = null;
    pipelineEditorState.tempEdgeId = null;
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
    inspector.innerHTML = `
        <div>
            <div class="label">Modul</div>
            <div class="value">${selected.label}</div>
        </div>
        <div>
            <div class="label">Typ</div>
            <div class="value">${selected.kind}</div>
        </div>
        <div>
            <div class="label">Node-ID</div>
            <div class="value">${selected.id}</div>
        </div>
        <div>
            <div class="label">Verbindungen</div>
            <div class="value">${connectedEdges.length}</div>
        </div>
    `;
}

function updatePipelinePreview() {
    renderPipelineConfigPreview();
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
    return { config, issues };
}

function getPipelineGraphModel() {
    return {
        nodes: pipelineEditorState.nodes.map(node => ({
            id: node.id,
            label: node.label,
            kind: node.kind
        })),
        edges: pipelineEditorState.edges.map(edge => ({
            from: edge.from,
            to: edge.to
        }))
    };
}

function buildPipelineGraphConfig(model) {
    const nodes = model?.nodes || [];
    const edges = model?.edges || [];
    const issues = [];
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
                message: `Signal "${signalType}" passt nicht zu "${toNode.label}".`
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
                message: `Input "${node.label}" hat eingehende Verbindungen. Die Pipeline muss mit Input starten.`
            });
        }
    });

    nodes.forEach(node => {
        const inEdges = incoming.get(node.id) || [];
        const outEdges = outgoing.get(node.id) || [];
        if (inEdges.length === 0 && outEdges.length === 0) {
            issues.push({
                type: 'unconnected',
                message: `Node "${node.label}" ist nicht verbunden.`
            });
        }
    });

    const ringbufferDefaults = {
        slots: 6000,
        chunk_ms: 100,
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
        const codecType = inferCodecType(node.label);
        codecEntries[codecId] = buildCodecDefaults(codecType);
    });

    const inputs = {};
    inputNodes.forEach(node => {
        const inputId = inputIdMap.get(node.id);
        const inputType = inferInputType(node.label);
        const bufferNode = findDownstreamNode(node.id, incoming, outgoing, nodesById, candidate => candidate.kind === 'buffer');
        const bufferId = bufferNode ? ringbufferIdMap.get(bufferNode.id) : primaryRingbufferId;
        inputs[inputId] = buildInputConfig(inputType, bufferId);
    });

    const outputs = {};
    outputNodes.forEach(node => {
        const outputId = outputIdMap.get(node.id);
        const outputType = inferOutputType(node.label);
        const upstreamInput = findUpstreamNode(node.id, incoming, nodesById, candidate => candidate.kind === 'input');
        const upstreamBuffer = findUpstreamNode(node.id, incoming, nodesById, candidate => candidate.kind === 'buffer');
        const upstreamCodec = findUpstreamNode(node.id, incoming, nodesById, candidate => candidate.kind === 'processing');
        const bufferId = upstreamBuffer ? ringbufferIdMap.get(upstreamBuffer.id) : primaryRingbufferId;
        if (!upstreamInput && !upstreamBuffer) {
            issues.push({
                type: 'output-connection',
                message: `Output "${node.label}" ist nicht mit Input oder Buffer verbunden.`
            });
        }
        if (!upstreamCodec) {
            issues.push({
                type: 'codec-missing',
                message: `Output "${node.label}" hat keine Codec-Zuweisung.`
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
        const serviceType = inferServiceType(node.label);
        const upstreamInput = findUpstreamNode(node.id, incoming, nodesById, candidate => candidate.kind === 'input');
        const upstreamBuffer = findUpstreamNode(node.id, incoming, nodesById, candidate => candidate.kind === 'buffer');
        const upstreamCodec = findUpstreamNode(node.id, incoming, nodesById, candidate => candidate.kind === 'processing');
        if (!upstreamInput && !upstreamBuffer) {
            issues.push({
                type: 'service-connection',
                message: `Service "${node.label}" ist nicht mit Input oder Buffer verbunden.`
            });
        }
        if (!upstreamCodec) {
            issues.push({
                type: 'codec-missing',
                message: `Service "${node.label}" hat keine Codec-Zuweisung.`
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

function inferInputType(label) {
    const lower = (label || '').toLowerCase();
    if (lower.includes('icecast')) return 'icecast';
    if (lower.includes('http')) return 'http_stream';
    if (lower.includes('file')) return 'file';
    return 'srt';
}

function inferOutputType(label) {
    const lower = (label || '').toLowerCase();
    if (lower.includes('icecast')) return 'icecast_out';
    if (lower.includes('file')) return 'file_out';
    if (lower.includes('udp')) return 'udp_out';
    return 'srt_out';
}

function inferServiceType(label) {
    const lower = (label || '').toLowerCase();
    if (lower.includes('monitor')) return 'monitoring';
    if (lower.includes('metadata')) return 'metadata';
    return 'audio_http';
}

function inferCodecType(label) {
    const lower = (label || '').toLowerCase();
    if (lower.includes('opus')) return 'opus_ogg';
    if (lower.includes('mp3')) return 'mp3';
    if (lower.includes('aac')) return 'aac_lc';
    if (lower.includes('flac')) return 'flac';
    if (lower.includes('vorbis')) return 'vorbis';
    return 'pcm';
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
    } else if (inputType === 'file') {
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
    if (serviceType === 'monitoring') {
        config.interval_ms = 30000;
    } else if (serviceType === 'metadata') {
        config.url = 'https://api.example';
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
    }
    return config;
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
    countElement.textContent = modules.length;
    
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
    countElement.textContent = inactive.length;
    
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
