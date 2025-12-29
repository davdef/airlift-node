use airlift_node::codecs::pcm::{PcmCodec, PcmPassthroughDecoder};
use airlift_node::codecs::{AudioCodec, PCM_I16_SAMPLES};
use airlift_node::decoders::AudioDecoder;

#[test]
fn pcm_roundtrip() {
    let mut codec = PcmCodec::new();
    let mut decoder = PcmPassthroughDecoder::new(0);
    let pcm: Vec<i16> = (0..PCM_I16_SAMPLES as i16).collect();

    let encoded = codec.encode(&pcm).expect("encode");
    let decoded = decoder
        .decode(&encoded[0].payload)
        .expect("decode")
        .expect("frame");

    assert_eq!(decoded.samples, pcm);
}

#[test]
#[ignore]
fn pcm_encode_decode_bench() {
    use std::time::Instant;

    const ITERATIONS: usize = 5_000;
    let pcm: Vec<i16> = (0..PCM_I16_SAMPLES as i16).collect();

    let mut codec = PcmCodec::new();
    let mut decoder = PcmPassthroughDecoder::new(0);

    let start = Instant::now();
    for _ in 0..ITERATIONS {
        let encoded = codec.encode(&pcm).expect("encode");
        let _ = decoder
            .decode(&encoded[0].payload)
            .expect("decode")
            .expect("frame");
    }
    let direct_elapsed = start.elapsed();

    let start = Instant::now();
    for _ in 0..ITERATIONS {
        let mut buffer = Vec::with_capacity(pcm.len() * 2);
        for sample in &pcm {
            buffer.extend_from_slice(&sample.to_le_bytes());
        }
        for chunk in buffer.chunks_exact(2) {
            let _ = i16::from_le_bytes([chunk[0], chunk[1]]);
        }
    }
    let reference_elapsed = start.elapsed();

    eprintln!(
        "direct_cast: {:?}, reference: {:?}, ratio: {:.2}x",
        direct_elapsed,
        reference_elapsed,
        reference_elapsed.as_secs_f64() / direct_elapsed.as_secs_f64()
    );
}
