// src/config/mod.rs - Angepasst für neues Core-Modell

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use anyhow::Result;

// ============================================================================
// NODE KONFIGURATION
// ============================================================================

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct NodeConfig {
    /// Anzeigename des Nodes
    #[serde(default = "default_node_name")]
    pub name: String,
    
    /// Node-Rolle
    #[serde(default = "default_node_role")]
    pub role: NodeRole,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]  // Default hinzufügen
pub enum NodeRole {
    #[serde(rename = "edge")]
    #[default]  // Edge als Default festlegen
    Edge,
    #[serde(rename = "agent")]
    Agent,
    #[serde(rename = "hub")]
    Hub,
}

// ============================================================================
// PRODUCER KONFIGURATION (Inputs)
// ============================================================================

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ProducerConfig {
    /// Typ: "alsa", "file", "http", "srt"
    pub r#type: String,
    
    /// Aktiviert
    #[serde(default = "default_true")]
    pub enabled: bool,
    
    /// Automatische Format-Erkennung
    #[serde(default = "default_true")]
    pub auto_detect: bool,
    
    /// Puffer-Einstellungen
    #[serde(default)]
    pub buffer: BufferConfig,
    
    /// Typ-spezifische Einstellungen
    #[serde(flatten)]
    pub settings: HashMap<String, serde_json::Value>,
}

// ============================================================================
// PROCESSOR KONFIGURATION (Audio-Verarbeitung)
// ============================================================================

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ProcessorConfig {
    /// Typ: "peak_monitor", "analyzer", "level_meter"
    pub r#type: String,
    
    /// Aktiviert
    #[serde(default = "default_true")]
    pub enabled: bool,
    
    /// Quell-Producer
    pub source: String,
    
    /// Update-Intervall in Millisekunden
    #[serde(default = "default_update_interval")]
    pub update_interval_ms: u64,
    
    /// Typ-spezifische Einstellungen
    #[serde(flatten)]
    pub settings: HashMap<String, serde_json::Value>,
}

// ============================================================================
// ENCODER KONFIGURATION (PCM → Encoded)
// ============================================================================

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct EncoderConfig {
    /// Codec: "opus_ogg", "opus_webrtc", "mp3"
    pub codec: String,
    
    /// Quell-Producer
    pub source: String,
    
    /// Bitrate in bps
    #[serde(default = "default_bitrate")]
    pub bitrate: u32,
    
    /// Ausgabe-Puffer
    #[serde(default)]
    pub buffer: BufferConfig,
    
    /// Codec-spezifische Einstellungen
    #[serde(flatten)]
    pub settings: HashMap<String, serde_json::Value>,
}

// ============================================================================
// CONSUMER KONFIGURATION (Outputs)
// ============================================================================

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ConsumerConfig {
    /// Typ: "icecast", "file", "http", "udp", "srt"
    pub r#type: String,
    
    /// Aktiviert
    #[serde(default = "default_true")]
    pub enabled: bool,
    
    /// Quell-Encoder
    pub source: String,
    
    /// Typ-spezifische Einstellungen
    #[serde(flatten)]
    pub settings: HashMap<String, serde_json::Value>,
}

// ============================================================================
// SERVICE KONFIGURATION (Node-Infrastruktur)
// ============================================================================

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ServiceConfig {
    /// Typ: "http_api", "monitoring", "discovery"
    pub r#type: String,
    
    /// Aktiviert
    #[serde(default = "default_true")]
    pub enabled: bool,
    
    /// Bind-Adresse (für Netzwerk-Services)
    #[serde(default)]
    pub bind: Option<String>,
    
    /// Typ-spezifische Einstellungen
    #[serde(flatten)]
    pub settings: HashMap<String, serde_json::Value>,
}

// ============================================================================
// BUFFER KONFIGURATION
// ============================================================================

#[derive(Debug, Deserialize, Serialize, Clone, Default)]  // Default hinzufügen
pub struct BufferConfig {
    /// Anzahl Slots im Puffer
    #[serde(default = "default_buffer_slots")]
    pub slots: usize,
    
    /// Pre-allokierte Samples
    #[serde(default = "default_prealloc_samples")]
    pub prealloc_samples: usize,
}

