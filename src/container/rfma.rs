use anyhow::{Result, anyhow};
use byteorder::{BigEndian, ReadBytesExt};
use std::io::{Cursor, Read};

const MAGIC: &[u8; 4] = b"RFMA";

pub struct RfmaPacket {
    pub utc_ns: u64,
    pub payload: Vec<u8>,
}

pub fn parse_packet(buf: &[u8]) -> Result<RfmaPacket> {
    let mut c = Cursor::new(buf);

    let mut magic = [0u8; 4];
    c.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(anyhow!("invalid RFMA magic"));
    }

    let _seq = c.read_u64::<BigEndian>()?;
    let utc_ns = c.read_u64::<BigEndian>()?;
    let pcm_len = c.read_u32::<BigEndian>()? as usize;

    if pcm_len % 2 != 0 {
        return Err(anyhow!("invalid pcm_len {}", pcm_len));
    }

    let mut payload = vec![0u8; pcm_len];
    c.read_exact(&mut payload)?;

    Ok(RfmaPacket { utc_ns, payload })
}
