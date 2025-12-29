# Performance & Profiling

Dieses Dokument beschreibt die aktuellen Performance-Maßnahmen rund um Mixer und
Ringbuffer sowie reproduzierbare Profiling-/Benchmark-Workflows.

## Mixer: Batch-Verarbeitung

Der Mixer verarbeitet pro `process`-Aufruf mehrere Frames (`MAX_BATCH_FRAMES = 8`).
Dadurch werden Funktionsaufrufe, Lock-Overhead und Logging pro Frame reduziert,
insbesondere wenn mehrere Frames bereits in den Input-Buffer liegen. Die
Batch-Verarbeitung ist vollständig kompatibel zum bisherigen Verhalten: Es wird
nur gemischt, wenn mindestens ein Input-Frame verfügbar ist.

## Profiling (perf / flamegraph)

### Mixer-Pfad

```bash
cargo bench --bench mixer_bench
```

Sampling mit `perf` (Linux):

```bash
perf record --call-graph dwarf -- cargo bench --bench mixer_bench
perf report
```

Flamegraph (mit `inferno` oder `flamegraph`):

```bash
perf record --call-graph dwarf -- cargo bench --bench mixer_bench
perf script | inferno-flamegraph > mixer-flamegraph.svg
```

### Ringbuffer-Pfad

```bash
cargo bench --bench ringbuffer_bench
```

Profiling analog:

```bash
perf record --call-graph dwarf -- cargo bench --bench ringbuffer_bench
perf report
```

## Benchmarks & Hinweise

- Die Benchmarks befinden sich in `benches/mixer_bench.rs` und
  `benches/ringbuffer_bench.rs`.
- In Umgebungen ohne ALSA-Entwicklungspakete schlägt der Build fehl
  (`alsa-sys` benötigt `alsa.pc`). Installiere die System-Abhängigkeit oder
  setze `PKG_CONFIG_PATH`, um die Benchmarks auszuführen.
- Nutze die Benchmarks, um die Auswirkungen der Batch-Verarbeitung zu messen
  (z. B. Iterationen/s vor/nach Änderungen).
