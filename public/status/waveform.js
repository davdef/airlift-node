// Waveform State
let ringbufferData = null;
let bufferCapacity = 6000;
let bufferHeadIndex = 0;
let bufferTailIndex = 0;

let fileOutData = [];
let fileOutFiles = [];

// Animation state
let animationFrameId = null;
let lastDrawTime = 0;
const DRAW_INTERVAL = 100; // 100ms zwischen Canvas-Updates

// Helper function
function normalizeHistoryPoint(point) {
    if (!point) return null;
    const ts = point.ts || point.timestamp;
    if (!ts) return null;
    return {
        ts,
        peaks: point.peaks || [point.peak_l || 0, point.peak_r || 0],
        silence: Boolean(point.silence)
    };
}

// EINFACHE Ringbuffer Initialization
async function initializeRingbuffer(status) {
    try {
        const ring = status.ringbuffer || {};
        bufferCapacity = ring.capacity || 6000;
        bufferHeadIndex = ring.head_index || 0;
        bufferTailIndex = ring.tail_index || 0;
        
        // Hole History für den gesamten Buffer (10min)
        const currentTime = latestStudioTime || Date.now();
        const bufferDuration = bufferCapacity * 100; // 10min in ms
        const historyStart = currentTime - bufferDuration;
        
        const response = await fetch(`/api/history?from=${historyStart}&to=${currentTime}`);
        if (response.ok) {
            const data = await response.json();
            const points = Array.isArray(data) 
                ? data.map(normalizeHistoryPoint).filter(Boolean)
                : [];
            
            // Initialize ALLE Buffer-Positionen
            ringbufferData = new Array(bufferCapacity).fill(null).map(() => ({
                l: 0, r: 0, silence: false,
                hasData: false
            }));
            
            if (points.length > 0) {
                // Sortiere nach Zeit (älteste zuerst)
                points.sort((a, b) => a.ts - b.ts);
                
                // Berechne Zeit-Offset vom ältesten Punkt
                const oldestTime = points[0].ts;
                
                // Ordne JEDEN Punkt einem Buffer-Slot zu
                points.forEach(point => {
                    // Zeitdifferenz vom ältesten Punkt in 100ms-Slots
                    const timeDiff = point.ts - oldestTime;
                    const slotsFromStart = Math.round(timeDiff / 100);
                    
                    // Berechne Position relativ zu Head
                    // Head ist der NEUESTE Punkt, also am RECHTEN Ende
                    const bufferPos = (bufferHeadIndex - slotsFromStart + bufferCapacity) % bufferCapacity;
                    
                    // Sicherstellen, dass wir im Buffer bleiben
                    const safePos = bufferPos % bufferCapacity;
                    
                    ringbufferData[safePos].l = point.peaks[0] || 0;
                    ringbufferData[safePos].r = point.peaks[1] || 0;
                    ringbufferData[safePos].silence = point.silence;
                    ringbufferData[safePos].hasData = true;
                });
            }
            
            // Start animation loop
            startDrawLoop();
            
        } else {
            // Initialize empty buffer
            ringbufferData = new Array(bufferCapacity).fill(null).map(() => ({
                l: 0, r: 0, silence: false,
                hasData: false
            }));
            startDrawLoop();
        }
        
    } catch (error) {
        // Silent error
        ringbufferData = new Array(bufferCapacity).fill(null).map(() => ({
            l: 0, r: 0, silence: false,
            hasData: false
        }));
    }
}

// File-Out Initialization (bleibt gleich)
async function initializeFileOut(status) {
    try {
        // Aktuelle Stunde berechnen
        const currentHour = Math.floor(Date.now() / 1000 / 3600);
        const hourStart = currentHour * 3600 * 1000;
        const hourEnd = hourStart + 3600 * 1000;
        
        // History für aktuelle Stunde
        const response = await fetch(`/api/history?from=${hourStart}&to=${Math.min(latestStudioTime || Date.now(), hourEnd)}`);
        if (response.ok) {
            const data = await response.json();
            fileOutData = Array.isArray(data) 
                ? data.map(normalizeHistoryPoint).filter(Boolean)
                : [];
        }
        
        // File-Out files vom Status
        if (status.file_out && status.file_out.current_files) {
            fileOutFiles = status.file_out.current_files;
            updateFileOutFiles();
        }
        
    } catch (error) {
        // Silent error
    }
}

