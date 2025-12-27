use anyhow::{Result, anyhow};
use std::io::Read;

pub struct OggStreamParser<R: Read> {
    reader: R,
    buffer: Vec<u8>,
    current_page_pos: usize,
    segments_in_current_page: Vec<u8>,
    current_segment_idx: usize,
    packet_accumulator: Vec<u8>,
}

impl<R: Read> OggStreamParser<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            buffer: Vec::with_capacity(65536),
            current_page_pos: 0,
            segments_in_current_page: Vec::new(),
            current_segment_idx: 0,
            packet_accumulator: Vec::new(),
        }
    }

    pub fn next_packet(&mut self) -> Result<Option<Vec<u8>>> {
        loop {
            if !self.segments_in_current_page.is_empty()
                && self.current_segment_idx < self.segments_in_current_page.len()
            {
                if let Some(packet) = self.extract_next_packet() {
                    return Ok(Some(packet));
                }
            }

            match self.find_and_parse_next_page() {
                Ok(true) => continue,
                Ok(false) => return Ok(None),
                Err(e) => return Err(e),
            }
        }
    }

    fn find_and_parse_next_page(&mut self) -> Result<bool> {
        loop {
            for i in self.current_page_pos..self.buffer.len().saturating_sub(27) {
                if self.buffer[i..].starts_with(&[0x4f, 0x67, 0x67, 0x53]) {
                    if let Some(page_info) = self.parse_page_at(i) {
                        self.current_page_pos = i + page_info.total_size;
                        self.segments_in_current_page = page_info.segment_table;
                        self.current_segment_idx = 0;
                        return Ok(true);
                    }
                }
            }

            let mut temp_buf = [0u8; 8192];
            match self.reader.read(&mut temp_buf) {
                Ok(0) => return Ok(false),
                Ok(n) => {
                    self.buffer.extend_from_slice(&temp_buf[..n]);
                    if self.buffer.len() > 131072 {
                        if self.current_page_pos > 65536 {
                            let to_remove = self.current_page_pos - 32768;
                            self.buffer.drain(..to_remove);
                            self.current_page_pos -= to_remove;
                        }
                    }
                }
                Err(e) => return Err(anyhow!("Read error: {}", e)),
            }
        }
    }

    fn parse_page_at(&self, start: usize) -> Option<PageInfo> {
        if start + 27 > self.buffer.len() {
            return None;
        }

        let capture_pattern = &self.buffer[start..start + 4];
        if capture_pattern != b"OggS" {
            return None;
        }

        let version = self.buffer[start + 4];
        if version != 0 {
            return None;
        }

        let header_type = self.buffer[start + 5];
        let _granule_position = u64::from_le_bytes([
            self.buffer[start + 6],
            self.buffer[start + 7],
            self.buffer[start + 8],
            self.buffer[start + 9],
            self.buffer[start + 10],
            self.buffer[start + 11],
            self.buffer[start + 12],
            self.buffer[start + 13],
        ]);
        let _bitstream_serial = u32::from_le_bytes([
            self.buffer[start + 14],
            self.buffer[start + 15],
            self.buffer[start + 16],
            self.buffer[start + 17],
        ]);
        let _page_sequence = u32::from_le_bytes([
            self.buffer[start + 18],
            self.buffer[start + 19],
            self.buffer[start + 20],
            self.buffer[start + 21],
        ]);
        let _checksum = u32::from_le_bytes([
            self.buffer[start + 22],
            self.buffer[start + 23],
            self.buffer[start + 24],
            self.buffer[start + 25],
        ]);
        let page_segments = self.buffer[start + 26] as usize;

        if start + 27 + page_segments > self.buffer.len() {
            return None;
        }

        let segment_table_start = start + 27;
        let mut segment_table = Vec::with_capacity(page_segments);
        let mut total_segment_size = 0;

        for i in 0..page_segments {
            let seg_size = self.buffer[segment_table_start + i];
            segment_table.push(seg_size);
            total_segment_size += seg_size as usize;
        }

        let page_end = start + 27 + page_segments + total_segment_size;
        if page_end > self.buffer.len() {
            return None;
        }

        Some(PageInfo {
            total_size: page_end - start,
            segment_table,
            header_type,
        })
    }

    fn extract_next_packet(&mut self) -> Option<Vec<u8>> {
        while self.current_segment_idx < self.segments_in_current_page.len() {
            let segment_size = self.segments_in_current_page[self.current_segment_idx] as usize;

            let page_start = self.current_page_pos
                - self
                    .segments_in_current_page
                    .iter()
                    .map(|&s| s as usize)
                    .sum::<usize>()
                - 27
                - self.segments_in_current_page.len();

            let segment_start = page_start
                + 27
                + self.segments_in_current_page.len()
                + self.segments_in_current_page[..self.current_segment_idx]
                    .iter()
                    .map(|&s| s as usize)
                    .sum::<usize>();

            if segment_start + segment_size <= self.buffer.len() {
                let segment_data = &self.buffer[segment_start..segment_start + segment_size];
                self.packet_accumulator.extend_from_slice(segment_data);
            }

            self.current_segment_idx += 1;

            let is_last_segment = self.current_segment_idx == self.segments_in_current_page.len();
            let segment_completes_packet = segment_size < 255;

            if is_last_segment || segment_completes_packet {
                if !self.packet_accumulator.is_empty() {
                    let packet = self.packet_accumulator.clone();
                    self.packet_accumulator.clear();
                    return Some(packet);
                }
            }

            if is_last_segment {
                self.segments_in_current_page.clear();
                self.current_segment_idx = 0;
            }
        }

        None
    }
}

struct PageInfo {
    total_size: usize,
    segment_table: Vec<u8>,
    header_type: u8,
}
