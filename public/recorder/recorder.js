const recordBtn = document.getElementById("recordBtn");
const statusEl = document.getElementById("status");
const levelEl = document.getElementById("level");
const canvas = document.getElementById("waveform");
const ctx = canvas.getContext("2d");

const METER_HISTORY = 220;
const meterL = new Float32Array(METER_HISTORY);
const meterR = new Float32Array(METER_HISTORY);
let meterIndex = 0;

let audioContext = null;
let mediaStream = null;
let mediaSource = null;
let processor = null;
let audioWs = null;
let peakWs = null;
let echoWs = null;
let running = false;
let rafId = null;
let producerId = null;

// Echo-Variablen
let echoEnabled = false;
let echoAudioContext = null;
let echoScriptProcessor = null;
let echoBufferL = new Float32Array(48000); // 1 Sekunde Buffer bei 48kHz
let echoBufferR = new Float32Array(48000);
let echoBufferIndex = 0;
let echoReadIndex = 0;
let echoVolume = 0.7;
let lastPeakLogTime = 0;
let peakEventCount = 0;

// UI-Elemente
let echoToggle = document.getElementById("echoToggle");
let volumeSlider = document.getElementById("volumeSlider");

function logPeakEvent(flow, peaks) {
    const now = Date.now();
    peakEventCount++;
    
    // Logge nur alle 2 Sekunden oder bei wichtigen Ereignissen
    if (now - lastPeakLogTime > 2000 || peakEventCount % 100 === 0) {
        console.log(`Peak [${flow}]: L=${peaks[0]?.toFixed(3) || 0}, R=${peaks[1]?.toFixed(3) || 0} (events: ${peakEventCount})`);
        lastPeakLogTime = now;
    }
}

function setStatus(text) {
    statusEl.textContent = text;
}

function setLevelText(left, right) {
    levelEl.textContent = `L ${left.toFixed(3)} / R ${right.toFixed(3)}`;
}

function resetMeters() {
    meterL.fill(0);
    meterR.fill(0);
    meterIndex = 0;
    setLevelText(0, 0);
}

function updateViewportUnit() {
    const vh = window.innerHeight * 0.01;
    document.documentElement.style.setProperty("--vh", `${vh}px`);
}

function resizeCanvas() {
    const ratio = window.devicePixelRatio || 1;
    const width = canvas.clientWidth * ratio;
    const height = canvas.clientHeight * ratio;

    if (canvas.width !== width || canvas.height !== height) {
        canvas.width = width;
        canvas.height = height;
    }
}

function drawWaveform() {
    resizeCanvas();

    const w = canvas.width;
    const h = canvas.height;
    const midY = h / 2;
    const maxHeight = h * 0.45;

    ctx.clearRect(0, 0, w, h);
    ctx.fillStyle = "#000";
    ctx.fillRect(0, 0, w, h);

    ctx.beginPath();
    ctx.fillStyle = "rgba(90, 160, 255, 0.25)";
    ctx.strokeStyle = "#5aa0ff";
    ctx.lineWidth = 1;

    // Obere Hälfte der Wellenform
    for (let i = 0; i < METER_HISTORY; i += 1) {
        const idx = (meterIndex + i) % METER_HISTORY;
        const amp = Math.max(meterL[idx], meterR[idx]);
        const height = amp * maxHeight;
        const x = (i / (METER_HISTORY - 1)) * w;
        const y = midY - height;

        if (i === 0) {
            ctx.moveTo(x, y);
        } else {
            ctx.lineTo(x, y);
        }
    }

    // Untere Hälfte der Wellenform
    for (let i = METER_HISTORY - 1; i >= 0; i -= 1) {
        const idx = (meterIndex + i) % METER_HISTORY;
        const amp = Math.max(meterL[idx], meterR[idx]);
        const height = amp * maxHeight;
        const x = (i / (METER_HISTORY - 1)) * w;
        const y = midY + height;

        ctx.lineTo(x, y);
    }

    ctx.closePath();
    ctx.fill();
    ctx.stroke();

    rafId = window.requestAnimationFrame(drawWaveform);
}

async function createProducer() {
    const response = await fetch("/api/recorder/start", {
        method: "POST",
        headers: {
            "Content-Type": "application/json"
        }
    });

    if (!response.ok) {
        throw new Error(`Producer start fehlgeschlagen (${response.status})`);
    }

    const payload = await response.json();
    producerId = payload.producer_id || payload.producerId || payload.id;

    if (!producerId) {
        throw new Error("producer_id fehlt in Response");
    }

    console.log("Producer created:", producerId);
    return producerId;
}