// ============================================================================
// HAUPTCONFIG
// ============================================================================

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Config {
    /// Node-Konfiguration
    #[serde(default)]
    pub node: NodeConfig,
    
    /// Producer (Inputs)
    #[serde(default)]
    pub producers: HashMap<String, ProducerConfig>,
    
    /// Processor (Audio-Verarbeitung)
    #[serde(default)]
    pub processors: HashMap<String, ProcessorConfig>,
    
    /// Encoder (PCM → Encoded)
    #[serde(default)]
    pub encoders: HashMap<String, EncoderConfig>,
    
    /// Consumer (Outputs)
    #[serde(default)]
    pub consumers: HashMap<String, ConsumerConfig>,
    
    /// Services (Infrastruktur)
    #[serde(default)]
    pub services: HashMap<String, ServiceConfig>,
}

// ============================================================================
// DEFAULT-WERTE
// ============================================================================

fn default_node_name() -> String {
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "airlift-node".to_string())
}

fn default_node_role() -> NodeRole {
    NodeRole::Edge
}

fn default_true() -> bool {
    true
}

fn default_buffer_slots() -> usize {
    5000
}

fn default_prealloc_samples() -> usize {
    9600
}

fn default_update_interval() -> u64 {
    100  // 100ms
}

fn default_bitrate() -> u32 {
    128000  // 128 kbps
}

// ============================================================================
// IMPLEMENTIERUNGEN
// ============================================================================

impl Default for Config {
    fn default() -> Self {
        Self {
            node: NodeConfig {
                name: default_node_name(),
                role: default_node_role(),
            },
            producers: HashMap::new(),
            processors: HashMap::new(),
            encoders: HashMap::new(),
            consumers: HashMap::new(),
            services: HashMap::new(),
        }
    }
}

impl Config {
    /// Lädt Konfiguration aus Datei
    pub fn load(path: &str) -> Result<Self> {
        if !std::path::Path::new(path).exists() {
            return Ok(Self::default());
        }
        
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        
        // Einfache Validierung
        config.validate()?;
        
        Ok(config)
    }
    
    /// Prüft ob Konfiguration Audio-Komponenten enthält
    pub fn has_audio_components(&self) -> bool {
        !self.producers.is_empty() ||
        !self.processors.is_empty() ||
        !self.encoders.is_empty() ||
        !self.consumers.is_empty()
    }
    
    /// Prüft ob nur API-Services aktiv sind
    pub fn is_api_only(&self) -> bool {
        !self.has_audio_components() && 
        !self.services.is_empty()
    }
    
    /// Validierung der Konfiguration
    fn validate(&self) -> Result<()> {
        // 1. Quell-Referenzen validieren
        for (name, processor) in &self.processors {
            if !self.producers.contains_key(&processor.source) {
                return Err(anyhow::anyhow!(
                    "Processor '{}' references unknown producer '{}'",
                    name, processor.source
                ));
            }
        }
        
        for (name, encoder) in &self.encoders {
            if !self.producers.contains_key(&encoder.source) {
                return Err(anyhow::anyhow!(
                    "Encoder '{}' references unknown producer '{}'",
                    name, encoder.source
                ));
            }
        }
        
        for (name, consumer) in &self.consumers {
            if !self.encoders.contains_key(&consumer.source) {
                return Err(anyhow::anyhow!(
                    "Consumer '{}' references unknown encoder '{}'",
                    name, consumer.source
                ));
            }
        }
        
        // 2. Port-Konflikte prüfen (vereinfacht)
        let mut ports = HashMap::new();
        for (name, service) in &self.services {
            if let Some(bind) = &service.bind {
                if let Some(port) = extract_port(bind) {
                    if let Some(existing) = ports.insert(port, name) {
                        return Err(anyhow::anyhow!(
                            "Port conflict: Service '{}' and '{}' both use port {}",
                            existing, name, port
                        ));
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Leere Konfiguration (für API-only Mode)
    pub fn empty() -> Self {
        Self::default()
    }
}

/// Extrahiert Port aus Bind-String (z.B. "0.0.0.0:3008" → 3008)
fn extract_port(bind: &str) -> Option<u16> {
    bind.split(':').last().and_then(|p| p.parse().ok())
}
