// visualizers/yamnet-tagcloud.js
class YamnetTagCloudVisualizer {
    constructor(ctx, canvas) {
        this.ctx = ctx;
        this.canvas = canvas;

        // State
        this.activeTags = new Map();
        this.isActive = false;
        this.connectionStatus = 'disconnected';
        this.lastDataTime = 0;

        // API
        this.apiBaseUrl = window.location.origin;
        this.streamEndpoint = `${this.apiBaseUrl}/api/yamnet/stream`;

        // Animation
        this.animationFrame = null;
        this.lastAnimTime = 0;

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

        // Testdaten
        this.testTags = this.getTestTags();

        console.log('🌀 YAMNet Tag Cloud (Desktop-unified)');
    }

    /* ------------------------------------------------------------------ */
    /* Canvas / Scaling                                                    */
    /* ------------------------------------------------------------------ */

    getCanvasMetrics() {
        const rect = this.canvas.getBoundingClientRect();
        const dpr = window.devicePixelRatio || 1;
        return {
            width: rect.width || this.canvas.width / dpr,
            height: rect.height || this.canvas.height / dpr
        };
    }

    getScaleFactor() {
        const { width, height } = this.getCanvasMetrics();
        const area = width * height;

        if (area < 200_000) return 0.6;
        if (area < 500_000) return 0.8;
        if (area < 1_000_000) return 0.9;
        return 1.0;
    }

    getMaxTags() {
        const { width, height } = this.getCanvasMetrics();
        const area = width * height;

        if (area < 200_000) return 5;
        if (area < 500_000) return 7;
        if (area < 1_000_000) return 9;
        return 12;
    }

    /* ------------------------------------------------------------------ */
    /* Lifecycle                                                          */
    /* ------------------------------------------------------------------ */

    activate() {
        if (this.isActive) return;
        this.isActive = true;

        this.startWithTestData();
        this.startAnimation();
        setTimeout(() => this.startEventSource(), 100);

        console.log('✅ Tag Cloud aktiviert');
    }

    deactivate() {
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
        console.log('⏸️ Tag Cloud deaktiviert');
    }

    /* ------------------------------------------------------------------ */
    /* Testdaten                                                          */
    /* ------------------------------------------------------------------ */

    getTestTags() {
        return [
            { id: '0',   name: 'Speech',        confidence: 0.75, category: 'speech' },
            { id: '139', name: 'Music',         confidence: 0.45, category: 'music' },
            { id: '402', name: 'Singing',       confidence: 0.35, category: 'music' },
            { id: '2',   name: 'Conversation',  confidence: 0.25, category: 'speech' },
            { id: '145', name: 'Guitar',        confidence: 0.20, category: 'instrument' },
            { id: '146', name: 'Drum',          confidence: 0.18, category: 'instrument' },
            { id: '1',   name: 'Child speech',  confidence: 0.15, category: 'speech' },
            { id: '423', name: 'Pop music',     confidence: 0.12, category: 'music' }
        ];
    }

    startWithTestData() {
        const now = Date.now();
        this.activeTags.clear();

        this.testTags.slice(0, this.getMaxTags()).forEach((tag, index) => {
            this.activeTags.set(tag.id, {
                data: tag,
                currentConfidence: tag.confidence,
                targetConfidence: tag.confidence,
                color: this.colors[tag.category] || this.colors.other,
                created: now,
                lastUpdate: now,
                position: this.getInitialPosition(index)
            });
        });

        this.connectionStatus = 'demo';
    }

    /* ------------------------------------------------------------------ */
    /* Positioning (DESKTOP-LOGIK)                                         */
    /* ------------------------------------------------------------------ */

    getInitialPosition(index) {
        const total = this.getMaxTags();
        return {
            angle: (index / total) * Math.PI * 2,
            distance: 0.35 + (index % 3) * 0.12
        };
    }

    /* ------------------------------------------------------------------ */
    /* Data Handling                                                      */
    /* ------------------------------------------------------------------ */

    startEventSource() {
        try {
            this.eventSource = new EventSource(this.streamEndpoint);

            this.eventSource.onopen = () => {
                this.connectionStatus = 'connected';
            };

            this.eventSource.onmessage = (e) => {
                const data = JSON.parse(e.data);
                if (data?.topClasses) {
                    this.processAnalysis(data);
                    this.connectionStatus = 'live';
                    this.lastDataTime = Date.now();
                }
            };

            this.eventSource.onerror = () => {
                this.connectionStatus = 'error';
                this.eventSource.close();
                this.eventSource = null;
            };
        } catch {
            this.connectionStatus = 'failed';
        }
    }