function openAudioWebSocket(producerId) {
    const scheme = window.location.protocol === "https:" ? "wss" : "ws";
    const socket = new WebSocket(`${scheme}://${window.location.host}/ws/recorder/${producerId}`);
    socket.binaryType = "arraybuffer";

    socket.addEventListener("open", () => {
        console.log("✅ Audio WebSocket connected");
        setStatus("Verbunden");
    });

    socket.addEventListener("close", () => {
        console.log("🔌 Audio WebSocket closed");
        if (running) {
            setStatus("Audio-Verbindung getrennt");
        }
    });

    socket.addEventListener("error", (error) => {
        console.error("❌ Audio WebSocket error:", error);
        setStatus("Audio-WebSocket Fehler");
    });

    return socket;
}

function openPeakWebSocket(producerId) {
    const scheme = window.location.protocol === "https:" ? "wss" : "ws";
    const socket = new WebSocket(`${scheme}://${window.location.host}/ws`);

    socket.addEventListener("message", (event) => {
        let data = null;
        try {
            data = JSON.parse(event.data);
        } catch (error) {
            return; // Silent fail für nicht-JSON Nachrichten
        }

        if (!data || !Array.isArray(data.peaks) || !data.flow) return;

        // Prüfe ob die Nachricht für unseren Producer ist
        if (data.flow === producerId || data.flow.includes(producerId)) {
            const peakL = Number(data.peaks[0]) || 0;
            const peakR = Number(data.peaks[1]) || peakL;
            updateMeters(peakL, peakR);
            logPeakEvent(data.flow, data.peaks);
        }
    });

    socket.addEventListener("error", (error) => {
        console.error("❌ Peak WebSocket error:", error);
    });

    socket.addEventListener("close", () => {
        console.log("🔌 Peak WebSocket closed");
    });

    return socket;
}

function openEchoWebSocket(sessionId) {
    const scheme = window.location.protocol === "https:" ? "wss" : "ws";
    const socket = new WebSocket(`${scheme}://${window.location.host}/ws/echo/${sessionId}`);
    socket.binaryType = "arraybuffer";
    
    socket.addEventListener("open", () => {
        console.log("✅ Echo WebSocket connected");
        if (echoToggle) {
            echoToggle.textContent = "Echo AUS";
            echoToggle.disabled = false;
        }
        
        // AudioContext jetzt starten
        setupEchoAudio().then(audioCtx => {
            if (audioCtx && audioCtx.state === "suspended") {
                audioCtx.resume().then(() => {
                    console.log("🎵 Echo audio context resumed");
                }).catch(console.error);
            }
        });
    });
    
    socket.addEventListener("close", () => {
        console.log("🔌 Echo WebSocket closed");
        cleanupEchoAudio();
        if (echoToggle && running) {
            echoToggle.textContent = "Echo AN";
            echoToggle.disabled = false;
        }
    });
    
    socket.addEventListener("error", (error) => {
        console.error("❌ Echo WebSocket error:", error);
        cleanupEchoAudio();
        if (echoToggle) {
            echoToggle.textContent = "Echo AN";
            echoToggle.disabled = false;
        }
    });
    
    socket.addEventListener("message", (event) => {
        if (!echoAudioContext || echoAudioContext.state !== "running") {
            console.warn("Echo audio context not ready, ignoring data");
            return;
        }
        
        // Float32-Daten empfangen
        const floatData = new Float32Array(event.data);
        
        if (floatData.length === 0) return;
        
        // Buffer für Playback füllen
        const numSamples = floatData.length / 2;
        
        for (let i = 0; i < numSamples; i++) {
            const bufferIdx = (echoBufferIndex + i) % echoBufferL.length;
            
            // Interleaved Stereo-Daten: L,R,L,R,...
            const left = floatData[i * 2] * echoVolume;
            const right = floatData[i * 2 + 1] * echoVolume;
            
            // Additive Mixing für den Fall, dass Buffer noch nicht geleert wurde
            echoBufferL[bufferIdx] += left;
            echoBufferR[bufferIdx] += right;
        }
        
        echoBufferIndex = (echoBufferIndex + numSamples) % echoBufferL.length;
        
        // Logge nur gelegentlich
        if (Math.random() < 0.01) { // 1% Chance zu loggen
            console.log(`🎧 Echo: ${numSamples} samples added (total: ${floatData.length} floats)`);
        }
    });
    
    return socket;
}

