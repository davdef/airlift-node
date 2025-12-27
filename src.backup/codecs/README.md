# Codec architecture

The codec layer turns fixed-size PCM input into encoded frames without any
transport responsibilities. Codecs are modeled as **instance-based modules**
with their own IDs, configuration, state, and metrics.

## Responsibilities

- **Codecs** accept **100 ms PCM @ 48 kHz, stereo** (`i16` samples).
- **Outputs** reference a codec **instance ID** and only ship encoded frames.
- **Transports** (Icecast, SRT, UDP, HTTP, …) are isolated from codec logic.

## Codec instance principle

Each codec instance is a module:

- `id`
- `module_type = "codec"`
- `codec_type` (`opus_ogg`, `opus_webrtc`, `mp3`, `vorbis`, `pcm`, …)
- configuration (sample rate, channels, frame size, bitrate, container/mode, …)
- runtime state + metrics + last error

Multiple instances of the same codec type can run in parallel with different
parameters. Outputs must point at a specific instance ID.

## Core types

- `AudioCodec`: unified codec interface.
- `CodecInfo`: metadata (codec kind, samplerate, channels, container).
- `EncodedFrame`: payload + metadata returned by codecs.
- `CodecRegistry`: owns codec instance definitions and runtime state.

## Supported codec kinds

- PCM (raw)
- Opus (Ogg container)
- Opus (WebRTC/RTP payloads)
- MP3
- Vorbis
- Prepared only: AAC-LC, FLAC

## Conceptual configuration example

```toml
[[codecs]]
id = "codec_opus_ogg"
type = "opus_ogg"
sample_rate = 48000
channels = 2
frame_size_ms = 20
bitrate = 96000
container = "ogg"
mode = "stream"
application = "audio"

[[codecs]]
id = "codec_opus_webrtc"
type = "opus_webrtc"
sample_rate = 48000
channels = 2
frame_size_ms = 20
bitrate = 96000
container = "rtp"
mode = "webrtc"
application = "voip"

[[codecs]]
id = "codec_mp3"
type = "mp3"
sample_rate = 48000
channels = 2
frame_size_ms = 100
bitrate = 128
container = "mpeg"
```

Outputs refer to codec instances:

```toml
[icecast_out]
codec_id = "codec_opus_ogg"

[mp3_out]
codec_id = "codec_mp3"
```
