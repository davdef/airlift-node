use crate::ring::PcmFrame;

pub trait AudioDecoder: Send {
    fn decode(&mut self, packet: &[u8]) -> anyhow::Result<Option<PcmFrame>>;
}