function updateMeters(peakL, peakR) {
    meterL[meterIndex] = peakL;
    meterR[meterIndex] = peakR;
    meterIndex = (meterIndex + 1) % METER_HISTORY;
    setLevelText(peakL, peakR);
}

async function setupEchoAudio() {
    if (echoAudioContext) {
        if (echoAudioContext.state === "suspended") {
            await echoAudioContext.resume();
        }
        return echoAudioContext;
    }
    
    try {
        echoAudioContext = new AudioContext({ 
            sampleRate: 48000,
            latencyHint: "interactive"
        });
        
        // Wichtig: AudioContext starten (Benutzerinteraktion erforderlich)
        if (echoAudioContext.state === "suspended") {
            await echoAudioContext.resume();
            console.log("🎵 Echo audio context started");
        }
        
        echoScriptProcessor = echoAudioContext.createScriptProcessor(1024, 0, 2);
        
        echoScriptProcessor.onaudioprocess = (event) => {
            const outputL = event.outputBuffer.getChannelData(0);
            const outputR = event.outputBuffer.getChannelData(1);
            
            // Aus Echo-Buffer lesen (circular buffer)
            for (let i = 0; i < 1024; i++) {
                const readIndex = (echoReadIndex + i) % echoBufferL.length;
                outputL[i] = echoBufferL[readIndex];
                outputR[i] = echoBufferR[readIndex];
                
                // Buffer nach dem Lesen löschen
                echoBufferL[readIndex] = 0;
                echoBufferR[readIndex] = 0;
            }
            
            echoReadIndex = (echoReadIndex + 1024) % echoBufferL.length;
        };
        
        echoScriptProcessor.connect(echoAudioContext.destination);
        console.log("🎵 Echo audio setup complete");
        
        return echoAudioContext;
    } catch (error) {
        console.error("❌ Failed to setup echo audio:", error);
        cleanupEchoAudio();
        return null;
    }
}

function cleanupEchoAudio() {
    if (echoScriptProcessor) {
        echoScriptProcessor.disconnect();
        echoScriptProcessor.onaudioprocess = null;
        echoScriptProcessor = null;
    }
    if (echoAudioContext) {
        echoAudioContext.close().catch(console.error);
        echoAudioContext = null;
    }
    
    // Buffer leeren
    echoBufferL.fill(0);
    echoBufferR.fill(0);
    echoBufferIndex = 0;
    echoReadIndex = 0;
}

async function toggleEcho() {
    if (!producerId || !running) {
        if (echoToggle) echoToggle.disabled = true;
        return;
    }
    
    if (echoEnabled) {
        // Echo ausschalten
        echoEnabled = false;
        if (echoWs) {
            echoWs.close();
            echoWs = null;
        }
        cleanupEchoAudio();
        if (echoToggle) {
            echoToggle.textContent = "Echo AN";
        }
        console.log("🔇 Echo disabled");
    } else {
        // Echo einschalten
        echoEnabled = true;
        try {
            echoWs = openEchoWebSocket(producerId);
            console.log("🔊 Echo enabled");
        } catch (error) {
            console.error("❌ Failed to enable echo:", error);
            echoEnabled = false;
            cleanupEchoAudio();
            if (echoToggle) {
                echoToggle.textContent = "Echo AN";
            }
        }
    }
}

function updateEchoVolume(value) {
    echoVolume = parseFloat(value) / 100.0;
    console.log("🎚️ Echo volume:", echoVolume);
}

async function startAudio() {
    try {
        mediaStream = await navigator.mediaDevices.getUserMedia({
            audio: {
                channelCount: 2,
                sampleRate: 48000,
                echoCancellation: false,
                noiseSuppression: false,
                autoGainControl: false,
                latency: 0.01
            }
        });

        console.log("🎤 MediaStream obtained");

        audioContext = new AudioContext({ sampleRate: 48000 });
        console.log("🎵 AudioContext sampleRate:", audioContext.sampleRate);

        mediaSource = audioContext.createMediaStreamSource(mediaStream);
        processor = audioContext.createScriptProcessor(1024, 2, 2);

        // Mute output (verhindert Feedback)
        const mute = audioContext.createGain();
        mute.gain.value = 0;

        mediaSource.connect(processor);
        processor.connect(mute);
        mute.connect(audioContext.destination);

        processor.onaudioprocess = (event) => {
            if (!running) return;
            
            const input = event.inputBuffer;
            const left = input.getChannelData(0);
            const right = input.numberOfChannels > 1 ? input.getChannelData(1) : left;

            const length = left.length;
            const interleaved = new Float32Array(length * 2);

            for (let i = 0; i < length; i += 1) {
                const l = left[i];
                const r = right[i];
                interleaved[i * 2] = l;
                interleaved[i * 2 + 1] = r;
            }

            if (audioWs && audioWs.readyState === WebSocket.OPEN) {
                audioWs.send(interleaved.buffer);
            }
        };

        console.log("🎛️ Audio processing setup complete");
    } catch (error) {
        console.error("❌ Error starting audio:", error);
        throw error;
    }
}

