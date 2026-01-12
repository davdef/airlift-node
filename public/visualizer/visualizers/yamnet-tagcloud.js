// visualizers/yamnet-tagcloud.js
class YamnetTagCloudVisualizer {
    constructor(ctx, canvas) {
        this.ctx = ctx;
        this.canvas = canvas;
        this.isAudioIndependent = true;

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
        this.lastEventTime = 0;
        this.lastTickIntervalMs = 1000;
        this.animationTauSeconds = 0.35;

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

        this.theme = 'dark'; // 'dark' | 'light'
        this.backgroundColors = {
           dark: 'rgba(10,20,40,1)',
           light: 'rgba(205,205,205,1)' // helles Grau, nicht Reinweiß
        };

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
        const boost = 1.5;

        if (area < 200_000) return 0.6 * boost;
        if (area < 500_000) return 0.8 * boost;
        if (area < 1_000_000) return 0.9 * boost;
        return 1.0 * boost;
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

        this.connectionStatus = 'connecting';
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
        const nextTags = analysis.topClasses.slice(0, maxTags);
        const nextIds = new Set(nextTags.map(tag => String(tag.id)));
        const totalConfidence = nextTags.reduce((sum, tag) => sum + tag.confidence, 0);

        if (this.lastEventTime) {
            const interval = now - this.lastEventTime;
            this.lastTickIntervalMs = Math.min(2000, Math.max(300, interval));
        }
        this.lastEventTime = now;

        nextTags.forEach((tag, index) => {
            const id = String(tag.id);
            const normalizedConfidence = totalConfidence > 0
                ? tag.confidence / totalConfidence
                : tag.confidence;
            if (this.activeTags.has(id)) {
                const t = this.activeTags.get(id);
                t.data = tag;
                t.color = this.colors[tag.category] || this.colors.other;
                t.targetConfidence = normalizedConfidence;
                t.lastUpdate = now;
                t.fadingOut = false;
            } else {
                this.activeTags.set(id, {
                    data: tag,
                    currentConfidence: 0,
                    targetConfidence: normalizedConfidence,
                    color: this.colors[tag.category] || this.colors.other,
                    created: now,
                    lastUpdate: now,
                    position: this.getInitialPosition(index),
                    fadingOut: false
                });
            }
        });

        [...this.activeTags.entries()].forEach(([id, t]) => {
            if (!nextIds.has(id)) {
                t.targetConfidence = 0;
                t.lastUpdate = now;
                t.fadingOut = true;
            }
            if (now - t.lastUpdate > 15000) this.activeTags.delete(id);
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
        const tau = Math.max(0.15, Math.min(1.0, (this.lastTickIntervalMs / 1000) / 3));
        this.animationTauSeconds = tau;
        const lerpFactor = 1 - Math.exp(-dt / this.animationTauSeconds);
        const toRemove = [];

        this.activeTags.forEach((t, id) => {
            t.currentConfidence += (t.targetConfidence - t.currentConfidence) * lerpFactor;
            if (t.fadingOut && t.currentConfidence < 0.002) {
                toRemove.push(id);
            }
        });

        toRemove.forEach(id => this.activeTags.delete(id));
    }

    /* ------------------------------------------------------------------ */
    /* Rendering (DESKTOP-RENDERER)                                        */
    /* ------------------------------------------------------------------ */

    draw() {
        const { width, height } = this.getCanvasMetrics();
        const ctx = this.ctx;

const bg = this.backgroundColors[this.theme] 
    || this.backgroundColors.dark;

ctx.fillStyle = bg;
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
            if (c < 0.005) return;

            const angle = t.position.angle + time * 0.15;
            const dist = t.position.distance * (0.7 + c * 0.6);

            let x = centerX + Math.cos(angle) * radius * dist;
            let y = centerY + Math.sin(angle) * radius * dist;

            x += Math.sin(time * 1.2 + i) * 20 * c;
            y += Math.cos(time * 1.4 + i) * 20 * c;

            const fontSize = (24 + c * 36) * scale;
            this.drawTag(ctx, t, x, y, fontSize, time);
        });
    }

    drawTag(ctx, tag, x, y, fontSize, time) {
        const c = tag.currentConfidence;
        const opacity = Math.min(1, 0.2 + c * 3);
        const outlineOpacity = Math.min(0.7, opacity + 0.2);

        ctx.save();
        ctx.font = `bold ${fontSize}px Segoe UI, Arial, sans-serif`;
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        ctx.strokeStyle = `rgba(0, 0, 0, ${outlineOpacity})`;
        ctx.lineWidth = Math.max(1, fontSize * 0.08);
        ctx.fillStyle = this.hexToRgba(tag.color, opacity);
        ctx.shadowColor = this.hexToRgba(tag.color, opacity * 0.3);
        ctx.shadowBlur = 8 + 18 * c;

        ctx.strokeText(tag.data.name, x, y);
        ctx.fillText(tag.data.name, x, y);

        ctx.font = `${Math.max(12, fontSize * 0.32)}px Segoe UI`;
        ctx.fillStyle = `rgba(255,255,255,${opacity})`;
        ctx.strokeStyle = `rgba(0, 0, 0, ${outlineOpacity * 0.8})`;
        ctx.lineWidth = Math.max(1, fontSize * 0.05);
        ctx.strokeText(`${Math.round(c * 100)}%`, x, y + fontSize * 0.55);
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
