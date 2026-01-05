export class WebSocketManager {
    constructor(onMessage, onStatus) {
        this.onMessage = onMessage;
        this.onStatus = onStatus;
        this.ws = null;
        this.reconnectAttempts = 0;
        this.reconnectTimer = null;
        this.isConnected = false;
    }
    
    connect() {
        // Vereinfachte Version für jetzt
        console.log('[WS] Connect placeholder');
    }
    
    disconnect() {
        console.log('[WS] Disconnect placeholder');
    }
}
