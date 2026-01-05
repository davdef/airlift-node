import { CONFIG } from './config.js';

export class UIManager {
    constructor(player) {
        this.player = player;
        this.elements = {};
        this.uiAutoHide = true;
        this.uiVisibleTimer = null;
        this.uiHideDelay = 3000;
        
        this.init();
        this.setupAutoHide();
    }
    
    init() {
        this.elements = {
            canvas: document.getElementById('waveform'),
            liveBtn: document.getElementById('liveBtn'),
            playBtn: document.getElementById('playBtn'),
            jumpLiveBtn: document.getElementById('jumpLiveBtn'),
            status: document.getElementById('status'),
            playIcon: document.getElementById('playIcon'),
            debugPanel: document.getElementById('debugPanel'),
            controls: document.getElementById('controls')
        };
        
        // IDLE OVERLAY ERSTELLEN (WIE IM ORIGINAL)
        this.createIdleOverlay();
        
        this.attachEvents();
        this.showUI(); // Start mit sichtbarer UI
    }
    
    createIdleOverlay() {
        const overlay = document.createElement('div');
        overlay.className = 'idle-overlay';
        const header = document.querySelector('header');
        const updateBounds = () => {
            const headerHeight = header?.getBoundingClientRect().height ?? 0;
            overlay.style.top = `${headerHeight}px`;
        };
        overlay.style.cssText = `
            position: fixed;
            left: 0;
            right: 0;
            bottom: 0;
            background: rgba(0, 0, 0, 0.85);
            display: none;
            align-items: center;
            justify-content: center;
            z-index: 900;
            text-align: center;
            color: #fff;
            padding: 24px;
            backdrop-filter: blur(4px);
        `;

        const content = document.createElement('div');
        content.style.cssText = `
            display: flex;
            flex-direction: column;
            align-items: center;
            gap: 16px;
            max-width: 420px;
        `;

        const logo = document.createElement('img');
        logo.src = '../rfm-logo.png';
        logo.alt = 'RFM Logo';
        logo.style.cssText = 'width: 96px; opacity: 0.9; filter: drop-shadow(0 2px 4px rgba(0,0,0,0.3));';

        const title = document.createElement('div');
        title.style.cssText = 'font-size: 20px; font-weight: 600; line-height: 1.4;';
        title.textContent = 'Kein Kontakt zur API';

        const subtitle = document.createElement('div');
        subtitle.style.cssText = 'font-size: 15px; color: #cbd5e1; line-height: 1.5; opacity: 0.9;';
        subtitle.textContent = 'Bitte API starten und Pipeline prüfen.';

        content.appendChild(logo);
        content.appendChild(title);
        content.appendChild(subtitle);
        overlay.appendChild(content);
        document.body.appendChild(overlay);

        this.elements.idleOverlay = overlay;
        this.elements.idleTitle = title;
        this.elements.idleSubtitle = subtitle;
        updateBounds();
        window.addEventListener('resize', updateBounds);
    }
    
    setupAutoHide() {
        // Nur auf mobilen Geräten
        if (window.innerWidth < 900) {
            this.uiAutoHide = true;
            this.startHideTimer();
            
            window.addEventListener('mousemove', () => this.showUI());
            window.addEventListener('touchstart', () => this.showUI());
            window.addEventListener('touchmove', () => this.showUI());
        } else {
            this.uiAutoHide = false;
        }
    }
    
    showUI() {
        if (!this.uiAutoHide) return;
        
        if (this.elements.controls) this.elements.controls.classList.remove('dimmed');
        if (this.elements.debugPanel) this.elements.debugPanel.classList.remove('dimmed');
        if (this.elements.jumpLiveBtn) this.elements.jumpLiveBtn.classList.remove('dimmed');
        
        this.startHideTimer();
    }
    
    startHideTimer() {
        if (!this.uiAutoHide) return;
        
        if (this.uiVisibleTimer) clearTimeout(this.uiVisibleTimer);
        this.uiVisibleTimer = setTimeout(() => {
            if (this.elements.controls) this.elements.controls.classList.add('dimmed');
            if (this.elements.debugPanel) this.elements.debugPanel.classList.add('dimmed');
            if (this.elements.jumpLiveBtn) this.elements.jumpLiveBtn.classList.add('dimmed');
        }, this.uiHideDelay);
    }
    
    attachEvents() {
        if (this.elements.liveBtn) {
            this.elements.liveBtn.addEventListener('click', () => this.player.switchToLive());
        }
        
        if (this.elements.playBtn) {
            this.elements.playBtn.addEventListener('click', () => this.player.togglePlayback());
        }
        
        if (this.elements.jumpLiveBtn) {
            this.elements.jumpLiveBtn.addEventListener('click', () => this.player.switchToLive());
        }
    }
    
    updateStatus(text, type = 'info') {
        if (!this.elements.status) return;
        
        this.elements.status.textContent = text;
        // Status-Klassen gemäß CSS
        const typeMap = {
            'error': 'status-error',
            'warning': 'status-warning', 
            'success': 'status-success',
            'info': 'status-info'
        };
        this.elements.status.className = `status ${typeMap[type] || 'status-info'}`;
    }
    
    showError(message, details = {}) {
        console.error('[UI Error]:', message, details);
        this.updateStatus(message, 'error');
        
        // Temporäre Fehler-Anzeige (könnte als Overlay erweitert werden)
        if (details.fatal) {
            this.setIdleState({
                active: true,
                title: 'Kritischer Fehler',
                subtitle: message
            });
        }
    }
    
    setIdleState({ active, title, subtitle } = {}) {
        if (!this.elements.idleOverlay) return;
        
        this.elements.idleOverlay.style.display = active ? 'flex' : 'none';
        
        if (title && this.elements.idleTitle) {
            this.elements.idleTitle.textContent = title;
        }
        if (subtitle && this.elements.idleSubtitle) {
            this.elements.idleSubtitle.textContent = subtitle;
        }

        this.setControlsEnabled(!active);
    }

    setControlsEnabled(enabled) {
        const targets = [
            this.elements.playBtn,
            this.elements.liveBtn,
            this.elements.jumpLiveBtn
        ];

        targets.forEach((button) => {
            if (button) {
                button.disabled = !enabled;
            }
        });

        if (this.elements.controls) {
            this.elements.controls.classList.toggle('dimmed', !enabled);
        }
    }
}
