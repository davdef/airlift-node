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

this.video.style.position = 'absolute';
this.video.style.inset = '0';
this.video.style.width = '100%';
this.video.style.height = '100%';
this.video.style.objectFit = 'cover';
this.video.style.zIndex = '2';

this.canvas.style.position = 'absolute';
this.canvas.style.inset = '0';
this.canvas.style.zIndex = '1';

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
        stage.className = 'rfm-stage';

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


_setupVideo() {
    if (!this.options.videoUrl) return;

    const video = this.video;

    video.muted = true;
    video.playsInline = true;
    video.autoplay = true;

    if (window.Hls && window.Hls.isSupported()) {
        this.hls = new window.Hls({
            lowLatencyMode: true,
            maxBufferLength: 6
        });

        this.hls.loadSource(this.options.videoUrl);
        this.hls.attachMedia(video);

this.hls.on(Hls.Events.FRAG_BUFFERED, () => {
    if (this.video.currentTime > 0) {
        this._showVideo();
    }
});

        this.hls.on(window.Hls.Events.ERROR, () => {
            this._showCanvas();
        });
    } else {
        video.src = this.options.videoUrl;
    }

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

_showVideo() {
    if (this.state === 'video') return;
    this.state = 'video';

    this.canvas.style.display = 'none';
    this.video.style.display = 'block';

    this.tagCloud?.deactivate?.();
}

_showCanvas() {
    if (this.state === 'canvas') return;
    this.state = 'canvas';

    this.video.style.display = 'none';
    this.canvas.style.display = 'block';

    this.tagCloud?.activate?.();
}

/* ------------------------------------------------------------ */
/* Debug                                                        */
/* ------------------------------------------------------------ */

showVideoDebug() {
    console.warn('[RfmStage][DEBUG] force video');
    this.video.style.display = 'block';
    this.canvas.style.display = 'none';
}

showCanvasDebug() {
    console.warn('[RfmStage][DEBUG] force canvas');
    this.video.style.display = 'none';
    this.canvas.style.display = 'block';

    if (this.tagCloud?.activate) {
        this.tagCloud.activate();
    }
}


}
