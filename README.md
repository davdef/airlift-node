# Airlift Node

Airlift Node ist eine Audio-Pipeline für PCM-Frames mit klarer Trennung der
Rollen **AirliftNode → Flow → Producer/Processor/Consumer**. Die aktuelle
Implementierung ist in `src/core/node.rs` zu finden und bildet die Grundlage
für Konfiguration, Start und Laufzeitverarbeitung.

## Architektur

Eine detaillierte Beschreibung der Pipeline, des Datenflusses, des
Buffer-Lifecycles und des Threading-Modells findet sich in
[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

## Aktuelle Pipeline-Struktur (AirliftNode → Flow → Producer/Processor/Consumer)

Die zentrale Pipeline besteht aus:

- **AirliftNode** (`src/core/node.rs`): Orchestriert Produzenten und Flows,
  hält die Buffer-Registry und startet/stoppt das System als Ganzes.
- **Producer** (`src/core/mod.rs` + Implementierungen in `src/producers/*`):
  Schreiben Audio-Frames in Ringbuffer, die vom Node verwaltet werden.
- **Flow** (`src/core/node.rs`): Verbindet mehrere Producer-Inputs, führt sie
  zusammen, verarbeitet sie über eine Processor-Kette und verteilt an Consumer.
- **Processor** (`src/core/processor/*`, `src/processors/*`): Transformieren
  den Audiostream (z. B. PassThrough, Gain, Mixer).
- **Consumer** (`src/core/consumer/*`): Konsumieren den Flow-Output
  (z. B. FileWriter).

`AirliftNode` verwaltet mehrere `Flow`-Instanzen. Jeder Flow besitzt eigene
Ringbuffer und wird von einem Processing-Thread bedient.

## Flow-Struktur (Inputs → Merge → Processor-Kette → Output → Consumers)

Die `Flow`-Struktur in `src/core/node.rs` besteht aus folgenden Feldern:

- `input_buffers`: Alle Input-Ringbuffer, die von Producers/Inputs befüllt
  werden.
- `input_merge_buffer`: Sammelbuffer, in den die Frames aus allen
  `input_buffers` zusammengeführt werden.
- `processor_buffers`: Segment-Buffer zwischen einzelnen Processors
  (Legacy: immer vorhanden, Simplified: optional pro Teilstrecke).
- `output_buffer`: Endbuffer nach der Processor-Kette.
- `processors`: Liste der Processor-Instanzen (Reihenfolge = Kette).
- `consumers`: Liste der Consumer-Instanzen, die am `output_buffer` hängen.

Die Verarbeitung läuft wie folgt:

1. **Inputs** liefern Frames in `input_buffers`.
2. **Merge**: Der Flow sammelt alle Frames in `input_merge_buffer`.
3. **Processor-Kette**: Jeder Processor liest aus dem jeweiligen Input-Buffer
   und schreibt in den nächsten Buffer der Kette. Segment-Buffer sind optional;
   nicht gepufferte Teilstrecken werden über interne Scratch-Buffer verkettet.
4. **Output**: Der letzte Processor schreibt in `output_buffer`.
5. **Consumers** lesen aus `output_buffer` und geben die Daten aus.

## Startmodi

Der Einstiegspunkt ist `src/main.rs`. Es gibt drei Startmodi:

1. **Normaler Modus** (Standard)
   - Start ohne Argumente: `cargo run --`
   - Lädt `config.toml` (oder Default), baut Node/Flows/Processor/Consumer und
     startet die Verarbeitung.

2. **ALSA-Discovery** (`--discover`)
   - `cargo run -- --discover`
   - Listet verfügbare Audio-Devices als JSON und Log-Ausgabe.

3. **Device-Test** (`--test-device <id>`)
   - `cargo run -- --test-device hw:0,0`
   - Führt einen Kurztest gegen das angegebene Device durch und gibt
     Format-Informationen sowie JSON-Ausgabe zurück.

## Konfigurationen

Für verschiedene Umgebungen liegen fertige Konfigurationsdateien unter
`config/`:

- `config/development.toml` – lokale Entwicklung mit Sine-Generator.
- `config/production.toml` – Beispiel für ALSA-Input (anpassen!).
- `config/docker.toml` – Docker-Setup (Sine-Generator + Datei-Output).

Der Node lädt immer `config.toml` im aktuellen Working Directory. Für lokale
Starts kann die passende Konfiguration kopiert werden:

```bash
cp config/development.toml config.toml
```

## Lokaler Start (Development)

```bash
mkdir -p data
cp config/development.toml config.toml
cargo run --release
```

Monitoring: `http://localhost:8087/metrics` und `http://localhost:8087/health`.

## Docker-Start (Node + Monitoring)

```bash
mkdir -p data
docker compose up --build
```

- Airlift Node: `http://localhost:8087/metrics`
- Prometheus: `http://localhost:9090`

Die Datei-Ausgabe landet unter `./data/output.wav`.

## Systemd-Deployment (Production)

1. Binary installieren (Beispielpfade):

   ```bash
   sudo install -m 0755 target/release/airlift-node /opt/airlift-node/airlift-node
   sudo install -d -m 0755 /etc/airlift-node /var/lib/airlift-node
   sudo cp config/production.toml /etc/airlift-node/config.toml
   ```

2. Service-User anlegen:

   ```bash
   sudo useradd --system --no-create-home --shell /usr/sbin/nologin airlift
   sudo chown -R airlift:airlift /var/lib/airlift-node
   ```

3. Systemd-Unit installieren und starten:

   ```bash
   sudo cp deploy/airlift-node.service /etc/systemd/system/airlift-node.service
   sudo systemctl daemon-reload
   sudo systemctl enable --now airlift-node
   ```

4. Status prüfen:

   ```bash
   sudo systemctl status airlift-node
   ```

## Examples

Die Beispielprogramme nutzen die bestehenden `AirliftNode`/`Flow`-Strukturen
und können direkt über Cargo gestartet werden:

- **Simple Recording** → `cargo run --example simple_recording`
- **Live Streaming (encoded ring)** → `cargo run --example live_streaming`
- **Audio Mixing (Mixer + zwei Sine-Producer)** → `cargo run --example audio_mixing`
- **Custom Processor (eigene Processor-Implementierung)** → `cargo run --example custom_processor`

## Test-Übersicht

### Unit-Tests

Unit-Tests sind direkt in den Modulen definiert (per `#[cfg(test)]`).
Beispiele:

- `src/core/mod.rs` (ProducerStatus-Validierung)
- `src/core/node.rs` (Flow/AirliftNode-Tests)
- `src/core/ringbuffer.rs` (Default) und optional `src/core/ringbuffer_lockfree.rs` (via Feature `lockfree`)
- `src/core/logging.rs`
- `src/processors/mixer.rs`

### Integration-Tests

Integration-Tests liegen unter `tests/` und `tests/integration/`:

- `tests/integration/logging_test.rs`
- `tests/integration_tests.rs`
- `tests/run_logging_tests.rs`
- `tests/standalone_test.rs`

## Testausführung

- **Alle Tests:** `cargo test`
- **Nur Integration-Tests:** `cargo test --tests`
- **E2E-Flow-Test:** `cargo test --test flow_e2e`
- **Benchmarks (Mixer/Ringbuffer):** `cargo bench`

## API-Übersicht (geplant)

Aktuell verfügbar:

- **POST `/api/config`**: Runtime-Konfigurationsupdates via JSON-Patch.
- **GET `/health`**: Monitoring-Healthcheck (200 = ok, 503 = not running).
- **GET `/metrics`**: Prometheus-kompatible Metriken (Frames processed, Buffer-Auslastung, Latenz).

Geplantes Zielbild:

- **Remote-Steuerung** (Start/Stop/Status)
- **Flow-CRUD** (Create/Update/Delete von Flows)
- **Headless-Betrieb** (Betrieb ohne UI, komplett über API steuerbar)

## Einstiegspunkte

- **Runtime/Bootstrap**: `src/main.rs`
- **Core-Pipeline**: `src/core/node.rs`
- **Processor-Implementierungen**: `src/core/processor/*`, `src/processors/*`

## Ringbuffer-Auswahl

Standardmäßig wird `src/core/ringbuffer.rs` verwendet (Mutex/RwLock-basierte
Synchronisierung). Die Lockfree-Variante `src/core/ringbuffer_lockfree.rs`
ist nur aktiv, wenn das Feature `lockfree` gesetzt ist:

- **Default (ohne Feature):** `src/core/ringbuffer.rs` – stabiler, einfacher
  zu debuggen, geeignet für die meisten Deployments.
- **Feature `lockfree`:** `src/core/ringbuffer_lockfree.rs` – optimiert für
  hohe Parallelität/Contention und geringere Lock-Overheads, dafür mit
  komplexerer Debugging-Oberfläche.

Aktivierung: `cargo build --features lockfree`. Externe Nutzung importiert
immer `crate::core::ringbuffer`, unabhängig vom Feature-Flag.
- **Consumers**: `src/core/consumer/*`
- **Tests**: `src/core/*.rs`, `src/processors/mixer.rs`, `tests/*`
