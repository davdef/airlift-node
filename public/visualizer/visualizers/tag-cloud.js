class TagCloudVisualizer extends BaseVisualizer {
    constructor(ctx, canvas, options = {}) {
        super(ctx, canvas);

        this.container = document.querySelector('.tag-cloud');
        if (!this.container) {
            this.container = document.createElement('div');
            this.container.className = 'tag-cloud';
            document.querySelector('.visualizer-container')?.appendChild(this.container);
        }

        this.apiUrl = options.apiUrl
            || document.body?.dataset?.yamnetApi
            || '/yamnet';
        this.refreshInterval = options.refreshInterval ?? 4000;
        this.lastFetch = 0;
        this.tags = [];
        this.isActive = false;
        this.fetchInFlight = false;
    }

    setActive(isActive) {
        this.isActive = isActive;
        this.container?.classList.toggle('is-active', isActive);
    }

    async fetchTags() {
        if (this.fetchInFlight) return;

        this.fetchInFlight = true;
        this.lastFetch = performance.now();

        try {
            const response = await fetch(this.apiUrl, { cache: 'no-store' });
            if (!response.ok) return;

            const payload = await response.json();
            const tags = this.normalizeTags(payload);
            if (tags.length) {
                this.tags = tags.slice(0, 28);
                this.renderTags();
            }
        } catch (error) {
            // Netzwerkfehler tolerieren
        } finally {
            this.fetchInFlight = false;
        }
    }

    normalizeTags(payload) {
        const raw = Array.isArray(payload)
            ? payload
            : (payload?.tags
                || payload?.labels
                || payload?.results
                || payload?.predictions
                || []);

        if (!Array.isArray(raw)) return [];

        return raw
            .map(item => {
                if (typeof item === 'string') {
                    return { label: item, score: 1 };
                }

                if (typeof item === 'object' && item) {
                    const label = item.label
                        || item.tag
                        || item.name
                        || item.class
                        || item.category;
                    const score = item.score
                        ?? item.confidence
                        ?? item.probability
                        ?? item.value
                        ?? 1;

                    return label ? { label, score: Number(score) || 0 } : null;
                }

                return null;
            })
            .filter(Boolean);
    }

    renderTags() {
        if (!this.container) return;

        const scores = this.tags.map(tag => tag.score);
        const minScore = Math.min(...scores, 0);
        const maxScore = Math.max(...scores, 1);
        const range = maxScore - minScore || 1;

        this.container.innerHTML = '';

        this.tags.forEach(tag => {
            const scale = (tag.score - minScore) / range;
            const tagEl = document.createElement('span');
            tagEl.className = 'tag-cloud__tag';
            tagEl.style.setProperty('--tag-scale', scale.toFixed(2));
            tagEl.textContent = tag.label;
            this.container.appendChild(tagEl);
        });
    }

    draw(frequencyData, timeData, config, deltaTime) {
        const { width, height } = this.getCanvasSize();
        this.ctx.clearRect(0, 0, width, height);

        if (!this.isActive) return;

        const now = performance.now();
        if (now - this.lastFetch >= this.refreshInterval) {
            void this.fetchTags();
        }
    }

    onResize() {
        if (this.isActive) {
            this.renderTags();
        }
    }
}