function updateFileOutFiles() {
    const filesContainer = document.getElementById('recorderFiles');
    
    if (fileOutFiles.length === 0) {
        filesContainer.innerHTML = `
            <div class="empty-state">
                <div class="icon">📁</div>
                <div class="message">Keine WAV-Dateien</div>
            </div>`;
        return;
    }
    
    filesContainer.innerHTML = fileOutFiles.map(file => {
        const fileName = file.split('/').pop();
        
        return `
            <div class="file-item">
                <div class="file-icon">🎵</div>
                <div class="file-info">
                    <div class="file-name">${fileName}</div>
                    <div class="file-path">${file}</div>
                </div>
                <div class="file-size">WAV</div>
            </div>
        `;
    }).join('');
}

// Animation Loop
function startDrawLoop() {
    if (animationFrameId) cancelAnimationFrame(animationFrameId);
    
    function drawLoop() {
        const now = Date.now();
        if (now - lastDrawTime >= DRAW_INTERVAL) {
            drawRingbuffer();
            drawFileOut();
            lastDrawTime = now;
        }
        animationFrameId = requestAnimationFrame(drawLoop);
    }
    
    drawLoop();
}

// Draw Ringbuffer - ZWEI BALKEN (oben/unten) mit NULL-LINIE
function drawRingbuffer() {
    if (!ringbufferCtx || !ringbufferData) return;
    
    const width = ringbufferCanvas.width;
    const height = ringbufferCanvas.height;
    const midY = Math.floor(height / 2);
    
    // Clear canvas
    ringbufferCtx.fillStyle = '#0d1117';
    ringbufferCtx.fillRect(0, 0, width, height);
    
    // Weiße Null-Linie in der Mitte
    ringbufferCtx.strokeStyle = 'rgba(255, 255, 255, 0.3)';
    ringbufferCtx.lineWidth = 1;
    ringbufferCtx.beginPath();
    ringbufferCtx.moveTo(0, midY);
    ringbufferCtx.lineTo(width, midY);
    ringbufferCtx.stroke();
    
    // Berechne wie viele Balken wir zeichnen können
    const barWidth = Math.max(1, width / bufferCapacity);
    
    // Zeichne alle Buffer-Positionen
    for (let i = 0; i < bufferCapacity; i++) {
        const point = ringbufferData[i];
        const x = i * barWidth;
        
        if (!point.hasData) {
            // Keine Daten: sehr schwache rote Markierung
            ringbufferCtx.fillStyle = 'rgba(231, 76, 60, 0.05)';
            ringbufferCtx.fillRect(x, 0, barWidth, height);
            continue;
        }
        
        // Bestimme ob dieser Chunk KRÄFTIG (links von Head) oder MATT (rechts von Tail) ist
        const isKraeftig = isLeftOfHead(i, bufferHeadIndex, bufferTailIndex, bufferCapacity);
        
        // Berechne Balkenhöhen
        const leftHeight = Math.min(point.l * height * 0.45, midY);
        const rightHeight = Math.min(point.r * height * 0.45, midY);
        
        // Linker Kanal (oben) - BLAU
        if (leftHeight > 0) {
            ringbufferCtx.fillStyle = isKraeftig 
                ? 'rgba(90, 160, 255, 0.8)'  // Kräftig
                : 'rgba(90, 160, 255, 0.3)'; // Matt
            ringbufferCtx.fillRect(x, midY - leftHeight, barWidth - 0.5, leftHeight);
        }
        
        // Rechter Kanal (unten) - LILA
        if (rightHeight > 0) {
            ringbufferCtx.fillStyle = isKraeftig 
                ? 'rgba(155, 89, 182, 0.8)'  // Kräftig
                : 'rgba(155, 89, 182, 0.3)'; // Matt
            ringbufferCtx.fillRect(x, midY, barWidth - 0.5, rightHeight);
        }
        
        // Silence Overlay (wenn beide Kanäle silence sind)
        if (point.silence) {
            ringbufferCtx.fillStyle = isKraeftig 
                ? 'rgba(231, 76, 60, 0.3)'  // Kräftig rot
                : 'rgba(231, 76, 60, 0.1)'; // Matt rot
            ringbufferCtx.fillRect(x, 0, barWidth, height);
        }
    }
    
    // Draw markers - Head und Tail
    const headX = (bufferHeadIndex / bufferCapacity) * width;
    const tailX = (bufferTailIndex / bufferCapacity) * width;
    
    const overlay = document.getElementById('ringbufferOverlay');
    overlay.innerHTML = `
        <div class="waveform-marker head" style="left: ${headX}px;" title="Head (aktuellster Chunk)"></div>
        <div class="waveform-marker tail" style="left: ${tailX}px;" title="Tail (wird als nächstes überschrieben)"></div>
    `;
    
    // Update time display
    const timeDisplay = document.getElementById('ringbufferTime');
    if (latestStudioTime) {
        const tenMinutesAgo = latestStudioTime - (10 * 60 * 1000);
        timeDisplay.textContent = `${formatTime(tenMinutesAgo)} ← ${formatTime(latestStudioTime)}`;
    }
    
    // Update stats
    const statsContainer = document.getElementById('ringbufferStats');
    if (currentStatus?.ring) {
        const errorFills = currentStatus.ring.fill || 0;
        const headSeq = currentStatus.ring.head_seq || 0;
        
        // Zähle gefüllte Slots
        const filledSlots = ringbufferData.filter(p => p.hasData).length;
        const fillPercentage = ((filledSlots / bufferCapacity) * 100).toFixed(1);
        
        statsContainer.innerHTML = `
            <div class="waveform-stat">
                <div class="waveform-stat-label">Error-Fills</div>
                <div class="waveform-stat-value">${errorFills}</div>
                <div class="waveform-stat-sub">Gefüllte Lücken</div>
            </div>
            <div class="waveform-stat">
                <div class="waveform-stat-label">Buffer-Füllung</div>
                <div class="waveform-stat-value">${fillPercentage}%</div>
                <div class="waveform-stat-sub">${filledSlots}/${bufferCapacity}</div>
            </div>
            <div class="waveform-stat">
                <div class="waveform-stat-label">Head</div>
                <div class="waveform-stat-value">${bufferHeadIndex}</div>
                <div class="waveform-stat-sub">Schreibposition</div>
            </div>
            <div class="waveform-stat">
                <div class="waveform-stat-label">Tail</div>
                <div class="waveform-stat-value">${bufferTailIndex}</div>
                <div class="waveform-stat-sub">Wird überschrieben</div>
            </div>
            <div class="waveform-stat">
                <div class="waveform-stat-label">Sequenzen</div>
                <div class="waveform-stat-value">${headSeq}</div>
                <div class="waveform-stat-sub">Buffer-Durchläufe</div>
            </div>
        `;
    }
    
    // Update title
    document.getElementById('ringbufferTitle').textContent = 
        `Ringbuffer (${bufferCapacity} × 100ms = 10min)`;
}

