const WinampPreset = {
    name: "WinAmp Classic",
    config: {
        sensitivity: 7,
        speed: 1.2,
        primaryColor: '#00ff00',
        secondaryColor: '#009900',
        effects: {
            invert: false,
            blur: false,
            trail: true
        }
    },
    
    apply(app) {
        // Konfiguration anwenden
        Object.assign(app.config, this.config);
        
        // UI aktualisieren
        document.getElementById('sensitivity').value = this.config.sensitivity;
        document.getElementById('speed').value = this.config.speed;
        document.getElementById('primary-color').value = this.config.primaryColor;
        document.getElementById('secondary-color').value = this.config.secondaryColor;
        
        // Aktive Buttons setzen
        document.querySelectorAll('.effect-btn').forEach(btn => {
            const effect = btn.id.replace('effect-', '');
            btn.classList.toggle('active', this.config.effects[effect]);
        });
        
        // Visualizer auf Frequenzbalken setzen
        app.setVisualizer('frequencyBars');
        
        console.log(`Preset "${this.name}" geladen`);
    }
};
