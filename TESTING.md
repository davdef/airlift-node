# Testing

## Philosophie

Das Projekt priorisiert Integrationstests, weil die Audio-Pipeline nur in realistischen
End-to-End-Flows zuverlässig validiert werden kann (Buffering, Timing, Verbindungen,
Threading, Logging). Unit-Tests sind auf wenige, klar abgegrenzte Fälle beschränkt,
bei denen interne Details zwingend erforderlich sind und eine öffentliche Abstraktion
nicht existiert.

## Unit-Test vs. Integrationstest im Projekt

**Unit-Tests (src/)**
- Dürfen interne Details prüfen, wenn keine öffentliche API existiert.
- Werden sparsam eingesetzt (z. B. Konfigurations-Invarianten innerhalb eines Moduls).

**Integrationstests (tests/)**
- Nutzen ausschließlich die Public API (`airlift_node::...`).
- Verifizieren vollständige Flows oder beobachtbares Verhalten (z. B. Ringbuffer-Reads,
  Codec-Roundtrip, Logging-Format, Producer-Status).

## Was bewusst nicht getestet wird

- Private Flow-Interna, die keine Public API besitzen (z. B. private Zustandsfelder,
  interne Reader-Positionen, private Puffer-Implementierungsdetails).
- Nicht-deterministische Timing-Details, die nur über interne Hooks sichtbar wären.

Wenn ein fachlich sinnvoller Test nur durch Zugriff auf private Felder möglich wäre,
bleibt er **bewusst ungetestet** und wird hier dokumentiert, statt die API dafür
aufzuweichen oder spezielle Getter einzuführen.

## Beispiele

**Guter Test (E2E Flow über Public API)**
```rust
use airlift_node::{AudioRingBuffer, PcmFrame};

let buffer = AudioRingBuffer::new(8);
buffer.push(PcmFrame { utc_ns: 0, samples: vec![0; 96], sample_rate: 48_000, channels: 2 });
let frame = buffer.pop().expect("frame");
assert_eq!(frame.samples.len(), 96);
```

**Schlechter Test (Zugriff auf private Felder)**
```rust
// NICHT erlaubt: private Felder sind keine stabile API
let mixer = Mixer::new("test");
assert_eq!(mixer.output_sample_rate, 48_000);
```

## Neue Tests korrekt hinzufügen

1. **Prüfen, ob der Test über die Public API möglich ist.**
2. **Integrationstest in `tests/` anlegen.**
   - Nur `airlift_node::...` importieren.
   - Keine privaten Felder/Methoden.
   - Vorhandene Mocks verwenden (`airlift_node::testing::mocks`).
3. **Unit-Test in `src/` nur wenn zwingend nötig.**
   - Keine neuen Getter/Helper nur für Tests.
4. **`cargo test` muss grün sein.**