// Hilfsfunktion: Bestimmt ob ein Chunk links von Head ist (kräftig)
function isLeftOfHead(bufferIndex, headIndex, tailIndex, capacity) {
    // Vereinfachte Logik:
    // Alles zwischen Tail und Head (zyklisch) ist KRÄFTIG (links von Head)
    // Alles zwischen Head und Tail (zyklisch) ist MATT (rechts von Head)
    
    if (headIndex < tailIndex) {
        // Buffer nicht gewrapped (z.B. Head=100, Tail=5000)
        // Kräftig: [Tail...capacity-1] + [0...Head]
        // Matt: [Head+1...Tail-1]
        return bufferIndex <= headIndex || bufferIndex >= tailIndex;
    } else if (headIndex > tailIndex) {
        // Buffer gewrapped (z.B. Head=5000, Tail=100)
        // Kräftig: [Tail...Head]
        // Matt: [Head+1...capacity-1] + [0...Tail-1]
        return bufferIndex >= tailIndex && bufferIndex <= headIndex;
    } else {
        // Head == Tail: Buffer voll oder leer
        return true;
    }
}

// Update Ringbuffer with new peak - EINFACH
function updateRingbufferPoint(timestamp, peaks, silence) {
    if (!ringbufferData) return;
    
    // Einfach: Head um 1 erhöhen
    bufferHeadIndex = (bufferHeadIndex + 1) % bufferCapacity;
    bufferTailIndex = (bufferHeadIndex + 1) % bufferCapacity;
    
    // Schreibe in die neue Head-Position
    const left = peaks[0] || 0;
    const right = peaks[1] || 0;
    
    ringbufferData[bufferHeadIndex].l = left;
    ringbufferData[bufferHeadIndex].r = right;
    ringbufferData[bufferHeadIndex].silence = silence;
    ringbufferData[bufferHeadIndex].hasData = true;
    
    // Überschreibe Tail-Position (setze auf leer)
    ringbufferData[bufferTailIndex].hasData = false;
    ringbufferData[bufferTailIndex].l = 0;
    ringbufferData[bufferTailIndex].r = 0;
    ringbufferData[bufferTailIndex].silence = false;
}

