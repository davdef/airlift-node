// public/modules/rfm-stage/rfm-stage.js

import '/visualizer/visualizers/yamnet-tagcloud.js';

/**
 * RfmStage – Mini-App für WP-Einbettung
 * - Autoplay-Video (muted)
 * - Yamnet Tag Cloud als Fallback
 * - kein Audio, kein UI, kein Global State
 */
export class RfmStage {
    constructor(rootEl, options = {}) {
        if (!rootEl) {
            throw new Error('RfmStage: rootEl fehlt');
        }

        this.rootEl = rootEl;

        this.options = {
            videoUrl: null,
            yamnetEndpoint: null,
            hlsScriptUrl: 'https://cdn.jsdelivr.net/npm/hls.js@1.5.18/dist/hls.min.js',
            ...options
        };

        this.video = null;
        this.canvas = null;
        this.ctx = null;
        this.tagCloud = null;
        this.debug = Boolean(options.debug);

        this.state = 'init'; // init | video | canvas | error
        this._resizeObserver = null;
    }

    /* ------------------------------------------------------------ */
    /* Lifecycle                                                    */
    /* ------------------------------------------------------------ */

    mount() {
        this._buildDOM();

        this._setupCanvas();
        this._setupVideo();
        this._startVideoProbe();
        this._setupResizeHandling();

        this._showCanvas(); // Default: Visual sichtbar
        this._startTagCloud();
        this._tryStartVideo();
    }

    destroy() {
        if (this.tagCloud?.deactivate) {
            this.tagCloud.deactivate();
        }

        if (this.video) {
            this.video.pause();
            this.video.src = '';
        }

        if (this._videoProbeTimer) {
            clearInterval(this._videoProbeTimer);
        }

        if (this._resizeObserver) {
            this._resizeObserver.disconnect();
        }

        this.rootEl.innerHTML = '';
    }

    /* ------------------------------------------------------------ */
    /* DOM                                                          */
    /* ------------------------------------------------------------ */

    _buildDOM() {
        this.rootEl.innerHTML = '';

        const stage = document.createElement('div');
        stage.className = 'rfm-stage rfm-stage--canvas';

        const visual = document.createElement('div');
        visual.className = 'rfm-stage-visual';

        const video = document.createElement('video');
        video.className = 'rfm-stage-video';
        video.muted = true;
        video.playsInline = true;
        video.autoplay = true;
        video.controls = false;

        const canvas = document.createElement('canvas');
        canvas.className = 'rfm-stage-canvas';

        visual.appendChild(video);
        visual.appendChild(canvas);
        stage.appendChild(visual);

        this.rootEl.appendChild(stage);

        this.video = video;
        this.canvas = canvas;
        this.ctx = canvas.getContext('2d');
        this.visualEl = visual;
        this.stageEl = stage;
    }

    /* ------------------------------------------------------------ */
    /* Video                                                        */
    /* ------------------------------------------------------------ */

_startVideoProbe() {
    this._videoProbeTimer = setInterval(() => {
        if (!this.video) return;

        // HAVE_FUTURE_DATA = 3
        if (
            this.video.readyState >= 3 &&
            this.video.currentTime > 0 &&
            !this.video.paused
        ) {
            this._showVideo();
        }
    }, 500);
}


async _setupVideo() {
    if (!this.options.videoUrl) return;

    const video = this.video;

    video.muted = true;
    video.playsInline = true;
    video.autoplay = true;

    const initHlsIfSupported = () => {
        if (window.Hls && window.Hls.isSupported()) {
            this._initHls(video);
            return true;
        }
        return false;
    };

    if (!initHlsIfSupported()) {
        const hlsLoaded = await this._loadHlsScript();
        if (!hlsLoaded || !initHlsIfSupported()) {
            video.src = this.options.videoUrl;
        }
    }

    this._attachVideoListeners(video);
}

_initHls(video) {
    this.hls = new window.Hls({
        lowLatencyMode: true,
        maxBufferLength: 6
    });

    this.hls.loadSource(this.options.videoUrl);
    this.hls.attachMedia(video);

    this.hls.on(window.Hls.Events.FRAG_BUFFERED, () => {
        if (this.video.currentTime > 0) {
            this._showVideo();
        }
    });

    this.hls.on(window.Hls.Events.ERROR, () => {
        this._showCanvas();
    });
}

_attachVideoListeners(video) {
    // Umschalt-Trigger
    video.addEventListener('canplay', () => {
        if (video.readyState >= 3) {
            this._showVideo();
        }
    });

    video.addEventListener('playing', () => {
        this._showVideo();
    });

    video.addEventListener('stalled', () => {
        this._showCanvas();
    });

    video.addEventListener('error', () => {
        this._showCanvas();
    });
}

_loadHlsScript() {
    if (window.Hls) {
        return Promise.resolve(true);
    }

    if (!this.options.hlsScriptUrl) {
        return Promise.resolve(false);
    }

    if (this._hlsScriptPromise) {
        return this._hlsScriptPromise;
    }

    this._hlsScriptPromise = new Promise((resolve) => {
        const script = document.createElement('script');
        script.src = this.options.hlsScriptUrl;
        script.async = true;
        script.onload = () => resolve(true);
        script.onerror = () => {
            if (this.debug) {
                console.warn('[RfmStage] HLS script konnte nicht geladen werden');
            }
            resolve(false);
        };
        document.head.appendChild(script);
    });

    return this._hlsScriptPromise;
}

