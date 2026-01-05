export const CONFIG = {
    // Viewport
    MIN_VISIBLE_DURATION: 5_000,           // 5 Sekunden
    MAX_VISIBLE_DURATION: 7 * 24 * 60 * 60_000, // 7 Tage
    DEFAULT_VISIBLE_DURATION: 30_000,      // 30 Sekunden
    
    // History
    HISTORY_BUFFER_FACTOR: 2.0,           // Wie viel mehr History als sichtbar laden
    MIN_HISTORY_SPAN: 1_000,              // Mindestspanne für History-Request
    HISTORY_RATE_LIMIT: 150,              // ms zwischen History-Requests
    HISTORY_MAX_RETRIES: 5,               // ← NEU: Max Retries für History
    HISTORY_RETRY_BASE_DELAY: 1000,       // ← NEU: Basis-Delay für Exponential Backoff
    
    // Rendering
    WAVEFORM_RESOLUTION_FACTOR: 2.0,      // Max Punkte pro Pixel
    SMOOTHING_THRESHOLD: 300_000,         // >5 Minuten = Glättung
    SMOOTHING_WINDOW: 0.01,               // 1% der Punkte als Fenster
    
    // WebSocket
    WS_RECONNECT_BASE_DELAY: 1_000,
    WS_RECONNECT_MAX_DELAY: 30_000,
    WS_HEARTBEAT_INTERVAL: 30_000,
    
    // Audio
    AUDIO_LOAD_TIMEOUT: 5_000,
    AUDIO_RETRY_COUNT: 3,
    
    // Player
    INIT_TIMEOUT: 10000,                  // ← NEU: 10s Timeout für Initialisierung
    
    // Timeline
    TICK_STEPS: [
        { duration: 30_000, step: 1_000, format: 'ss' },      // <30s: Sekunden
        { duration: 120_000, step: 5_000, format: 'mm:ss' },  // <2min
        { duration: 300_000, step: 10_000, format: 'mm:ss' }, // <5min
        { duration: 900_000, step: 30_000, format: 'HH:mm' }, // <15min
        { duration: 3_600_000, step: 60_000, format: 'HH:mm' }, // <1h
        { duration: 18_000_000, step: 300_000, format: 'HH:mm' }, // <5h
        { duration: Infinity, step: 3_600_000, format: 'HH:mm' }  // >=5h
    ],
    
    // Farben
    COLORS: {
        background: '#111',
        waveform: '#5aa0ff',
        waveformFill: 'rgba(90, 160, 255, 0.25)',
        timeline: '#888',
        grid: '#333',
        playheadLive: '#ff4d4d',
        playheadTimeshift: '#ffd92c',
        bufferRange: 'rgba(255, 255, 255, 0.1)',
        error: '#ff6b6b',
        success: '#4ecdc4',
        warning: '#ffa726',
        info: '#5aa0ff'
    }
};