// Update File-Out with new peak
function updateFileOutPoint(timestamp, peaks, silence) {
    // Add to file-out data
    fileOutData.push({
        ts: timestamp,
        peaks: peaks || [0, 0],
        silence: silence
    });
    
    // Keep only last hour
    const hourAgo = timestamp - 3600000;
    fileOutData = fileOutData.filter(p => p.ts >= hourAgo);
}

// Draw File-Out - auch mit zwei Balken
function drawFileOut() {
    if (!recorderCtx) return;
    
    const width = recorderCanvas.width;
    const height = recorderCanvas.height;
    const midY = Math.floor(height / 2);
    
    // Clear
    recorderCtx.fillStyle = '#0d1117';
    recorderCtx.fillRect(0, 0, width, height);
    
    // Weiße Null-Linie
    recorderCtx.strokeStyle = 'rgba(255, 255, 255, 0.3)';
    recorderCtx.lineWidth = 1;
    recorderCtx.beginPath();
    recorderCtx.moveTo(0, midY);
    recorderCtx.lineTo(width, midY);
    recorderCtx.stroke();
    
    // Current hour
    const now = latestStudioTime || Date.now();
    const currentHour = Math.floor(now / 1000 / 3600);
    const hourStart = currentHour * 3600 * 1000;
    const hourEnd = hourStart + 3600 * 1000;
    
    // Progress
    const progress = Math.min(1, Math.max(0, (now - hourStart) / 3600000));
    const progressPercent = Math.round(progress * 100);
    document.getElementById('recorderProgress').textContent = `${progressPercent}%`;
    
    // Zeichne Wellenform wenn Daten vorhanden
    if (fileOutData.length > 0) {
        const barWidth = 2; // Feste Balkenbreite
        
        fileOutData.forEach(point => {
            const pointProgress = (point.ts - hourStart) / 3600000;
            if (pointProgress < 0 || pointProgress > progress) return;
            
            const x = Math.floor(pointProgress * width);
            const left = point.peaks[0] || 0;
            const right = point.peaks[1] || 0;
            
            const leftHeight = Math.min(left * height * 0.45, midY);
            const rightHeight = Math.min(right * height * 0.45, midY);
            
            // Linker Kanal (oben) - GRÜN
            if (leftHeight > 0) {
                recorderCtx.fillStyle = point.silence 
                    ? 'rgba(231, 76, 60, 0.6)' 
                    : 'rgba(46, 204, 113, 0.8)';
                recorderCtx.fillRect(x - barWidth/2, midY - leftHeight, barWidth, leftHeight);
            }
            
            // Rechter Kanal (unten) - HELLGRÜN
            if (rightHeight > 0) {
                recorderCtx.fillStyle = point.silence 
                    ? 'rgba(231, 76, 60, 0.6)' 
                    : 'rgba(39, 174, 96, 0.8)';
                recorderCtx.fillRect(x - barWidth/2, midY, barWidth, rightHeight);
            }
        });
        
        // Write pointer
        recorderCtx.strokeStyle = '#ffffff';
        recorderCtx.lineWidth = 2;
        recorderCtx.beginPath();
        recorderCtx.moveTo(width * progress, 0);
        recorderCtx.lineTo(width * progress, height);
        recorderCtx.stroke();
    }
    
    // Update time display
    const timeDisplay = document.getElementById('recorderTime');
    const hourDate = new Date(hourStart);
    const hourStr = hourDate.getHours().toString().padStart(2, '0');
    timeDisplay.textContent = `${formatTime(hourStart)} → ${formatTime(hourEnd)}`;
}
