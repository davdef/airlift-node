# API overview

This document summarizes the API surface exposed by the `src/api` module, the
expected inputs/outputs, and current behavior. It is intentionally derived from
implementation code to help align frontend expectations with backend behavior.

## Transport

- HTTP endpoints are served by the API server (`src/api/mod.rs`).
- WebSocket endpoints are also served by the API server and are implemented in
  `src/api/ws.rs` and `src/api/recorder.rs`.

## Health & monitoring

### `GET /health`

Health probe. Uses `monitoring::handle_health_request`.

- **200**: `ok` when the node is running.
- **503**: `not_running` when the node is stopped.

### `GET /metrics`

Prometheus text output for node and buffers (producer and ring buffer metrics).
Content-Type: `text/plain; version=0.0.4`.

## Config

### `POST /api/config`

Applies a partial configuration patch.

- **Request body**: JSON matching `ConfigPatch` (`crate::config::ConfigPatch`).
- **Success**: `200` with JSON `{ "status": "ok", "config": <full config> }`.
- **Errors**:
  - `400` invalid JSON / invalid patch.
  - `500` config lock failure.

## Status

### `GET /api/status`

Returns runtime status derived from `AirliftNode::status()`.

- **Response body**: JSON matching `StatusResponse` (`src/api/status.rs`).
  Includes `running`, `uptime_seconds`, `producers`, `flows`, `ringbuffer`,
  and `timestamp_ms`.

## Peak history

### `GET /api/peaks`

Returns the current buffer range of peak history.

- **Query params** (optional):
  - `flow`: filter to a specific flow name.

- **Response body**:
  ```json
  {
    "ok": true,
    "start": 1712345678901,
    "end": 1712345689012
  }
  ```
  - `ok` is `false` and `start`/`end` are `null` if no peaks are recorded yet.

### `GET /api/history?from=<ms>&to=<ms>`

Returns historical peak points for the given inclusive range.

- **Query params** (optional):
  - `flow`: filter to a specific flow name.

- **Query params**: `from`, `to` as millisecond timestamps; `from < to`.
- **Response body**: array of peak points
  ```json
  [
    { "ts": 1712345678901, "peak_l": 0.12, "peak_r": 0.10, "silence": false }
  ]
  ```
- **Errors**: `400` on invalid query.

Peak history is populated from `AudioPeak` events emitted by flows.

## Control

### `POST /api/control`

Execute a control action.

- **Request body**:
  ```json
  {
    "action": "start" | "stop" | "restart" |
               "reload" | "config.reload" | "node.reload" |
               "config.import" |
               "flow.start" | "flow.stop" | "flow.restart",
    "target": "flow-name",
    "parameters": { "toml": "..." } | "..." 
  }
  ```
- **Response**: JSON `{ "ok": true|false, "message": "..." }`.
- **Notes**:
  - `config.import` requires TOML in `parameters` (string or object with
    `toml`/`config_toml`).
  - `flow.*` actions require `target`.

## Catalog

### `GET /api/catalog`

Returns the available producer/processor/consumer types and buffer names.

- **Response body**:
  ```json
  {
    "inputs": [ { "name": "...", "type": "producer" } ],
    "buffers": [ { "name": "...", "type": "buffer" } ],
    "processing": [ { "name": "...", "type": "processor" } ],
    "services": [ { "name": "flow", "type": "flow" } ],
    "outputs": [ { "name": "...", "type": "consumer" } ]
  }
  ```

## Recorder

### `POST /api/recorder/start`

Creates a recorder producer backed by a WebSocket input.

- **Response body**:
  ```json
  { "producer_id": "recorder-1" }
  ```

### `POST /api/recorder/stop/<producer_id>`

Stops and removes a recorder session (producer + flow). Returns `200` on
success, `404` if the session does not exist.

## WebSockets

### `GET /ws`

WebSocket that streams `AudioPeak` events as JSON payloads. Example event:

```json
{
  "timestamp": 1712345678901,
  "peaks": [0.12, 0.10],
  "silence": false,
  "flow": "recorder-1"
}
```

### `GET /ws/recorder/<producer_id>`

WebSocket for sending PCM audio frames to the recorder producer.

- **Client message format**: binary frames containing interleaved `f32` samples
  encoded as little-endian bytes. The server converts to `i16` internally.
- **Sample rate**: 48kHz; **channels**: 2.

### `GET /ws/echo/<session_id>`

WebSocket for echo playback for a recorder session.

- **Server message format**: binary frames containing interleaved `i16` samples
  encoded as little-endian bytes.
- **Sample rate**: 48kHz; **channels**: 2.

## Known inconsistencies & follow-ups

These are implementation details that may be surprising to clients or worth
addressing:

1. **Status response omits module metadata**
   - `StatusResponse` includes `modules`, `inactive_modules`, and
     `configuration_issues`, but they are currently always empty (`Vec::new()`).
   - The status UI expects these to render module diagnostics.

2. **Peak history is keyed by flow name**
   - `AudioPeak` events include `flow` in their payload. Frontends that need
     per-producer or per-flow peak views should filter by this field.

If you want, I can file follow-up patches to populate module diagnostics in
`StatusResponse`.
