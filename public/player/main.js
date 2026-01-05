import { AircheckPlayer } from './modules/aircheckPlayer.js';
import { DebugMonitor } from './modules/debugMonitor.js';

// Viewport Height für Mobile
const setVH = () => {
    const vh = window.visualViewport?.height || window.innerHeight;
    document.documentElement.style.setProperty('--vh', `${vh * 0.01}px`);
};

// Initialisierung mit Error Recovery
async function initializePlayer() {
    try {
        setVH();
        window.addEventListener('resize', setVH);
        if (window.visualViewport) {
            window.visualViewport.addEventListener('resize', setVH);
        }
        
        // Player initialisieren
        window.player = new AircheckPlayer();
        
        // Debug Monitor starten
        const debugMonitor = new DebugMonitor({
            status: '#status',
            dbgMode: '#dbgMode',
            dbgViewport: '#dbgViewport',
            dbgPlayhead: '#dbgPlayhead',
            dbgAudioTime: '#dbgAudioTime',
            dbgHistory: '#dbgHistory',
            dbgLastWs: '#dbgLastWs',
            bufferInfo: '#bufferInfo'
        });
        
        debugMonitor.start(window.player);
        window.debugMonitor = debugMonitor;
        
    } catch (error) {
        console.error('Failed to initialize player:', error);
        showFatalError(error);
    }
}

function showFatalError(error) {
    document.body.innerHTML = `
        <div style="padding: 40px 20px; color: white; background: #0b0e11; height: 100vh; text-align: center;">
            <img src="../rfm-logo.png" style="height: 60px; opacity: 0.8; margin-bottom: 20px;">
            <h2 style="color: #ff6b6b;">Player konnte nicht geladen werden</h2>
            <p style="color: #cbd5e1; font-family: monospace; margin: 20px 0; padding: 15px; background: rgba(255,107,107,0.1); border-radius: 6px;">
                ${error.message}
            </p>
            <button onclick="location.reload()" style="
                padding: 12px 24px;
                background: #5aa0ff;
                color: #0b0e11;
                border: none;
                border-radius: 6px;
                font-weight: bold;
                cursor: pointer;
                margin-top: 20px;
            ">Neu laden</button>
            <p style="margin-top: 30px; font-size: 14px; color: #888;">
                Bei wiederholten Fehlern prüfen Sie bitte:<br>
                1. Server-Verbindung<br>
                2. API-Service Status<br>
                3. Browser-Konsole für Details
            </p>
        </div>
    `;
}

// Global Error Handler - nur für UNERWARTETE Fehler
window.addEventListener('error', (event) => {
    // Ignoriere Fehler, die bereits vom Player behandelt wurden
    if (event.error?.message?.includes('Initialisierung') || 
        event.error?.message?.includes('HTTP 502')) {
        event.preventDefault();
        return;
    }
    
    console.error('Global error:', event.error);
    if (window.player?.ui && !window.player.offlineMode) {
        window.player.ui.showError('Unerwarteter Fehler', { fatal: false });
    }
});

window.addEventListener('unhandledrejection', (event) => {
    console.error('Unhandled promise rejection:', event.reason);
});

// Start
window.addEventListener('load', initializePlayer);
