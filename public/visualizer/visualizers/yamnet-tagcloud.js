// visualizers/yamnet-tagcloud-mobile.js
class YamnetTagCloudVisualizer {
    constructor(ctx, canvas) {
        this.ctx = ctx;
        this.canvas = canvas;
        
        // State Management
        this.activeTags = new Map();
        this.isActive = false;
        this.connectionStatus = 'disconnected';
        this.lastDataTime = 0;
        
        // API
        this.apiBaseUrl = window.location.origin;
        this.streamEndpoint = `${this.apiBaseUrl}/api/yamnet/stream`;
        
        this.scaleFactor = this.getScaleFactor();
        
        // Farben
        this.colors = {
            music: '#5aff8c',
            instrument: '#2ecc71',
            speech: '#ff8c5a',
            human: '#e74c3c',
            animal: '#9b59b6',
            vehicle: '#3498db',
            nature: '#1abc9c',
            electronic: '#00bcd4',
            household: '#795548',
            impact: '#e91e63',
            tool: '#f39c12',
            sport: '#e67e22',
            other: '#607d8b'
        };
        
        // Animation
        this.animationFrame = null;
        this.lastAnimTime = 0;
        
        // Test-Daten für sofortige Anzeige
        this.testTags = this.getTestTags();
        
        console.log('📱 YAMNet Tag Cloud bereit');
    }
    
    getCanvasMetrics() {
        const rect = this.canvas.getBoundingClientRect();
        const dpr = window.devicePixelRatio || 1;
        const width = rect.width || this.canvas.width / dpr;
        const height = rect.height || this.canvas.height / dpr;
        return { width, height };
    }
    
    getScaleFactor() {
        const { width, height } = this.getCanvasMetrics();
        const area = width * height;
        
        if (area < 200000) return 0.6;      // Sehr klein
        if (area < 500000) return 0.8;      // Klein
        if (area < 1000000) return 0.9;     // Mittel
        return 1.0;                         // Groß
    }
    
    getTestTags() {
        // Realistische Test-Daten basierend auf Radio-Inhalten
        return [
            { id: '0', name: 'Speech', confidence: 0.75, category: 'speech' },
            { id: '139', name: 'Music', confidence: 0.45, category: 'music' },
            { id: '402', name: 'Singing', confidence: 0.35, category: 'music' },
            { id: '2', name: 'Conversation', confidence: 0.25, category: 'speech' },
            { id: '145', name: 'Guitar', confidence: 0.20, category: 'instrument' },
            { id: '146', name: 'Drum', confidence: 0.18, category: 'instrument' },
            { id: '1', name: 'Child speech', confidence: 0.15, category: 'speech' },
            { id: '423', name: 'Pop music', confidence: 0.12, category: 'music' }
        ].slice(0, this.getMaxTags()); // Nur so viele wie passen
    }
    
    getMaxTags() {
        const { width, height } = this.getCanvasMetrics();
        const area = width * height;
        
        if (area < 200000) return 5;
        if (area < 500000) return 7;
        if (area < 1000000) return 9;
        return 12;
    }
    
    activate() {
        if (this.isActive) return;
        
        console.log('✅ YAMNet Tag Cloud aktiviert');
        this.isActive = true;
        
        // 1. SOFORT mit Test-Daten starten
        this.startWithTestData();
        
        // 2. Animation starten (wichtig!)
        this.startAnimation();
        
        // 3. SSE im Hintergrund versuchen
        setTimeout(() => this.startEventSource(), 100);
    }
    
    deactivate() {
        if (!this.isActive) return;
        
        console.log('⏸️ YAMNet Tag Cloud deaktiviert');
        this.isActive = false;
        
        if (this.eventSource) {
            this.eventSource.close();
            this.eventSource = null;
        }
        
        if (this.animationFrame) {
            cancelAnimationFrame(this.animationFrame);
            this.animationFrame = null;
        }
        
        this.activeTags.clear();
    }
    
