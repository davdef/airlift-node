export class TimeUtils {
    static formatTime(ts, format = 'HH:mm:ss') {
        if (!Number.isFinite(ts)) return '--:--:--';
        const d = new Date(ts);
        
        switch(format) {
            case 'ss': return d.getSeconds().toString().padStart(2, '0');
            case 'mm:ss': 
                return `${d.getMinutes().toString().padStart(2, '0')}:${d.getSeconds().toString().padStart(2, '0')}`;
            case 'HH:mm':
                return `${d.getHours().toString().padStart(2, '0')}:${d.getMinutes().toString().padStart(2, '0')}`;
            case 'HH:mm:ss':
            default:
                return `${d.getHours().toString().padStart(2, '0')}:${d.getMinutes().toString().padStart(2, '0')}:${d.getSeconds().toString().padStart(2, '0')}`;
        }
    }
    
    static getTickConfig(duration) {
        // Diese Funktion muss noch mit CONFIG verbunden werden
        // Für jetzt einen einfachen Fallback
        return { step: 1000, format: 'HH:mm:ss' };
    }
    
    static calculateOptimalTickSpacing(canvasWidth, viewportDuration) {
        // Platzhalter - wird später mit CONFIG verbunden
        return { step: 1000, format: 'HH:mm:ss' };
    }
}
