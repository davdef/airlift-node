class SpecNHoppVisualizer extends BaseVisualizer {
    constructor(ctx, canvas) {
        super(ctx, canvas);

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
    project(x, y, z, camAngle, camHeight, camPitch) {
        const ca = Math.cos(camAngle);
        const sa = Math.sin(camAngle);

        // Rotation um Y
        const rx = x * ca - z * sa;
        const rz = x * sa + z * ca;

        // Kamera-Höhe & Pitch
        const ry = y - camHeight;
        const cp = Math.cos(camPitch);
        const sp = Math.sin(camPitch);

        const ryp = ry * cp - rz * sp;
        const rzp = ry * sp + rz * cp;

        // Perspektive (Tiefe mit Pitch)
        const depth = 1 / (1 + rzp * 0.003);

        return {
            x: rx * depth,
            y: ryp * depth
        };
    }

    draw(frequencyData, timeData, config, deltaTime) {
        const ctx = this.ctx;
        const { width: w, height: h } = this.getCanvasSize();
        const cx = w / 2;
        const cy = h * 0.66;

        // Clear
        ctx.fillStyle = 'rgba(0,0,0,0.25)';
        ctx.fillRect(0, 0, w, h);

        // Kamera
        this.angle += deltaTime * 0.00025;
        this.heightPhase += deltaTime * 0.00015;

        const camAngle = this.angle;
        const camHeight = 140 + Math.sin(this.heightPhase) * 20;
        const camPitch = 0.5 + Math.sin(this.heightPhase * 0.7) * 0.06;

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

        const platformDepth = 12;

        ctx.fillStyle = `${config.secondaryColor}33`;
        ctx.beginPath();
        platform.forEach(([x, z], i) => {
            const p = this.project(x, 0, z, camAngle, camHeight, camPitch);
            if (i === 0) ctx.moveTo(p.x, p.y);
            else ctx.lineTo(p.x, p.y);
        });
        ctx.closePath();
        ctx.fill();
        ctx.stroke();

        // Plattform-Kante
        ctx.beginPath();
        platform.forEach(([x, z], i) => {
            const p = this.project(x, -platformDepth, z, camAngle, camHeight, camPitch);
            if (i === 0) ctx.moveTo(p.x, p.y);
            else ctx.lineTo(p.x, p.y);
        });
        ctx.closePath();
        ctx.stroke();

        platform.forEach(([x, z]) => {
            const p1 = this.project(x, 0, z, camAngle, camHeight, camPitch);
            const p2 = this.project(x, -platformDepth, z, camAngle, camHeight, camPitch);
            ctx.beginPath();
            ctx.moveTo(p1.x, p1.y);
            ctx.lineTo(p2.x, p2.y);
            ctx.stroke();
        });

        // Säulen (segmentiert, echtes Volumen)
        ctx.strokeStyle = config.primaryColor;

        const segHeight = 18;
        const colRadius = 16;
        const colSides = 16;

        for (let i = 0; i < this.columns; i++) {
            const a = (i / this.columns) * Math.PI * 2;
            const baseX = Math.cos(a) * 160;
            const baseZ = Math.sin(a) * 90;

            const segments = Math.min(this.maxSegments, Math.floor(this.level[i]));

            const corners = Array.from({ length: colSides }, (_, j) => {
                const theta = (j / colSides) * Math.PI * 2;
                return [Math.cos(theta) * colRadius, Math.sin(theta) * colRadius];
            });

            const topY = 0;
            const bottomY = -segments * segHeight;

            for (let s = 0; s <= segments; s++) {
                const y = -s * segHeight;
                ctx.beginPath();
                corners.forEach(([dx, dz], j) => {
                    const p = this.project(
                        baseX + dx,
                        y,
                        baseZ + dz,
                        camAngle,
                        camHeight,
                        camPitch
                    );
                    if (j === 0) ctx.moveTo(p.x, p.y);
                    else ctx.lineTo(p.x, p.y);
                });
                ctx.closePath();
                ctx.stroke();
            }

            corners.forEach(([dx, dz]) => {
                const p1 = this.project(
                    baseX + dx,
                    topY,
                    baseZ + dz,
                    camAngle,
                    camHeight,
                    camPitch
                );
                const p2 = this.project(
                    baseX + dx,
                    bottomY,
                    baseZ + dz,
                    camAngle,
                    camHeight,
                    camPitch
                );
                ctx.beginPath();
                ctx.moveTo(p1.x, p1.y);
                ctx.lineTo(p2.x, p2.y);
                ctx.stroke();
            });
        }

        ctx.restore();
    }

    onResize() {}
}