    startWithTestData() {
        console.log('🚀 Starte mit Test-Daten...');
        
        this.activeTags.clear();
        const now = Date.now();
        
        this.testTags.forEach((tag, index) => {
            this.activeTags.set(tag.id, {
                data: tag,
                currentConfidence: tag.confidence, // SOFORT voll anzeigen
                targetConfidence: tag.confidence,
                color: this.colors[tag.category] || this.colors.other,
                created: now,
                lastUpdate: now,
                position: this.getInitialPosition(index)
            });
        });
        
        console.log(`✅ ${this.testTags.length} Test-Tags geladen`);
        this.connectionStatus = 'demo';
    }
    
    getInitialPosition(index) {
        const totalTags = this.testTags.length;
        
        const angle = (index / totalTags) * Math.PI * 2;
        const distance = 0.25 + (index % 3) * 0.12;
        return {
            type: 'circular',
            angle: angle,
            distance: distance
        };
    }
    
    startEventSource() {
        if (this.eventSource) return;
        
        console.log('📡 Versuche SSE-Verbindung...');
        this.connectionStatus = 'connecting';
        
        try {
            this.eventSource = new EventSource(this.streamEndpoint);
            
            this.eventSource.onopen = () => {
                console.log('✅ SSE-Verbindung geöffnet');
                this.connectionStatus = 'connected';
            };
            
            this.eventSource.onmessage = (event) => {
                try {
                    const data = JSON.parse(event.data);
                    
                    if (data.keepalive) {
                        // Keep-alive ignorieren
                        return;
                    }
                    
                    console.log(`📨 Live-Daten: ${data.topClasses?.length || 0} Tags`);
                    this.processAnalysis(data);
                    this.connectionStatus = 'live';
                    this.lastDataTime = Date.now();
                    
                } catch (e) {
                    console.error('Parse-Fehler:', e);
                }
            };
            
            this.eventSource.onerror = () => {
                console.warn('⚠️  SSE-Fehler');
                this.connectionStatus = 'error';
                
                if (this.eventSource) {
                    this.eventSource.close();
                    this.eventSource = null;
                }
                
                // Fallback zu Polling
                setTimeout(() => this.startPollingFallback(), 3000);
            };
            
        } catch (error) {
            console.error('SSE-Erstellungsfehler:', error);
            this.connectionStatus = 'failed';
            this.startPollingFallback();
        }
    }
    
    startPollingFallback() {
        console.log('🔄 Starte Polling-Fallback...');
        
        // Poll alle 3 Sekunden
        this.pollingInterval = setInterval(async () => {
            try {
                const response = await fetch(`${this.apiBaseUrl}/api/yamnet/analysis`);
                if (response.ok) {
                    const data = await response.json();
                    if (data.topClasses && data.topClasses.length > 0) {
                        this.processAnalysis(data);
                        this.connectionStatus = 'polling';
                    }
                }
            } catch (error) {
                // Silent fail
            }
        }, 3000);
    }
    
    processAnalysis(analysis) {
        if (!this.isActive || !analysis.topClasses) return;
        
        const now = Date.now();
        const maxTags = this.getMaxTags();
        
        // Aktuelle Tags aktualisieren/hinzufügen
        analysis.topClasses.slice(0, maxTags).forEach((tag, index) => {
            const id = tag.id.toString();
            
            if (this.activeTags.has(id)) {
                // Update existing
                const existing = this.activeTags.get(id);
                existing.targetConfidence = tag.confidence;
                existing.lastUpdate = now;
            } else {
                // Add new
                this.activeTags.set(id, {
                    data: tag,
                    currentConfidence: 0, // Start bei 0 für Fade-In
                    targetConfidence: tag.confidence,
                    color: tag.color || this.colors[tag.category] || this.colors.other,
                    created: now,
                    lastUpdate: now,
                    position: this.getInitialPosition(index)
                });
            }
        });
        
        // Alte Tags entfernen (nicht mehr in Analyse)
        const toRemove = [];
        this.activeTags.forEach((tagState, id) => {
            const timeSinceUpdate = now - tagState.lastUpdate;
            if (timeSinceUpdate > 5000) { // 5 Sekunden nicht gesehen
                toRemove.push(id);
            }
        });
        
        toRemove.forEach(id => this.activeTags.delete(id));
        
        // Tags limitieren
        if (this.activeTags.size > maxTags) {
            const tagsArray = Array.from(this.activeTags.entries());
            tagsArray.sort(([,a], [,b]) => b.targetConfidence - a.targetConfidence);
            const toKeep = tagsArray.slice(0, maxTags);
            this.activeTags.clear();
            toKeep.forEach(([id, tag]) => this.activeTags.set(id, tag));
        }
    }
    