    async _tryStartVideo() {
        if (!this.video || !this.options.videoUrl) return;

        try {
            await this.video.play();
            // playing-Event entscheidet final
        } catch {
            // Autoplay verweigert → Canvas bleibt aktiv
            this._showCanvas();
        }
    }

    /* ------------------------------------------------------------ */
    /* Canvas / Tag Cloud                                           */
    /* ------------------------------------------------------------ */

    _setupCanvas() {
        this._resizeCanvas();

        if (window.YamnetTagCloudVisualizer) {
            this.tagCloud = new window.YamnetTagCloudVisualizer(
                this.ctx,
                this.canvas
            );
            this.tagCloud.theme = 'light';

            if (this.options.yamnetEndpoint) {
                this.tagCloud.streamEndpoint = this.options.yamnetEndpoint;
            }
        } else {
            console.warn('RfmStage: YamnetTagCloudVisualizer nicht gefunden');
        }
    }

    _startTagCloud() {
        if (this.tagCloud?.activate) {
            this.tagCloud.activate();
        }
    }

    /* ------------------------------------------------------------ */
    /* Resize                                                       */
    /* ------------------------------------------------------------ */

    _setupResizeHandling() {
        this._resizeObserver = new ResizeObserver(() => {
            this._resizeCanvas();
            if (this.tagCloud?.onResize) {
                this.tagCloud.onResize();
            }
        });

        this._resizeObserver.observe(this.visualEl);
    }

    _resizeCanvas() {
        const rect = this.visualEl.getBoundingClientRect();
        const dpr = window.devicePixelRatio || 1;

        this.canvas.width = Math.floor(rect.width * dpr);
        this.canvas.height = Math.floor(rect.height * dpr);

        this.canvas.style.width = `${rect.width}px`;
        this.canvas.style.height = `${rect.height}px`;

        this.ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    }

    /* ------------------------------------------------------------ */
    /* State Switching                                              */
    /* ------------------------------------------------------------ */

_showVideo(force = false) {
    if (!force && this.state === 'video') return;
    this.state = 'video';

    this.stageEl.classList.remove('rfm-stage--canvas');
    this.stageEl.classList.add('rfm-stage--video');

    this.tagCloud?.deactivate?.();
}

_showCanvas(force = false) {
    if (!force && this.state === 'canvas') return;
    this.state = 'canvas';

    this.stageEl.classList.remove('rfm-stage--video');
    this.stageEl.classList.add('rfm-stage--canvas');

    this.tagCloud?.activate?.();
}

/* ------------------------------------------------------------ */
/* Debug                                                        */
/* ------------------------------------------------------------ */

showVideoDebug() {
    console.warn('[RfmStage][DEBUG] force video');
    this._showVideo(true);
}

showCanvasDebug() {
    console.warn('[RfmStage][DEBUG] force canvas');
    this._showCanvas(true);
}


}
