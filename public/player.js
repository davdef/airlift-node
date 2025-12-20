class AircheckPlayer {
    constructor() {
        this.canvas = document.getElementById("waveform");
        if (!this.canvas) {
            console.error("Canvas #waveform not found");
            return;
        }
        this.ctx = this.canvas.getContext("2d");

        // === BUFFER-INFO (vom Server) ===
        this.bufferStart = null; // ms
        this.bufferEnd   = null; // ms

        // === HISTORY (von /history + WS) ===
        this.history = [];          // [{ ts, peaks: [...] }]
        this.latestWsTs = null;
        this.loadingHistory = false;
        this.lastHistoryRequest = null;

        // === VIEWPORT ===
        this.minVisibleDuration = 5_000;           // 5s
        this.maxVisibleDuration = 2 * 60 * 60_000; // 2h harte Obergrenze
        this.visibleDuration    = 30_000;          // Start: 30s

        const now = Date.now();
        this.viewportLeft = 0;
        this.viewportRight = 0;
        this.followLive = true;     // Viewport folgt Live
        this.isLiveAudio = false;   // Audio-Modus
        this.dragging = false;

        // === INTERACTION STATE ===
        this.mouseDown = false;
        this.dragStartX = 0;
        this.dragStartLeft = 0;

        // Touch
        this.touchDragging = false;
        this.pinchStartDist = null;
        this.pinchStartViewport = null;

        // === AUDIO ===
        this.audio = new Audio();
        this.audio.crossOrigin = "anonymous";
        this.isPlaying = false;
        this.playbackServerStartTime = null; // ms

        // === DEBUG-UI ===
        this.debug = {
            mode:       document.getElementById("dbgMode"),
            viewport:   document.getElementById("dbgViewport"),
            playhead:   document.getElementById("dbgPlayhead"),
            audioTime:  document.getElementById("dbgAudioTime"),
            history:    document.getElementById("dbgHistory"),
            lastWs:     document.getElementById("dbgLastWs"),
            status:     document.getElementById("status"),
            bufferInfo: document.getElementById("bufferInfo")
        };

        this.setStatus("Initialisiere Player...");

        this.setupCanvas();
        this.setupUI();
        this.setupInteraction();
        this.setupAudioEvents();
        this.setupWebSocket();
        this.fetchBufferInfo().then(() => {
            this.maybeLoadInitialHistory();
        });

        this.startRenderLoop();

        console.log("🎵 Aircheck Player gestartet");
        this.setStatus("Player bereit");
    }

    // ---------------------------------------------------
    //  Hilfsfunktionen
    // ---------------------------------------------------
    setStatus(msg) {
        if (this.debug.status) {
            this.debug.status.textContent = msg;
        }
    }

    formatTime(ts) {
        if (!Number.isFinite(ts)) return "--:--:--";
        const d = new Date(ts);
        const h = String(d.getHours()).padStart(2, "0");
        const m = String(d.getMinutes()).padStart(2, "0");
        const s = String(d.getSeconds()).padStart(2, "0");
        return `${h}:${m}:${s}`;
    }

    getTickStep() {
        const span = this.viewportRight - this.viewportLeft;
        if (span <= 30_000)   return 1_000;   // 1s
        if (span <= 120_000)  return 5_000;   // 5s
        if (span <= 300_000)  return 10_000;  // 10s
        if (span <= 900_000)  return 30_000;  // 30s
        if (span <= 1_800_000)return 60_000;  // 1min
        return 300_000;                       // 5min
    }

getCurrentPlaybackTime() {
    if (this.isLiveAudio) {
        // Live: Aktuellste Server-Zeit
        return this.latestWsTs ?? this.bufferEnd ?? 0;
    }
    
    if (this.playbackServerStartTime != null) {
        // Timeshift: Server-Startzeit + Audio-Offset
        return this.playbackServerStartTime + (this.audio.currentTime * 1000);
    }
    
    // Fallback: Aktuellste verfügbare Server-Zeit
    return this.latestWsTs ?? this.bufferEnd ?? 0;
}

    setupCanvas() {
        const resize = () => {
            const rect = this.canvas.getBoundingClientRect();
            this.canvas.width = rect.width;
            this.canvas.height = rect.height;
        };
        resize();
        window.addEventListener("resize", resize);
    }

    // ---------------------------------------------------
    //  Buffer-Info & Initial-History
    // ---------------------------------------------------
    async fetchBufferInfo() {
        try {
            const res = await fetch("/api/peaks");
            const data = await res.json();
            if (data && data.ok) {
                this.bufferStart = data.start;
                this.bufferEnd   = data.end;

                // initial viewport an Buffer-Ende (Live-Bereich)
                const span = this.visibleDuration;
                this.viewportRight = this.bufferEnd;
                this.viewportLeft  = this.bufferEnd - span;
                this.clampViewportToBuffer();

           // LADE INITIAL HISTORY für diesen Viewport
            await this.loadHistoryWindow(this.viewportLeft, this.viewportRight);

                if (this.debug.bufferInfo) {
                    const durMs = this.bufferEnd - this.bufferStart;
                    const mins = (durMs / 60000).toFixed(1);
                    this.debug.bufferInfo.textContent =
                        `Buffer: ${this.formatTime(this.bufferStart)} – ${this.formatTime(this.bufferEnd)} (${mins} min)`;
                }

                this.setStatus("Buffer-Info geladen");
            } else {
                this.setStatus("Buffer-Info fehlgeschlagen");
            }
        } catch (err) {
            console.error("buffer-info error", err);
            this.setStatus("Fehler bei Buffer-Info");
        }
    }

    async maybeLoadInitialHistory() {
        // Ein erster kleiner Bereich am aktuellen Buffer-Ende
        if (!this.bufferEnd) return;
        const from = this.bufferEnd - 60_000;
        const to   = this.bufferEnd;
        await this.loadHistoryWindow(from, to);
    }

    // ---------------------------------------------------
    //  WebSocket (Live-Peaks)
    // ---------------------------------------------------
    setupWebSocket() {
        const proto = location.protocol === "https:" ? "wss://" : "ws://";
        const wsUrl = proto + window.location.host + '/ws';
        this.ws = new WebSocket(wsUrl);

        this.ws.onopen = () => {
            console.log("[WS] connected");
            this.setStatus("WebSocket verbunden");
        };

        this.ws.onerror = (err) => {
            console.error("[WS] error", err);
            this.setStatus("WebSocket-Fehler");
        };

        this.ws.onclose = () => {
            console.warn("[WS] closed");
            this.setStatus("WebSocket getrennt");
            // optional: Reconnect-Logik später
        };

        this.ws.onmessage = (e) => {
            let data;
            try {
                data = JSON.parse(e.data);
            } catch {
                return;
            }
            if (!data || !Array.isArray(data.peaks) || typeof data.timestamp !== "number") return;

            const ts = data.timestamp;
            this.latestWsTs = ts;
            this.bufferEnd = ts;

            if (!this.followLive) {
                return;
            }

            const entry = {
                ts,
                peaks: [data.peaks[0], data.peaks[1]],
                silence: !!data.silence
            };

            const last = this.history[this.history.length - 1];
            if (last && ts < last.ts) {
                this.history.push(entry);
                this.history.sort((a,b)=>a.ts - b.ts);
            } else {
                this.history.push(entry);
            }
            this.trimHistory();

        };
    }

    // ---------------------------------------------------
    //  UI Buttons
    // ---------------------------------------------------
    setupUI() {
        const liveBtn = document.getElementById("liveBtn");
        const playBtn = document.getElementById("playBtn");

        if (liveBtn) {
            liveBtn.addEventListener("click", () => {
                this.switchToLive();
            });
        }

        if (playBtn) {
            playBtn.addEventListener("click", () => {
                this.togglePlayback();
            });
        }
    }

    togglePlayback() {
        if (this.audio.paused) {
            if (!this.audio.src) {
                this.switchToLive();
            } else {
                this.audio.play().catch(err => {
                    console.error("Audio play error", err);
                    this.setStatus("Audio-Fehler (Play)");
                });
            }
        } else {
            this.audio.pause();
        }
    }

    switchToLive() {
        this.isLiveAudio = true;
        this.followLive = true;
        this.playbackServerStartTime = null;

        this.audio.src = "/audio/live";
        this.audio
            .play()
            .then(() => {
                this.setStatus("LIVE");
            })
            .catch(err => {
                console.error("Live play error", err);
                this.setStatus("Live-Play Fehler");
            });

        const ref = this.latestWsTs ?? this.bufferEnd ?? Date.now();
        this.viewportRight = ref;
        this.viewportLeft  = ref - this.visibleDuration;
        this.clampViewportToBuffer();
    }

    // ---------------------------------------------------
    //  Audio Events
    // ---------------------------------------------------
    setupAudioEvents() {
        this.audio.addEventListener("play", () => {
            this.isPlaying = true;
        });

        this.audio.addEventListener("pause", () => {
            this.isPlaying = false;
        });

        this.audio.addEventListener("error", (e) => {
            console.error("Audio error:", e);
            this.setStatus("Audio-Fehler");
        });
    }

    // ---------------------------------------------------
    //  Interaktion (Maus + Touch)
    // ---------------------------------------------------
    setupInteraction() {
        const canvas = this.canvas;

        // MOUSE
        canvas.addEventListener("mousedown", (e) => {
            // Verhindere Drag wenn kürzlich gezoomt wurde
            if (wheelTimeout && Date.now() - wheelTimeout < 100) {
                return;
            }
            this.mouseDown = true;
            this.dragging = false;
            this.dragStartX = e.clientX;
            this.dragStartLeft = this.viewportLeft;
        });

        window.addEventListener("mousemove", (e) => {
            if (!this.mouseDown) return;

            const dx = e.clientX - this.dragStartX;

            if (!this.dragging && Math.abs(dx) > 4) {
                this.dragging = true;
                this.followLive = false;
            }

            if (this.dragging) {
                const span = this.viewportRight - this.viewportLeft;
                if (span <= 0) return;

                const pxPerMs = this.canvas.width / span;
                const msShift = dx / pxPerMs;

                this.viewportLeft  = this.dragStartLeft - msShift;
                this.viewportRight = this.viewportLeft + this.visibleDuration;

                this.clampViewportToBuffer();
            }
        });

        window.addEventListener("mouseup", (e) => {
            if (!this.mouseDown) return;
            if (!this.dragging) {
                const rect = canvas.getBoundingClientRect();
                const x = e.clientX - rect.left;
                this.handleCanvasClickSeek(x);
            }
            this.mouseDown = false;
            this.dragging = false;
        });

        // Maus-Wheel (Zoom)
        let wheelTimeout = null;
        canvas.addEventListener("wheel", (e) => {
            e.preventDefault();
            
            // Verhindere versehentliches Dragging nach Zoom
            if (this.mouseDown) {
                this.mouseDown = false;
                this.dragging = false;
            }
        
            // Debouncing für smoother Zoom
            if (wheelTimeout) return;
            wheelTimeout = setTimeout(() => {
                wheelTimeout = null;
            }, 50);
        
            const factor = e.deltaY > 0 ? 1.25 : 0.8;
            this.applyZoom(factor, e.clientX);
        }, { passive: false });

        // TOUCH
        const getDistance = (a, b) => {
            const dx = a.clientX - b.clientX;
            const dy = a.clientY - b.clientY;
            return Math.sqrt(dx * dx + dy * dy);
        };

        canvas.addEventListener("touchstart", (e) => {
            e.preventDefault();
            if (e.touches.length === 1) {
                // Single touch = drag or tap
                const t = e.touches[0];
                this.dragStartX = t.clientX;
                this.dragStartLeft = this.viewportLeft;
                this.touchDragging = false;
                this.pinchStartDist = null;
            } else if (e.touches.length === 2) {
                // Pinch
                this.pinchStartDist = getDistance(e.touches[0], e.touches[1]);
                this.pinchStartViewport = {
                    left: this.viewportLeft,
                    right: this.viewportRight,
                    dur: this.visibleDuration
                };
                this.followLive = false;
            }
        }, { passive: false });

        canvas.addEventListener("touchmove", (e) => {
            e.preventDefault();

            if (e.touches.length === 2 && this.pinchStartDist !== null) {
                // Pinch-Zoom - PRÄZISER
                const newDist = getDistance(e.touches[0], e.touches[1]);
                const scale = newDist / this.pinchStartDist; // Umgekehrt für intuitiveres Zoomverhalten
        
                let newDur = this.pinchStartViewport.dur / scale; // Durch Teilen statt Multiplizieren
        
                const bufferSpan = (this.bufferStart && this.bufferEnd)
                    ? (this.bufferEnd - this.bufferStart)
                    : this.maxVisibleDuration;
                const maxDur = Math.min(this.maxVisibleDuration, bufferSpan);
                newDur = Math.max(this.minVisibleDuration, Math.min(maxDur, newDur));
        
                // Viewport um den originalen Mittelpunkt zoomen
                const center = (this.pinchStartViewport.left + this.pinchStartViewport.right) / 2;
                this.visibleDuration = newDur;
                this.viewportLeft = center - newDur / 2;
                this.viewportRight = center + newDur / 2;
        
                this.clampViewportToBuffer();
                return;
            }

            if (e.touches.length === 1 && this.pinchStartDist === null) {
                const t = e.touches[0];
                const dx = t.clientX - this.dragStartX;

                if (!this.touchDragging && Math.abs(dx) > 4) {
                    this.touchDragging = true;
                    this.followLive = false;
                }

                if (this.touchDragging) {
                    const span = this.viewportRight - this.viewportLeft;
                    if (span <= 0) return;
                    const pxPerMs = canvas.width / span;
                    const msShift = dx / pxPerMs;

                    this.viewportLeft  = this.dragStartLeft - msShift;
                    this.viewportRight = this.viewportLeft + this.visibleDuration;

                    this.clampViewportToBuffer();
                }
            }
        }, { passive: false });

        canvas.addEventListener("touchend", (e) => {
            if (!this.touchDragging && this.pinchStartDist === null && e.changedTouches.length === 1) {
                const rect = canvas.getBoundingClientRect();
                const t = e.changedTouches[0];
                const x = t.clientX - rect.left;
                this.handleCanvasClickSeek(x);
            }
            this.touchDragging = false;
            this.pinchStartDist = null;
        }, { passive: false });
    }

    // Zoom um Maus-/Touchposition
    applyZoom(factor, clientX) {
        this.followLive = false;

        const rect = this.canvas.getBoundingClientRect();
        const rel = (clientX - rect.left) / rect.width; // 0..1

        const span = this.viewportRight - this.viewportLeft;
        if (span <= 0) return;

        const centerTs = this.viewportLeft + rel * span;

        let newDur = this.visibleDuration * factor;

        const bufferSpan = (this.bufferStart && this.bufferEnd)
            ? (this.bufferEnd - this.bufferStart)
            : this.maxVisibleDuration;

        const maxDur = Math.min(this.maxVisibleDuration, bufferSpan);
        newDur = Math.max(this.minVisibleDuration, Math.min(maxDur, newDur));

        this.visibleDuration = newDur;
        this.viewportLeft  = centerTs - newDur / 2;
        this.viewportRight = centerTs + newDur / 2;

        this.clampViewportToBuffer();
    }

clampViewportToBuffer() {
    const span = this.visibleDuration;
    
    // MUSS Server-Zeiten verwenden!
    if (this.bufferStart != null && this.bufferEnd != null) {
        const leftLimit = this.bufferStart;
        const rightLimit = this.bufferEnd;
        
        // Viewport komplett nach rechts verschieben wenn nötig
        if (this.viewportRight > rightLimit) {
            this.viewportRight = rightLimit;
            this.viewportLeft = this.viewportRight - span;
        }
        
        if (this.viewportLeft < leftLimit) {
            this.viewportLeft = leftLimit;
            this.viewportRight = this.viewportLeft + span;
        }
        
        // Sicherstellen dass Viewport gültig ist
        if (this.viewportRight > rightLimit) {
            this.viewportRight = rightLimit;
            this.viewportLeft = Math.max(leftLimit, this.viewportRight - span);
        }
        
        this.trimHistory();
        return;
    }
    
    // Fallback nur für History (aber auch das sind Server-Zeiten!)
    if (!this.history.length) return;
    
    const earliest = this.history[0].ts;
    const latest = this.history[this.history.length - 1].ts;
    
    if (this.viewportRight > latest) {
        this.viewportRight = latest;
        this.viewportLeft = this.viewportRight - span;
    }
    
    if (this.viewportLeft < earliest) {
        this.viewportLeft = earliest;
        this.viewportRight = this.viewportLeft + span;
    }
    
    this.trimHistory();
}

    // ---------------------------------------------------
    //  Seeking
    // ---------------------------------------------------
    handleCanvasClickSeek(x) {
        const w = this.canvas.width;
        const span = this.viewportRight - this.viewportLeft;
        if (span <= 0) return;

        const rel = x / w;
        const targetServerTime = this.viewportLeft + rel * span;
        this.seekAudio(targetServerTime);
    }

    seekAudio(targetServerTime) {
        if (!Number.isFinite(targetServerTime)) return;

        this.isLiveAudio = false;
        this.followLive = false;
        this.playbackServerStartTime = targetServerTime;

        const src = `/audio/at?ts=${Math.floor(targetServerTime)}&_=${Date.now()}`;
        this.audio.src = src;

        const startClient = performance.now();

        this.audio.onplay = () => {
            const latency = performance.now() - startClient;
            console.log("⏱️ Timeshift started, latency =", latency.toFixed(1), "ms");
            this.setStatus("Timeshift");
        };

        this.audio
            .play()
            .catch(err => {
                console.error("Timeshift play error", err);
                this.setStatus("Timeshift-Fehler");
            });
    }

    // ---------------------------------------------------
    //  History laden
    // ---------------------------------------------------
async loadHistoryWindow(from, to) {
    try {
        // VERHINDERE REQUEST MIT GLEICHEN ZEITEN ODER NEGATIVER SPANNE
        const span = to - from;
        if (span <= 0) {
            console.log("[History] Skipping (invalid span:", span, "ms)");
            return;
        }
        
        // Mindestspanne von 1000ms (1 Sekunde)
        const minSpan = 1000;
        if (span < minSpan) {
            // Korrigiere to, um mindestens 1s zu haben
            to = from + minSpan;
            console.log("[History] Adjusted span to minimum 1s");
        }
        
        const url = `/api/history?from=${Math.floor(from)}&to=${Math.floor(to)}`;
        
//        console.log("[History] Loading:", url, "span:", span, "ms");
        
        // RATE LIMITING: Verhindere zu häufige Requests
        const now = Date.now();
        if (this.lastHistoryRequest && (now - this.lastHistoryRequest < 2000)) {
//            console.log("[History] Rate limiting, skipping...");
            return;
        }
        this.lastHistoryRequest = now;
        
        const res = await fetch(url);
        if (!res.ok) {
            throw new Error(`HTTP ${res.status}`);
        }
        
        const data = await res.json();
        console.log("[History] Loaded", data.length, "points");
        
        if (!Array.isArray(data)) {
            console.warn("[History] API returned non-array");
            return;
        }
        
        // Konvertiere Format
        const converted = data.map(point => ({
            ts: point.ts,  // Schon in Millisekunden!
            peaks: [point.peak_l, point.peak_r],
            amp: (point.peak_l + point.peak_r) / 2.0,
            silence: point.silence || false
        }));
        
        // Merge mit existing history
        const map = new Map(this.history.map(e => [e.ts, e]));
        converted.forEach(p => map.set(p.ts, p));
        this.history = Array.from(map.values()).sort((a,b) => a.ts - b.ts);
        this.trimHistory();
        
    } catch (err) {
        console.error("[History] Load failed:", err);
    }
}

    trimHistory() {
        if (!this.history.length) return;
        if (!Number.isFinite(this.viewportLeft) || !Number.isFinite(this.viewportRight)) return;

        const span = this.visibleDuration;
        const buffer = span * 2;
        const min = this.viewportLeft - buffer;
        const max = this.viewportRight + buffer;

        if (!Number.isFinite(min) || !Number.isFinite(max)) return;

        const trimmed = this.history.filter(e => e.ts >= min && e.ts <= max);
        if (trimmed.length !== this.history.length) {
            this.history = trimmed;
        }
    }

maybeLoadMoreHistory() {
    if (this.loadingHistory) return;
    if (this.bufferStart == null) return;

    // SICHERSTELLEN DASS from < to
    let from = this.viewportLeft;
    let to = Math.max(
        this.viewportRight, 
        this.latestWsTs || this.bufferEnd || this.viewportRight
    );
    
    // Beschränke auf verfügbaren Buffer
    from = Math.max(from, this.bufferStart);
    to = Math.min(to, this.bufferEnd || Infinity);
    
    // Mindestspanne von 2 Sekunden
    const minSpan = 2000;
    if (to - from < minSpan) {
        // Erweitere nach hinten wenn möglich
        if (this.bufferEnd && to < this.bufferEnd) {
            to = Math.min(from + minSpan, this.bufferEnd);
        } 
        // Oder nach vorne
        else if (this.bufferStart && from > this.bufferStart) {
            from = Math.max(to - minSpan, this.bufferStart);
        }
    }
    
    // Nur laden wenn span > 0
    if (to > from && (to - from) >= 1000) {
        this.loadHistoryWindow(from, to);
    }
}

    // ---------------------------------------------------
    //  Render-Loop
    // ---------------------------------------------------
    startRenderLoop() {
        const loop = () => {
            if (this.followLive && this.latestWsTs && !this.dragging) {
                this.viewportRight = this.latestWsTs;
                this.viewportLeft  = this.viewportRight - this.visibleDuration;
                this.clampViewportToBuffer();
            }

            this.maybeLoadMoreHistory();
            this.draw();
            this.updateDebugPanel();

            requestAnimationFrame(loop);
        };
        requestAnimationFrame(loop);
    }

    // ---------------------------------------------------
    //  Debug-Panel
    // ---------------------------------------------------
    updateDebugPanel() {
        const d = this.debug;
        if (!d) return;

        const mode = this.isLiveAudio
            ? "LIVE"
            : (this.playbackServerStartTime ? "TIMESHIFT" : "IDLE");
        if (d.mode) d.mode.textContent = mode;

        if (d.viewport) {
            const spanSec = (this.viewportRight - this.viewportLeft) / 1000;
            d.viewport.textContent =
                `${this.formatTime(this.viewportLeft)} – ${this.formatTime(this.viewportRight)} (${spanSec.toFixed(1)} s)`;
        }

        const ph = this.getCurrentPlaybackTime();
        if (d.playhead) {
            d.playhead.textContent =
                `${ph} (${this.formatTime(ph)})`;
        }

        if (d.audioTime) {
            d.audioTime.textContent = `${this.audio.currentTime.toFixed(3)} s`;
        }

        if (d.history) {
            const len = this.history.length;
            if (len) {
                d.history.textContent =
                    `${len} Punkte | ${this.formatTime(this.history[0].ts)} – ${this.formatTime(this.history[len - 1].ts)}`;
            } else {
                d.history.textContent = "leer";
            }
        }

        if (d.lastWs) {
            if (this.latestWsTs) {
                d.lastWs.textContent =
                    `${this.latestWsTs} (${this.formatTime(this.latestWsTs)})`;
            } else {
                d.lastWs.textContent = "–";
            }
        }
    }

    // ---------------------------------------------------
    //  Zeichnen
    // ---------------------------------------------------
    draw() {
        const w = this.canvas.width;
        const h = this.canvas.height;

        this.ctx.fillStyle = "#111"; // dunkler Hintergrund
        this.ctx.fillRect(0, 0, w, h);

        this.drawTimeline();
        this.drawWaveform();
        this.drawPlayhead();
        this.drawPlaybackInfo();
    }

    drawTimeline() {
        const w = this.canvas.width;
        const h = this.canvas.height;
        const from = this.viewportLeft;
        const to   = this.viewportRight;
        const span = to - from;
        if (!span || span <= 0) return;

        this.ctx.font = "11px sans-serif";
        this.ctx.fillStyle = "#eee";
        this.ctx.fillText(
            `${this.formatTime(from)} – ${this.formatTime(to)}`,
            10,
            14
        );

        const tick = this.getTickStep();
        const startTick = Math.floor(from / tick) * tick;

        this.ctx.strokeStyle = "#333";
        this.ctx.fillStyle   = "#888";

        for (let t = startTick; t <= to; t += tick) {
            if (t < from) continue;
            const rel = (t - from) / span;
            const x = Math.floor(rel * w);

            this.ctx.beginPath();
            this.ctx.moveTo(x, 20);
            this.ctx.lineTo(x, h);
            this.ctx.stroke();

            this.ctx.fillText(this.formatTime(t), x + 3, 30);
        }
    }

    drawWaveform() {
        if (!this.history.length) return;
    
        const w = this.canvas.width;
        const h = this.canvas.height;
        const mid = h / 2;
    
        const from = this.viewportLeft;
        const to   = this.viewportRight;
        const span = to - from;
        if (!span || span <= 0) return;
    
        const pxPerMs = w / span;
    
        // Konsistente Auflösung: 1 Punkt pro Pixel (max)
        const targetPoints = Math.min(w * 2, this.history.length); // Max 2x Canvas-Breite
        
        // Filtere sichtbare Punkte und reduziere konsistent
        let visible = this.history.filter(e => e.ts >= from && e.ts <= to);
        
        if (visible.length > targetPoints) {
            // Gleichmäßige Reduktion beibehalten, aber konsistenter
            const step = Math.max(1, visible.length / targetPoints);
            const reduced = [];
            for (let i = 0; i < targetPoints; i++) {
                const index = Math.floor(i * step);
                if (index < visible.length) {
                    reduced.push(visible[index]);
                }
            }
            visible = reduced;
        }
    
        // Für sehr große Zeiträume: zusätzliche Glättung
        if (span > 300_000) { // > 5 Minuten
            const smoothed = [];
            const windowSize = Math.max(1, Math.floor(visible.length / 500));
            for (let i = 0; i < visible.length; i++) {
                const start = Math.max(0, i - windowSize);
                const end = Math.min(visible.length - 1, i + windowSize);
                let sum = 0;
                let count = 0;
                for (let j = start; j <= end; j++) {
                    const peaksArr = Array.isArray(visible[j].peaks) ? visible[j].peaks : [visible[j].amp || 0];
                    const avg = peaksArr.reduce((a, b) => a + b, 0) / peaksArr.length;
                    sum += avg;
                    count++;
                }
                smoothed.push({
                    ts: visible[i].ts,
                    peaks: [sum / count]
                });
            }
            visible = smoothed;
        }
    
        this.ctx.beginPath();
        this.ctx.fillStyle   = "rgba(90, 160, 255, 0.25)";
        this.ctx.strokeStyle = "#5aa0ff";
    
        // Obere Hälfte
        for (let i = 0; i < visible.length; i++) {
            const p = visible[i];
            const x = (p.ts - from) * pxPerMs;
            const peaksArr = Array.isArray(p.peaks) ? p.peaks : [p.amp || 0];
            const avg = peaksArr.reduce((a, b) => a + b, 0) / peaksArr.length;
            const y = mid - Math.max(0.1, Math.min(1, avg)) * (h * 0.45); // Clamp auf 0.1-1.0
            
            if (i === 0) this.ctx.moveTo(x, y);
            else this.ctx.lineTo(x, y);
        }
    
        // Untere Hälfte
        for (let i = visible.length - 1; i >= 0; i--) {
            const p = visible[i];
            const x = (p.ts - from) * pxPerMs;
            const peaksArr = Array.isArray(p.peaks) ? p.peaks : [p.amp || 0];
            const avg = peaksArr.reduce((a, b) => a + b, 0) / peaksArr.length;
            const y = mid + Math.max(0.1, Math.min(1, avg)) * (h * 0.45); // Clamp auf 0.1-1.0
            
            this.ctx.lineTo(x, y);
        }
    
        this.ctx.closePath();
        this.ctx.fill();
        this.ctx.stroke();
    }

    drawPlayhead() {
        const w = this.canvas.width;
        const h = this.canvas.height;
        const from = this.viewportLeft;
        const to   = this.viewportRight;
        const span = to - from;
        if (span <= 0) return;

        const playbackTime = this.getCurrentPlaybackTime();
        const rel = (playbackTime - from) / span;
        const x = Math.floor(rel * w);

        if (x < 0 || x > w) return;

        const color = this.isLiveAudio ? "#ff4d4d" : "#ffd92c";

        this.ctx.strokeStyle = color;
        this.ctx.lineWidth = 2;
        this.ctx.beginPath();
        this.ctx.moveTo(x, 32);
        this.ctx.lineTo(x, h);
        this.ctx.stroke();
        this.ctx.lineWidth = 1;

        this.ctx.fillStyle = color;
        this.ctx.beginPath();
        this.ctx.moveTo(x, 28);
        this.ctx.lineTo(x - 6, 18);
        this.ctx.lineTo(x + 6, 18);
        this.ctx.closePath();
        this.ctx.fill();
    }

    drawPlaybackInfo() {
        const h = this.canvas.height;

        this.ctx.font = "11px sans-serif";
        this.ctx.fillStyle = this.isLiveAudio ? "#ff4d4d" : "#ffd92c";

        const mode = this.isLiveAudio ? "LIVE" : (this.playbackServerStartTime ? "TIMESHIFT" : "IDLE");
        const time = this.formatTime(this.getCurrentPlaybackTime());
        this.ctx.fillText(`▶ ${time} [${mode}]`, 10, h - 10);
    }
}

window.addEventListener("load", () => {
    new AircheckPlayer();
});
