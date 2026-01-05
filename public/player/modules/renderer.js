import { CONFIG } from './config.js';
import { TimeUtils } from './timeUtils.js';

export class Renderer {
    constructor(canvas) {
        this.canvas = canvas;
        this.ctx = canvas.getContext('2d');
        this.size = { width: 0, height: 0 };
        this.resize = this.resize.bind(this);

        if (window.ResizeObserver) {
            this.resizeObserver = new ResizeObserver(this.resize);
            this.resizeObserver.observe(this.canvas);
        } else {
            window.addEventListener('resize', this.resize);
        }
        this.resize();
    }

    resize() {
        const rect = this.canvas.getBoundingClientRect();
        const ratio = window.devicePixelRatio || 1;
        this.size.width = rect.width;
        this.size.height = rect.height;
        this.canvas.width = rect.width * ratio;
        this.canvas.height = rect.height * ratio;
        this.ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
    }
    
    render(viewport, history, audioState, bufferRange) {
        const w = this.size.width;
        const h = this.size.height;
        if (!w || !h) return;
        
        this.ctx.fillStyle = CONFIG.COLORS.background;
        this.ctx.fillRect(0, 0, w, h);

        this.drawBufferRange(viewport, bufferRange);
        this.drawTimeline(viewport);
        this.drawWaveform(viewport, history);
        this.drawPlayhead(viewport, audioState);
        this.drawPlaybackInfo(audioState);
    }

    drawBufferRange(viewport, bufferRange) {
        if (!bufferRange?.start || !bufferRange?.end) return;

        const { left, right, duration } = viewport.visibleRange;
        const span = duration;
        if (!span) return;

        const start = Math.max(bufferRange.start, left);
        const end = Math.min(bufferRange.end, right);
        if (end <= start) return;

        const x = ((start - left) / span) * this.size.width;
        const width = ((end - start) / span) * this.size.width;

        this.ctx.fillStyle = CONFIG.COLORS.bufferRange;
        this.ctx.fillRect(x, 0, width, this.size.height);
    }

    drawTimeline(viewport) {
        const { left, right, duration } = viewport.visibleRange;
        if (!duration) return;

        const w = this.size.width;
        const h = this.size.height;
        const { step, format } = this.getTickConfig(duration);

        this.ctx.font = '11px sans-serif';
        this.ctx.fillStyle = '#eee';
        this.ctx.fillText(
            `${TimeUtils.formatTime(left)} – ${TimeUtils.formatTime(right)}`,
            10,
            14
        );

        const startTick = Math.floor(left / step) * step;
        this.ctx.strokeStyle = CONFIG.COLORS.grid;
        this.ctx.fillStyle = CONFIG.COLORS.timeline;

        for (let t = startTick; t <= right; t += step) {
            if (t < left) continue;
            const rel = (t - left) / duration;
            const x = Math.floor(rel * w);

            this.ctx.beginPath();
            this.ctx.moveTo(x, 20);
            this.ctx.lineTo(x, h);
            this.ctx.stroke();

            this.ctx.fillText(TimeUtils.formatTime(t, format), x + 3, 30);
        }
    }

    drawWaveform(viewport, history) {
        const { left, right, duration } = viewport.visibleRange;
        if (!duration) return;

        const w = this.size.width;
        const h = this.size.height;
        const mid = h / 2;
        const visible = history.getVisiblePoints(left, right, w);
        if (!visible.length) return;

        const pxPerMs = w / duration;

        this.ctx.beginPath();
        this.ctx.fillStyle = CONFIG.COLORS.waveformFill;
        this.ctx.strokeStyle = CONFIG.COLORS.waveform;

        for (let i = 0; i < visible.length; i++) {
            const p = visible[i];
            const x = (p.ts - left) * pxPerMs;
            const avg = this.getAmplitude(p);
            const y = mid - avg * (h * 0.45);
            if (i === 0) this.ctx.moveTo(x, y);
            else this.ctx.lineTo(x, y);
        }

        for (let i = visible.length - 1; i >= 0; i--) {
            const p = visible[i];
            const x = (p.ts - left) * pxPerMs;
            const avg = this.getAmplitude(p);
            const y = mid + avg * (h * 0.45);
            this.ctx.lineTo(x, y);
        }

        this.ctx.closePath();
        this.ctx.fill();
        this.ctx.stroke();
    }

    drawPlayhead(viewport, audioState) {
        const { left, right, duration } = viewport.visibleRange;
        if (!duration) return;

        const playbackTime = audioState.currentTime;
        const rel = (playbackTime - left) / duration;
        const x = Math.floor(rel * this.size.width);
        if (x < 0 || x > this.size.width) return;

        const color = audioState.isLive
            ? CONFIG.COLORS.playheadLive
            : CONFIG.COLORS.playheadTimeshift;

        this.ctx.strokeStyle = color;
        this.ctx.lineWidth = 2;
        this.ctx.beginPath();
        this.ctx.moveTo(x, 32);
        this.ctx.lineTo(x, this.size.height);
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

    drawPlaybackInfo(audioState) {
        const h = this.size.height;
        const mode = audioState.isLive ? 'LIVE' : 'TIMESHIFT';
        const time = TimeUtils.formatTime(audioState.currentTime);
        const color = audioState.isLive
            ? CONFIG.COLORS.playheadLive
            : CONFIG.COLORS.playheadTimeshift;

        this.ctx.font = '11px sans-serif';
        this.ctx.fillStyle = color;
        this.ctx.fillText(`▶ ${time} [${mode}]`, 10, h - 10);
    }

    getTickConfig(duration) {
        const config = CONFIG.TICK_STEPS.find((step) => duration <= step.duration);
        return config || { step: 1000, format: 'HH:mm:ss' };
    }

    getAmplitude(point) {
        const source = Number.isFinite(point.smoothedAmp)
            ? point.smoothedAmp
            : point.amp;
        let avg = Number.isFinite(source) ? source : null;

        if (avg === null) {
            const peaks = Array.isArray(point.peaks) ? point.peaks : [0];
            avg = peaks.reduce((a, b) => a + b, 0) / peaks.length;
        }

        const clamped = Math.max(0.05, Math.min(1, avg));
        return clamped;
    }
}
