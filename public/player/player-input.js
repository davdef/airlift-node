class PlayerInputHandler {
    constructor(player, canvas) {
        this.player = player;
        this.canvas = canvas;

        this.isDragging = false;
        this.dragStartX = null;
        this.lastWheelAt = 0;

        this.handleMouseDown = this.handleMouseDown.bind(this);
        this.handleMouseMove = this.handleMouseMove.bind(this);
        this.handleMouseUp = this.handleMouseUp.bind(this);
        this.handleWheel = this.handleWheel.bind(this);
        this.handleTouchStart = this.handleTouchStart.bind(this);
        this.handleTouchMove = this.handleTouchMove.bind(this);
        this.handleTouchEnd = this.handleTouchEnd.bind(this);
        this.handleKeyDown = this.handleKeyDown.bind(this);
    }

    attach() {
        this.canvas.addEventListener('mousedown', this.handleMouseDown);
        window.addEventListener('mousemove', this.handleMouseMove);
        window.addEventListener('mouseup', this.handleMouseUp);

        this.canvas.addEventListener('touchstart', this.handleTouchStart, { passive: false });
        this.canvas.addEventListener('touchmove', this.handleTouchMove, { passive: false });
        this.canvas.addEventListener('touchend', this.handleTouchEnd);

        this.canvas.addEventListener('wheel', this.handleWheel, { passive: false });
        document.addEventListener('keydown', this.handleKeyDown);
    }

    detach() {
        this.canvas.removeEventListener('mousedown', this.handleMouseDown);
        window.removeEventListener('mousemove', this.handleMouseMove);
        window.removeEventListener('mouseup', this.handleMouseUp);

        this.canvas.removeEventListener('touchstart', this.handleTouchStart);
        this.canvas.removeEventListener('touchmove', this.handleTouchMove);
        this.canvas.removeEventListener('touchend', this.handleTouchEnd);

        this.canvas.removeEventListener('wheel', this.handleWheel);
        document.removeEventListener('keydown', this.handleKeyDown);
    }

    handleMouseDown(e) {
        if (e.button !== 0) {
            return;
        }

        if (Date.now() - this.lastWheelAt < 120) {
            return;
        }

        this.isDragging = false;
        this.dragStartX = e.clientX;
        this.player.viewport.followLive = false;
    }

    handleMouseMove(e) {
        if (this.dragStartX === null) {
            return;
        }

        const dx = e.clientX - this.dragStartX;

        if (!this.isDragging && Math.abs(dx) > 3) {
            this.isDragging = true;
        }

        if (this.isDragging) {
            const width = this.canvas.clientWidth;
            this.player.viewport.pan(dx, width);
            this.dragStartX = e.clientX;
            this.player.cacheValid = false;
        }
    }

    handleMouseUp(e) {
        if (this.dragStartX === null) {
            return;
        }

        if (!this.isDragging) {
            this.seekAtClientX(e.clientX);
        }

        this.resetDrag();
    }

    handleWheel(e) {
        e.preventDefault();
        this.lastWheelAt = Date.now();

        if (this.dragStartX !== null) {
            this.resetDrag();
        }

        const factor = e.deltaY > 0 ? 1.25 : 0.8;
        this.zoomAtClientX(e.clientX, factor);
    }

    handleTouchStart(e) {
        e.preventDefault();

        if (e.touches.length === 1) {
            this.dragStartX = e.touches[0].clientX;
            this.player.viewport.followLive = false;
        } else if (e.touches.length === 2) {
            const dx = e.touches[0].clientX - e.touches[1].clientX;
            const dy = e.touches[0].clientY - e.touches[1].clientY;
            const distance = Math.sqrt(dx * dx + dy * dy);
            this.player.viewport.startPinch(distance);
        }
    }

    handleTouchMove(e) {
        e.preventDefault();

        if (e.touches.length === 2 && this.player.viewport.pinchStart !== null) {
            const dx = e.touches[0].clientX - e.touches[1].clientX;
            const dy = e.touches[0].clientY - e.touches[1].clientY;
            const distance = Math.sqrt(dx * dx + dy * dy);
            this.player.viewport.updatePinch(distance);
            this.player.cacheValid = false;
        } else if (e.touches.length === 1 && this.dragStartX !== null) {
            const dx = e.touches[0].clientX - this.dragStartX;
            const width = this.canvas.clientWidth;

            if (!this.isDragging && Math.abs(dx) > 5) {
                this.isDragging = true;
            }

            if (this.isDragging) {
                this.player.viewport.pan(dx, width);
                this.dragStartX = e.touches[0].clientX;
                this.player.cacheValid = false;
            }
        }
    }

    handleTouchEnd(e) {
        if (!this.isDragging && this.dragStartX !== null && e.changedTouches.length === 1) {
            this.seekAtClientX(e.changedTouches[0].clientX);
        }

        this.resetDrag();
        this.player.viewport.endPinch();
    }

    handleKeyDown(e) {
        if (e.target.matches('input, textarea, [contenteditable="true"]')) {
            return;
        }

        if (e.code === 'Space') {
            e.preventDefault();
            this.player.togglePlayback();
            return;
        }

        const seekStep = e.shiftKey ? 30000 : 5000;
        if (e.code === 'ArrowLeft') {
            e.preventDefault();
            this.player.seekRelative(-seekStep);
            return;
        }

        if (e.code === 'ArrowRight') {
            e.preventDefault();
            this.player.seekRelative(seekStep);
            return;
        }

        if (e.code === 'ArrowUp' || e.code === 'ArrowDown') {
            e.preventDefault();
            const factor = e.code === 'ArrowUp' ? 0.8 : 1.25;
            const adjusted = e.shiftKey ? (e.code === 'ArrowUp' ? 0.6 : 1.6) : factor;
            const width = this.canvas.clientWidth;
            this.player.viewport.zoom(adjusted, width / 2, width);
            this.player.cacheValid = false;
        }
    }

    zoomAtClientX(clientX, factor) {
        const rect = this.canvas.getBoundingClientRect();
        const centerX = clientX - rect.left;
        const width = this.canvas.clientWidth;
        this.player.viewport.zoom(factor, centerX, width);
        this.player.cacheValid = false;
    }

    seekAtClientX(clientX) {
        const rect = this.canvas.getBoundingClientRect();
        const x = clientX - rect.left;
        const width = this.canvas.clientWidth;
        const { left, duration } = this.player.viewport.visibleRange;
        const targetTime = left + (x / width) * duration;
        this.player.seekTo(targetTime);
    }

    resetDrag() {
        this.isDragging = false;
        this.dragStartX = null;
    }
}
