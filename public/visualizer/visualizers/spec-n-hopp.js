class SpecNHoppVisualizer {
    constructor(ctx, canvas) {
        this.ctx = ctx;
        this.canvas = canvas;

        this.columns = 8;
        this.maxSegments = 10;

        this.level = new Float32Array(this.columns);
        this.vel = new Float32Array(this.columns);

        this.stiffness = 0.22;
        this.damping = 0.72;

        // Kamera
        this.angle = 0;
        this.heightPhase = 0;
    }

    // 3D → 2D Projektion (OHNE Skalierung!)
    project(x, y, z, camAngle, camHeight) {
        const ca = Math.cos(camAngle);
        const sa = Math.sin(camAngle);

        // Rotation um Y
        const rx = x * ca - z * sa;
        const rz = x * sa + z * ca;

        // Perspektive (nur Tiefe!)
        const depth = 1 / (1 + rz * 0.002);

        return {
            x: rx * depth,
            y: (y - camHeight) * depth
        };
    }

    draw(frequencyData, timeData, config, deltaTime) {
        const ctx = this.ctx;
        const w = this.canvas.width;
        const h = this.canvas.height;
        const cx = w / 2;
        const cy = h / 2;

        // Clear
        ctx.fillStyle = 'rgba(0,0,0,0.25)';
        ctx.fillRect(0, 0, w, h);

        // Kamera
        this.angle += deltaTime * 0.00025;
        this.heightPhase += deltaTime * 0.00015;

        const camAngle = this.angle;
        const camHeight = Math.max(-10, Math.sin(this.heightPhase) * 80);

        // FFT → 8 echte Bänder
        const binsPerCol = Math.max(1, Math.floor(frequencyData.length / this.columns));
        for (let i = 0; i < this.columns; i++) {
            let sum = 0;
            const start = i * binsPerCol;
            const end = Math.min(frequencyData.length, start + binsPerCol);
            for (let b = start; b < end; b++) sum += frequencyData[b];

            const amp = (sum / (end - start)) / 255;
            const target = amp * this.maxSegments;

            this.vel[i] += (target - this.level[i]) * this.stiffness;
            this.vel[i] *= this.damping;
            this.level[i] += this.vel[i];
        }

        ctx.save();
        ctx.translate(cx, cy);

        // Plattform (stabil!)
        ctx.strokeStyle = config.secondaryColor;
        ctx.lineWidth = 1;

        const platform = [
            [-260, -160],
            [ 260, -160],
            [ 260,  160],
            [-260,  160]
        ];

        ctx.beginPath();
        platform.forEach(([x, z], i) => {
            const p = this.project(x, 0, z, camAngle, 0);
            if (i === 0) ctx.moveTo(p.x, p.y);
            else ctx.lineTo(p.x, p.y);
        });
        ctx.closePath();
        ctx.stroke();

        // Säulen (segmentiert, echtes Volumen)
        ctx.strokeStyle = config.primaryColor;

        const segHeight = 18;
        const colHalf = 14;

        for (let i = 0; i < this.columns; i++) {
            const a = (i / this.columns) * Math.PI * 2;
            const baseX = Math.cos(a) * 160;
            const baseZ = Math.sin(a) * 90;

            const segments = Math.min(this.maxSegments, Math.floor(this.level[i]));

            const corners = [
                [-colHalf, -colHalf],
                [ colHalf, -colHalf],
                [ colHalf,  colHalf],
                [-colHalf,  colHalf]
            ];

            for (let s = 0; s < segments; s++) {
                const yTop = -s * segHeight;
                const yBot = yTop - segHeight;

                // Ring
                ctx.beginPath();
                corners.forEach(([dx, dz], j) => {
                    const p = this.project(
                        baseX + dx,
                        yBot,
                        baseZ + dz,
                        camAngle,
                        camHeight
                    );
                    if (j === 0) ctx.moveTo(p.x, p.y);
                    else ctx.lineTo(p.x, p.y);
                });
                ctx.closePath();
                ctx.stroke();

                // Vertikale Kanten (sparsam)
                if ((s & 1) === 0) {
                    corners.forEach(([dx, dz]) => {
                        const p1 = this.project(
                            baseX + dx,
                            yTop,
                            baseZ + dz,
                            camAngle,
                            camHeight
                        );
                        const p2 = this.project(
                            baseX + dx,
                            yBot,
                            baseZ + dz,
                            camAngle,
                            camHeight
                        );
                        ctx.beginPath();
                        ctx.moveTo(p1.x, p1.y);
                        ctx.lineTo(p2.x, p2.y);
                        ctx.stroke();
                    });
                }
            }
        }

        ctx.restore();
    }

    onResize() {}
}