    processAnalysis(analysis) {
        if (!analysis.topClasses) return;

        const now = Date.now();
        const maxTags = this.getMaxTags();

        analysis.topClasses.slice(0, maxTags).forEach((tag, index) => {
            const id = String(tag.id);
            if (this.activeTags.has(id)) {
                const t = this.activeTags.get(id);
                t.targetConfidence = tag.confidence;
                t.lastUpdate = now;
            } else {
                this.activeTags.set(id, {
                    data: tag,
                    currentConfidence: 0,
                    targetConfidence: tag.confidence,
                    color: this.colors[tag.category] || this.colors.other,
                    created: now,
                    lastUpdate: now,
                    position: this.getInitialPosition(index)
                });
            }
        });

        [...this.activeTags.entries()].forEach(([id, t]) => {
            if (now - t.lastUpdate > 5000) this.activeTags.delete(id);
        });
    }

    /* ------------------------------------------------------------------ */
    /* Animation                                                          */
    /* ------------------------------------------------------------------ */

    startAnimation() {
        const loop = (ts) => {
            if (!this.isActive) return;
            this.updateAnimations(ts);
            this.draw();
            this.animationFrame = requestAnimationFrame(loop);
        };
        this.animationFrame = requestAnimationFrame(loop);
    }

    updateAnimations(ts) {
        const dt = Math.min(ts - (this.lastAnimTime || ts), 100) / 1000;
        this.lastAnimTime = ts;
        const speed = 10 * dt;

        this.activeTags.forEach(t => {
            t.currentConfidence += (t.targetConfidence - t.currentConfidence) * speed;
        });
    }

    /* ------------------------------------------------------------------ */
    /* Rendering (DESKTOP-RENDERER)                                        */
    /* ------------------------------------------------------------------ */

    draw() {
        const { width, height } = this.getCanvasMetrics();
        const ctx = this.ctx;

        ctx.fillStyle = 'rgba(10,20,40,0.08)';
        ctx.fillRect(0, 0, width, height);

        this.drawDesktopUnified(ctx, width, height);
    }

    drawDesktopUnified(ctx, width, height) {
        const scale = this.getScaleFactor();
        const centerX = width / 2;
        const centerY = height / 2;
        const time = Date.now() * 0.001;
        const radius = Math.min(width, height) * 0.35;

        const tags = [...this.activeTags.values()]
            .sort((a, b) => b.targetConfidence - a.targetConfidence);

        tags.forEach((t, i) => {
            const c = t.currentConfidence;
            if (c < 0.01) return;

            const angle = t.position.angle + time * 0.15;
            const dist = t.position.distance * (0.7 + c * 0.6);

            let x = centerX + Math.cos(angle) * radius * dist;
            let y = centerY + Math.sin(angle) * radius * dist;

            x += Math.sin(time * 1.2 + i) * 20 * c;
            y += Math.cos(time * 1.4 + i) * 20 * c;

            const fontSize = (22 + c * 30) * scale;
            this.drawTag(ctx, t, x, y, fontSize, time);
        });
    }

    drawTag(ctx, tag, x, y, fontSize, time) {
        const c = tag.currentConfidence;
        const opacity = Math.min(1, c * 1.4);

        ctx.save();
        ctx.font = `bold ${fontSize}px Segoe UI, Arial, sans-serif`;
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        ctx.fillStyle = this.hexToRgba(tag.color, opacity);
        ctx.shadowColor = this.hexToRgba(tag.color, opacity * 0.3);
        ctx.shadowBlur = 10 * c;

        ctx.fillText(tag.data.name, x, y);

        ctx.font = `${Math.max(11, fontSize * 0.3)}px Segoe UI`;
        ctx.fillStyle = `rgba(255,255,255,${opacity})`;
        ctx.fillText(`${Math.round(c * 100)}%`, x, y + fontSize * 0.55);

        if (c > 0.2) {
            const pulse = Math.sin(time * 3) * 0.2 + 0.8;
            ctx.beginPath();
            ctx.arc(x, y, fontSize * 0.6, 0, Math.PI * 2);
            ctx.strokeStyle = this.hexToRgba(tag.color, opacity * pulse * 0.3);
            ctx.lineWidth = 1 + pulse;
            ctx.stroke();
        }

        ctx.restore();
    }

    hexToRgba(hex, a) {
        const r = parseInt(hex.slice(1, 3), 16);
        const g = parseInt(hex.slice(3, 5), 16);
        const b = parseInt(hex.slice(5, 7), 16);
        return `rgba(${r},${g},${b},${a})`;
    }

    onResize() {
        console.log('🔄 Canvas resized – desktop layout rescaled');
    }
}

// Global
window.YamnetTagCloudVisualizer = YamnetTagCloudVisualizer;

