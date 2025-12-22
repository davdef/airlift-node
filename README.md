# Airlift Node

Airlift Node ist ein Audio-Node, der kontinuierliche PCM-Audioframes annimmt,
über eine Ringpuffer-Pipeline verarbeitet und als Streams, Dateien oder HTTP-
Endpunkte bereitstellt. Das System bringt eine modulare Codec-Schicht,
Monitoring sowie eine Steuer- und Status-API mit.

## Aufgaben

- **Audioeingang**: SRT- und ALSA-Quellen in den gemeinsamen Ringpuffer führen.
- **Verarbeitung**: Codec-Instanzen auf feste PCM-Frames anwenden.
- **Ausgabe**: Icecast/SRT-Streams, Recorder (WAV/MP3) und Audio-HTTP-Endpoints.
- **Betrieb**: Status/Steuerung via API, Monitoring über Metrics & Health.

## Zentrale Bausteine

- **Ring** (`src/ring`): geteilte Audio-Drehscheibe mit Puffer- und Statistiklogik.
- **Codecs** (`src/codecs`): Instanzbasierte Encoder mit eigener Konfiguration.
- **I/O** (`src/io`): Eingänge (SRT/ALSA) und Ausgänge (Icecast/SRT).
- **Recorder** (`src/recorder`): WAV/MP3-Aufzeichnung und Retention.
- **API/Web** (`src/api`, `src/web`): Status, Steuerung, Peaks & History.
- **Services** (`src/services`): API-, Audio-HTTP- und Monitoring-Server.

## Datenfluss (vereinfacht)

```mermaid
flowchart LR
    SRTIn[SRT Input] --> Ring[Audio Ring]
    ALSAIn[ALSA Input] --> Ring

    Ring --> Codecs[Codec-Instanzen]
    Codecs --> Icecast[Icecast Out]
    Codecs --> SRTOut[SRT Out]

    Ring --> Recorder[Recorder]
    Ring --> AudioHTTP[Audio HTTP Endpoints]

    API[HTTP API] --> Control[Control State]
    Control --> Modules[Module Registry]

    Monitoring[Monitoring] --> Metrics[(Metrics/Health)]
```

## Einstiegspunkte

- **Bootstrap & Runtime**: `src/main.rs`, `src/bootstrap.rs`
- **Konfiguration**: `src/config.rs` (TOML)
- **Module-Registry**: `src/api/registry.rs`

## Konfiguration

Die Konfiguration erfolgt über eine TOML-Datei (Default: `config.toml`). Beim
Start kann eine alternative Datei als erstes Argument übergeben werden:

```
airlift-node ./config.vps.toml
```

### Beispielkonfiguration

Die Datei `config.sample.toml` ist eine kommentierte Referenz für die aktuelle
Graph-Pipeline. Wichtige Punkte:

- **Graph aktivieren**: Sobald `ringbuffers`, `inputs`, `outputs` oder
  `services` gesetzt sind, wird die Graph-Pipeline genutzt.
- **Genau ein Ringbuffer & ein Input**: Aktuell ist genau ein Ringbuffer und
  genau ein Input-Typ erlaubt.
- **Input-Typen**:
  - `srt` (Listener, benötigt `listen` + `latency_ms`)
  - `http_stream`/`icecast` (benötigt `url`, erwartet Ogg/Opus)
- **Buffer-Verknüpfung**: `buffer` muss exakt der Ringbuffer-ID entsprechen
  (im Sample: `main`).
- **Codecs**: Jeder Output benötigt ein `codec_id`, das in `[codecs.*]`
  definiert ist.

### VPS-Konfiguration (Icecast)

Die Datei `config.vps.toml` ist für den Betrieb mit dem RFM-Icecast-Stream
vorbereitet (`https://icecast.radiorfm.de/rfm.ogg`, 96 kbit/s Opus). Sie kann
die lokale `config.toml` ersetzen oder direkt beim Start angegeben werden.