    startAnimation() {
        const animate = (timestamp) => {
            if (!this.isActive) return;
            
            this.updateAnimations(timestamp);
            this.draw();
            
            this.animationFrame = requestAnimationFrame(animate);
        };
        
        this.animationFrame = requestAnimationFrame(animate);
    }
    
    updateAnimations(timestamp) {
        const deltaTime = timestamp - (this.lastAnimTime || timestamp);
        this.lastAnimTime = timestamp;
        const deltaSeconds = Math.min(deltaTime, 100) / 1000;
        
        // Schnelle Animationen (10x pro Frame)
        const speed = 10 * deltaSeconds;
        
        this.activeTags.forEach(tagState => {
            // Confidence animieren
            tagState.currentConfidence += (tagState.targetConfidence - tagState.currentConfidence) * speed;
            
            // Sanfte Positions-Änderung
            if (tagState.targetPosition) {
                if (tagState.position.type === 'circular') {
                    tagState.position.angle += (tagState.targetPosition.angle - tagState.position.angle) * speed * 0.5;
                    tagState.position.distance += (tagState.targetPosition.distance - tagState.position.distance) * speed * 0.5;
                }
            }
        });
    }
    
    draw() {
        if (!this.isActive) {
            this.ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);
            return;
        }
        
        const { width, height } = this.getCanvasMetrics();
        const ctx = this.ctx;
        
        // Canvas mit sehr leichtem Fade (für Trail-Effekt)
        ctx.fillStyle = 'rgba(10, 20, 40, 0.05)';
        ctx.fillRect(0, 0, width, height);
        
        // Tags zeichnen
        this.drawTags(ctx, width, height);
        
