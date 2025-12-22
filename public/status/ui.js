// UI Rendering Functions

// Audio Pipeline Rendering - zweispaltige Darstellung
function renderAudioPipeline(status) {
    const container = document.getElementById('pipelineTree');
    
    if (!status.graph || !status.graph.nodes || status.graph.nodes.length === 0) {
        container.innerHTML = `
            <div class="empty-state">
                <div class="icon">🔇</div>
                <div class="message">Keine aktiven Module</div>
            </div>`;
        return;
    }
    
    const nodes = status.graph.nodes || [];
    const edges = status.graph.edges || [];
    const modulesById = new Map((status.modules || []).map(module => [module.id, module]));
    const isMobile = window.innerWidth < 768;
    
    // Edge-Mappings
    const edgesByFrom = new Map();
    const edgesByTo = new Map();
    
    edges.forEach(edge => {
        if (!edgesByFrom.has(edge.from)) {
            edgesByFrom.set(edge.from, []);
        }
        edgesByFrom.get(edge.from).push(edge.to);
        
        if (!edgesByTo.has(edge.to)) {
            edgesByTo.set(edge.to, []);
        }
        edgesByTo.get(edge.to).push(edge.from);
    });
    
    // Baue Input-Buffer-Paare
    const inputBufferPairs = [];
    const bufferIdToInput = new Map();
    
    nodes.forEach(node => {
        if (node.kind === 'buffer') {
            const inputIds = edgesByTo.get(node.id) || [];
            const inputNodes = inputIds
                .map(id => nodes.find(n => n.id === id))
                .filter(n => n && n.kind === 'input');
            
            if (inputNodes.length > 0) {
                inputNodes.forEach(input => {
                    inputBufferPairs.push({
                        input: input,
                        buffer: node,
                        consumers: []
                    });
                    bufferIdToInput.set(node.id, input);
                });
            }
        }
    });
    
    // Finde Konsumenten für jedes Input-Buffer-Paar
    inputBufferPairs.forEach(pair => {
        const consumerIds = edgesByFrom.get(pair.buffer.id) || [];
        pair.consumers = consumerIds
            .map(id => nodes.find(n => n.id === id))
            .filter(n => n && (n.kind === 'processing' || n.kind === 'output' || n.kind === 'service'));
    });
    
    // Gruppiere Codecs und Outputs zusammen
    inputBufferPairs.forEach(pair => {
        // Sortiere Konsumenten: Services, dann Codecs+Outputs gemischt
        pair.consumers.sort((a, b) => {
            if (a.kind === 'service' && b.kind !== 'service') return -1;
            if (a.kind !== 'service' && b.kind === 'service') return 1;
            return 0;
        });
    });
    
    // Finde unverbundene Inputs
    const inputsWithoutBuffer = nodes.filter(node => 
        node.kind === 'input' && 
        !inputBufferPairs.some(pair => pair.input.id === node.id)
    );
    
    // HTML aufbauen
    let pipelineHTML = `
        <div class="pipeline-grid">
            <div class="pipeline-column inputs">
                <div style="margin-bottom: 8px; font-size: 11px; color: #888; text-transform: uppercase;">
                    Input & Buffer
                </div>`;
    
    // Zeige alle Input-Buffer-Paare in der linken Spalte
    if (inputBufferPairs.length > 0) {
        inputBufferPairs.forEach(pair => {
            pipelineHTML += `
                <div class="pipeline-row">
                    <div class="input-buffer-pair">
                        ${renderPipelineNode(pair.input, modulesById, false)}
                        ${renderPipelineNode(pair.buffer, modulesById, true)}
                    </div>
                </div>`;
        });
    }
    
    // Zeige unverbundene Inputs
    if (inputsWithoutBuffer.length > 0) {
        inputsWithoutBuffer.forEach(input => {
            pipelineHTML += `
                <div class="pipeline-row">
                    <div class="input-buffer-pair">
                        ${renderPipelineNode(input, modulesById, false)}
                        <div style="text-align: center; padding: 16px; color: #888; font-size: 11px;">
                            ⚠️ Kein Buffer
                        </div>
                    </div>
                </div>`;
        });
    }
    
    if (inputBufferPairs.length === 0 && inputsWithoutBuffer.length === 0) {
        pipelineHTML += `
            <div class="pipeline-row">
                <div style="text-align: center; padding: 20px; color: #888;">
                    Keine Inputs
                </div>
            </div>`;
    }
    
    pipelineHTML += `
            </div>
            
            <div class="pipeline-column consumers">
                <div style="margin-bottom: 8px; font-size: 11px; color: #888; text-transform: uppercase;">
                    Konsumenten
                </div>`;
    
    // Zeige Konsumenten in der rechten Spalte
    if (inputBufferPairs.length > 0) {
        inputBufferPairs.forEach((pair, index) => {
            if (pair.consumers.length > 0) {
                pipelineHTML += `
                    <div class="pipeline-row">
                        <div class="consumers-group">
                            ${pair.consumers.map(consumer => renderPipelineNode(consumer, modulesById, true)).join('')}
                        </div>
                    </div>`;
            } else {
                pipelineHTML += `
                    <div class="pipeline-row">
                        <div style="text-align: center; padding: 20px; color: #888;">
                            Keine Konsumenten
                        </div>
                    </div>`;
            }
        });
    }
    
    if (inputBufferPairs.length === 0) {
        pipelineHTML += `
            <div class="pipeline-row">
                <div style="text-align: center; padding: 20px; color: #888;">
                    Keine Konsumenten
                </div>
            </div>`;
    }
    
    pipelineHTML += `
            </div>
        </div>`;
    
    container.innerHTML = pipelineHTML;
}

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
