export class WebSocketManager {
    constructor(onMessage, onStatus) {
        this.onMessage = onMessage;
        this.onStatus = onStatus;
        this.ws = null;
        this.reconnectAttempts = 0;
        this.reconnectTimer = null;
        this.isConnected = false;
        this.shouldReconnect = true;
    }
    
    connect() {
        if (this.ws && (this.ws.readyState === WebSocket.OPEN || this.ws.readyState === WebSocket.CONNECTING)) {
            return;
        }

        const proto = location.protocol === 'https:' ? 'wss://' : 'ws://';
        const wsUrl = `${proto}${window.location.host}/ws`;

        this.onStatus('connecting', 'Verbinde WebSocket…');
        this.ws = new WebSocket(wsUrl);

        this.ws.onopen = () => {
            this.isConnected = true;
            this.reconnectAttempts = 0;
            this.onStatus('connected', 'Live verbunden');
        };

        this.ws.onmessage = (event) => {
            try {
                const data = JSON.parse(event.data);
                this.onMessage(data);
            } catch (error) {
                console.warn('[WS] Ungültige Nachricht', error);
            }
        };

        this.ws.onerror = () => {
            this.onStatus('error', 'WebSocket Fehler');
        };

        this.ws.onclose = () => {
            this.isConnected = false;
            this.onStatus('warning', 'WebSocket getrennt');
            if (this.shouldReconnect) {
                this.scheduleReconnect();
            }
        };
    }
    
    disconnect() {
        this.shouldReconnect = false;
        if (this.reconnectTimer) {
            clearTimeout(this.reconnectTimer);
            this.reconnectTimer = null;
        }
        if (this.ws) {
            this.ws.close();
            this.ws = null;
        }
        this.isConnected = false;
    }

    scheduleReconnect() {
        if (this.reconnectTimer) {
            return;
        }

        const delay = Math.min(1000 * Math.pow(2, this.reconnectAttempts), 15000);
        this.reconnectAttempts += 1;

        this.reconnectTimer = setTimeout(() => {
            this.reconnectTimer = null;
            this.connect();
        }, delay);
    }
}