        // Status-Overlay (nur wenn genug Platz)
        if (width > 320) {
            this.drawMobileStatus(ctx, width, height);
        }
    }
    
    drawTags(ctx, width, height) {
        if (this.activeTags.size === 0) return;
        
        // Tags in Array konvertieren und sortieren
        const tagsArray = Array.from(this.activeTags.values())
            .sort((a, b) => b.targetConfidence - a.targetConfidence);
        
        this.drawUnified(ctx, width, height, tagsArray);
    }
    
    drawUnified(ctx, width, height, tagsArray) {
        // Kreisförmige Anordnung für alle Layouts
        const centerX = width / 2;
        const centerY = height / 2;
        const time = Date.now() * 0.001;
        const radius = Math.min(width, height) * 0.32;
        
        tagsArray.forEach((tagState, index) => {
            const confidence = tagState.currentConfidence;
            if (confidence < 0.01) return;
            
            // Position im Kreis
            const angle = (index / tagsArray.length) * Math.PI * 2 + time * 0.1;
            const distance = 0.3 + confidence * 0.4;
            
            let x = centerX + Math.cos(angle) * radius * distance;
            let y = centerY + Math.sin(angle) * radius * distance;
            
            // Orbitale Bewegung
            const orbit = 20 * confidence;
            x += Math.sin(time * 1.2 + index) * orbit;
            y += Math.cos(time * 1.5 + index) * orbit;
            
            // Schriftgröße
            const fontSize = (18 + confidence * 28) * this.scaleFactor;
            
            this.drawTag(ctx, tagState, x, y, fontSize, time);
        });
    }
    
    drawTag(ctx, tagState, x, y, fontSize, time) {
        const confidence = tagState.currentConfidence;
        const color = tagState.color;
        const opacity = Math.min(1, confidence * 1.5);
        
        ctx.save();
        
        // Tag-Text mit Schatten für Tiefe
        ctx.font = `bold ${fontSize}px 'Segoe UI', Arial, sans-serif`;
        ctx.fillStyle = this.hexToRgba(color, opacity);
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        
        // Leichter Schatten
        ctx.shadowColor = this.hexToRgba(color, opacity * 0.3);
        ctx.shadowBlur = 8 * confidence;
        
        let displayName = tagState.data.name;
        if (fontSize < 18 && displayName.length > 12) {
            displayName = displayName.substring(0, 10) + '...';
        }
        
        ctx.fillText(displayName, x, y);
        
        // Confidence
        const confFontSize = Math.max(12, fontSize * 0.3);
        ctx.font = `${confFontSize}px 'Segoe UI', Arial, sans-serif`;
        ctx.fillStyle = `rgba(255, 255, 255, ${opacity * 0.9})`;
        ctx.fillText(`${Math.round(confidence * 100)}%`, x, y + fontSize * 0.5);
        
        // Pulsierender Ring
        if (confidence > 0.2) {
            const pulse = Math.sin(time * 3) * 0.2 + 0.8;
            ctx.beginPath();
            ctx.arc(x, y, fontSize * 0.6, 0, Math.PI * 2);
            ctx.strokeStyle = this.hexToRgba(color, opacity * pulse * 0.3);
            ctx.lineWidth = 1 + pulse;
            ctx.stroke();
        }
        
        ctx.shadowBlur = 0;
        ctx.restore();
    }
    
    drawMobileStatus(ctx, width, height) {
        const statusTexts = {
            'demo': '📱 Demo',
            'connected': '📡 Verbunden',
            'live': '🎵 Live',
            'polling': '🔄 Polling',
            'error': '⚠️  Fehler',
            'failed': '❌ Keine Verbindung'
        };
        
        const status = this.connectionStatus;
        const text = statusTexts[status] || status;
        
        // Kleine Status-Box oben
        ctx.fillStyle = 'rgba(0, 0, 0, 0.7)';
        ctx.fillRect(10, 10, 120, 40);
        
        ctx.font = '12px "Segoe UI", Arial, sans-serif';
        ctx.fillStyle = '#ffffff';
        ctx.textAlign = 'left';
        ctx.fillText('YAMNet AI', 20, 25);
        
        ctx.font = '11px "Segoe UI", Arial, sans-serif';
        ctx.fillStyle = status === 'live' ? '#5aff8c' : '#ff8c5a';
        ctx.fillText(text, 20, 40);
        
        // Tag-Count
        ctx.fillStyle = 'rgba(255, 255, 255, 0.6)';
        ctx.fillText(`${this.activeTags.size} Tags`, 90, 40);
    }
    
    hexToRgba(hex, alpha) {
        if (!hex || !hex.startsWith('#')) return `rgba(255, 255, 255, ${alpha})`;
        
        const r = parseInt(hex.slice(1, 3), 16);
        const g = parseInt(hex.slice(3, 5), 16);
        const b = parseInt(hex.slice(5, 7), 16);
        
        return `rgba(${r}, ${g}, ${b}, ${alpha})`;
    }
    
    onResize() {
        // Bei Größenänderung neu berechnen
        this.scaleFactor = this.getScaleFactor();
        
        console.log('📱 Resized: Tag Cloud skaliert');
        
        // Tags neu positionieren
        this.repositionTags();
    }
    
    repositionTags() {
        const tagsArray = Array.from(this.activeTags.values());
        
        tagsArray.forEach((tagState, index) => {
            tagState.position = this.getInitialPosition(index);
        });
    }
}

// Global verfügbar
if (typeof window !== 'undefined') {
    window.YamnetTagCloudVisualizer = YamnetTagCloudVisualizer;
}
