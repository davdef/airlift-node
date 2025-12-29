# Airlift Node

Airlift Node ist eine Audio-Pipeline für PCM-Frames mit klarer Trennung der
Rollen **AirliftNode → Flow → Producer/Processor/Consumer**. Die aktuelle
Implementierung ist in `src/core/node.rs` zu finden und bildet die Grundlage
für Konfiguration, Start und Laufzeitverarbeitung.

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
- `processor_buffers`: Zwischenbuffer zwischen den einzelnen Processors.
- `output_buffer`: Endbuffer nach der Processor-Kette.
- `processors`: Liste der Processor-Instanzen (Reihenfolge = Kette).
- `consumers`: Liste der Consumer-Instanzen, die am `output_buffer` hängen.

Die Verarbeitung läuft wie folgt:

1. **Inputs** liefern Frames in `input_buffers`.
2. **Merge**: Der Flow sammelt alle Frames in `input_merge_buffer`.
3. **Processor-Kette**: Jeder Processor liest aus dem jeweiligen Input-Buffer
   und schreibt in den nächsten Buffer der Kette (`processor_buffers`).
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

Die API ist **noch nicht implementiert**. Zielbild:

- **Remote-Steuerung** (Start/Stop/Status)
- **Flow-CRUD** (Create/Update/Delete von Flows)
- **Headless-Betrieb** (Betrieb ohne UI, komplett über API steuerbar)

## Einstiegspunkte

- **Runtime/Bootstrap**: `src/main.rs`
- **Core-Pipeline**: `src/core/node.rs`
- **Processor-Implementierungen**: `src/core/processor/*`, `src/processors/*`

## Ringbuffer-Auswahl

Standardmäßig wird `src/core/ringbuffer.rs` verwendet. Die Lockfree-Variante
`src/core/ringbuffer_lockfree.rs` ist nur aktiv, wenn das Feature
`lockfree` gesetzt ist. Externe Nutzung importiert immer
`crate::core::ringbuffer`, unabhängig vom Feature-Flag.
- **Consumers**: `src/core/consumer/*`
- **Tests**: `src/core/*.rs`, `src/processors/mixer.rs`, `tests/*`
