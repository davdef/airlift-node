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
let ws = null;
let running = false;
let rafId = null;

function setStatus(text) {
    statusEl.textContent = text;
}

function setLevelText(left, right) {
    levelEl.textContent = `L ${left.toFixed(2)} / R ${right.toFixed(2)}`;
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

    for (let i = 0; i < METER_HISTORY; i += 1) {
        const idx = (meterIndex + i) % METER_HISTORY;
        const amp = Math.max(meterL[idx], meterR[idx]);
        const height = Math.max(1, Math.min(1, amp)) * maxHeight;
        const x = (i / (METER_HISTORY - 1)) * w;
        const y = midY - height;

        if (i === 0) {
            ctx.moveTo(x, y);
        } else {
            ctx.lineTo(x, y);
        }
    }

    for (let i = METER_HISTORY - 1; i >= 0; i -= 1) {
        const idx = (meterIndex + i) % METER_HISTORY;
        const amp = Math.max(meterL[idx], meterR[idx]);
        const height = Math.max(1, Math.min(1, amp)) * maxHeight;
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
    const producerId = payload.producer_id || payload.producerId || payload.id;

    if (!producerId) {
        throw new Error("producer_id fehlt in Response");
    }

    return producerId;
}

function openWebSocket(producerId) {
    const scheme = window.location.protocol === "https:" ? "wss" : "ws";
    const socket = new WebSocket(`${scheme}://${window.location.host}/ws/recorder/${producerId}`);
    socket.binaryType = "arraybuffer";

    socket.addEventListener("open", () => {
        setStatus("Verbunden");
    });

    socket.addEventListener("close", () => {
        if (running) {
            setStatus("Verbindung getrennt");
        }
    });

    socket.addEventListener("error", () => {
        setStatus("WebSocket Fehler");
    });

    return socket;
}

async function startAudio() {
    mediaStream = await navigator.mediaDevices.getUserMedia({
        audio: {
            channelCount: 2,
            sampleRate: 48000,
            echoCancellation: false,
            noiseSuppression: false,
            autoGainControl: false
        }
    });

    audioContext = new AudioContext({ sampleRate: 48000 });
    mediaSource = audioContext.createMediaStreamSource(mediaStream);
    processor = audioContext.createScriptProcessor(1024, 2, 2);

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

        let peakL = 0;
        let peakR = 0;
        const length = left.length;
        const interleaved = new Float32Array(length * 2);

        for (let i = 0; i < length; i += 1) {
            const l = left[i];
            const r = right[i];
            interleaved[i * 2] = l;
            interleaved[i * 2 + 1] = r;

            const absL = Math.abs(l);
            const absR = Math.abs(r);
            if (absL > peakL) peakL = absL;
            if (absR > peakR) peakR = absR;
        }

        meterL[meterIndex] = peakL;
        meterR[meterIndex] = peakR;
        meterIndex = (meterIndex + 1) % METER_HISTORY;

        setLevelText(peakL, peakR);

        if (ws && ws.readyState === WebSocket.OPEN) {
            ws.send(interleaved.buffer);
        }
    };
}

async function startRecording() {
    if (running) return;
    running = true;
    recordBtn.textContent = "Stop";
    setStatus("Initialisiere...");
    resetMeters();

    try {
        const producerId = await createProducer();
        setStatus("Verbinde...");
        ws = openWebSocket(producerId);
        await startAudio();

        if (!rafId) {
            drawWaveform();
        }

        setStatus("Streaming");
    } catch (error) {
        console.error(error);
        setStatus("Fehler beim Start");
        await stopRecording();
    }
}

async function stopRecording() {
    if (!running) return;
    running = false;
    recordBtn.textContent = "Start";

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
        mediaStream.getTracks().forEach((track) => track.stop());
        mediaStream = null;
    }

    if (audioContext) {
        await audioContext.close();
        audioContext = null;
    }

    if (ws) {
        ws.close();
        ws = null;
    }

    if (rafId) {
        cancelAnimationFrame(rafId);
        rafId = null;
    }

    setStatus("Gestoppt");
}

recordBtn.addEventListener("click", () => {
    if (running) {
        stopRecording();
    } else {
        startRecording();
    }
});

window.addEventListener("resize", () => {
    updateViewportUnit();
    resizeCanvas();
});

window.addEventListener("beforeunload", () => {
    if (running) {
        stopRecording();
    }
});

updateViewportUnit();
resizeCanvas();
resetMeters();