async function startRecording() {
    if (running) return;
    
    running = true;
    recordBtn.textContent = "Stop";
    setStatus("Initialisiere...");
    resetMeters();

    try {
        console.log("🔄 Creating producer...");
        const id = await createProducer();
        producerId = id;
        
        setStatus("Verbinde...");
        console.log("🔗 Opening WebSockets for producer:", producerId);
        
        audioWs = openAudioWebSocket(producerId);
        peakWs = openPeakWebSocket(producerId);
        
        console.log("🎤 Starting audio capture...");
        await startAudio();

        if (!rafId) {
            console.log("📈 Starting waveform rendering");
            drawWaveform();
        }

        // Echo-Button aktivieren
        if (echoToggle) {
            echoToggle.disabled = false;
            echoToggle.textContent = "Echo AN";
        }
        
        setStatus("Aufnahme läuft");
        console.log("✅ Recording started successfully");
        
    } catch (error) {
        console.error("❌ Start recording error:", error);
        setStatus("Fehler beim Start");
        await stopRecording();
    }
}

async function stopRecording() {
    if (!running) return;
    
    console.log("🛑 Stopping recording...");
    running = false;
    recordBtn.textContent = "Start";

    // Echo-Cleanup zuerst
    echoEnabled = false;
    if (echoWs) {
        echoWs.close();
        echoWs = null;
    }
    cleanupEchoAudio();
    if (echoToggle) {
        echoToggle.textContent = "Echo AN";
        echoToggle.disabled = true;
    }

    // API-Aufruf zum Stoppen der Session
    if (producerId) {
        try {
            console.log("📡 Calling stop API for:", producerId);
            const response = await fetch(`/api/recorder/stop/${producerId}`, {
                method: 'POST'
            });
            console.log("📡 Stop API response:", response.status, response.statusText);
        } catch (error) {
            console.error("❌ Stop API error:", error);
        }
    }

    // Audio-Cleanup
    if (processor) {
        processor.disconnect();
        processor.onaudioprocess = null;
        processor = null;
    }

    if (mediaSource) {
        mediaSource.disconnect();
        mediaSource = null;
    }

    if (mediaStream) {
        mediaStream.getTracks().forEach((track) => {
            track.stop();
            track.enabled = false;
        });
        mediaStream = null;
    }

    if (audioContext) {
        await audioContext.close();
        audioContext = null;
    }

    // WebSocket-Cleanup
    if (audioWs) {
        audioWs.close();
        audioWs = null;
    }

    if (peakWs) {
        peakWs.close();
        peakWs = null;
    }

    // Render-Cleanup
    if (rafId) {
        cancelAnimationFrame(rafId);
        rafId = null;
    }

    // State reset
    producerId = null;
    setStatus("Gestoppt");
    console.log("🛑 Recording stopped completely");
}

// Event Listeners
recordBtn.addEventListener("click", () => {
    console.log("🎬 Record button clicked, running:", running);
    if (running) {
        stopRecording();
    } else {
        startRecording();
    }
});

if (echoToggle) {
    echoToggle.addEventListener("click", toggleEcho);
    echoToggle.disabled = true; // Initially disabled
}

if (volumeSlider) {
    volumeSlider.addEventListener("input", (e) => {
        updateEchoVolume(e.target.value);
    });
}

window.addEventListener("resize", () => {
    updateViewportUnit();
    resizeCanvas();
});

window.addEventListener("beforeunload", () => {
    if (running) {
        console.log("⚠️ Page unloading, stopping recording...");
        stopRecording();
    }
});

// Initialisierung
updateViewportUnit();
resizeCanvas();
resetMeters();
console.log("✅ Recorder initialized");
