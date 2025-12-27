use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub struct SignalAnalysis {
    pub rms_levels: Vec<f32>,        // RMS pro Kanal [-1.0..1.0]
    pub peak_levels: Vec<f32>,       // Peak pro Kanal [-1.0..1.0]
    pub clipping: bool,              // Clipping erkannt?
    pub dc_offset: Vec<f32>,         // DC-Offset pro Kanal
    pub frequency_hint: Option<f32>, // Dominante Frequenz (Hz)
    pub noise_floor: f32,            // Grundrauschen
    pub channel_correlation: f32,    // Korrelation zwischen Kanälen (bei Stereo)
}

pub struct SignalAnalyzer {
    sample_rate: u32,
    channels: usize,
    buffer: VecDeque<i16>,
    max_buffer_size: usize,
}

impl SignalAnalyzer {
    pub fn new(sample_rate: u32, channels: usize) -> Self {
        // 1 Sekunde Buffer für Frequenzanalyse
        let max_buffer_size = sample_rate as usize * channels;
        
        Self {
            sample_rate,
            channels,
            buffer: VecDeque::with_capacity(max_buffer_size),
            max_buffer_size,
        }
    }
    
    pub fn feed_samples(&mut self, samples: &[i16]) {
        for &sample in samples {
            self.buffer.push_back(sample);
            if self.buffer.len() > self.max_buffer_size {
                self.buffer.pop_front();
            }
        }
    }
    
    pub fn analyze(&self) -> SignalAnalysis {
        let samples: Vec<i16> = self.buffer.iter().copied().collect();
        
        if samples.is_empty() {
            return SignalAnalysis {
                rms_levels: vec![0.0; self.channels],
                peak_levels: vec![0.0; self.channels],
                clipping: false,
                dc_offset: vec![0.0; self.channels],
                frequency_hint: None,
                noise_floor: 0.0,
                channel_correlation: 0.0,
            };
        }
        
        // Separiere Kanäle
        let mut channel_samples: Vec<Vec<f32>> = (0..self.channels)
            .map(|_| Vec::new())
            .collect();
        
        for (i, &sample) in samples.iter().enumerate() {
            let channel = i % self.channels;
            channel_samples[channel].push(sample as f32 / 32768.0);
        }
        
        // Analysiere jeden Kanal
        let rms_levels: Vec<f32> = channel_samples.iter()
            .map(|ch| Self::calculate_rms(ch))
            .collect();
        
        let peak_levels: Vec<f32> = channel_samples.iter()
            .map(|ch| Self::calculate_peak(ch))
            .collect();
        
        let clipping = peak_levels.iter().any(|&p| p >= 0.99);
        
        let dc_offset: Vec<f32> = channel_samples.iter()
            .map(|ch| Self::calculate_dc_offset(ch))
            .collect();
        
        let frequency_hint = if self.buffer.len() >= 1024 {
            Self::estimate_frequency(&channel_samples[0], self.sample_rate)
        } else {
            None
        };
        
        let noise_floor = Self::estimate_noise_floor(&channel_samples[0]);
        
        let channel_correlation = if self.channels >= 2 {
            Self::calculate_correlation(&channel_samples[0], &channel_samples[1])
        } else {
            0.0
        };
        
        SignalAnalysis {
            rms_levels,
            peak_levels,
            clipping,
            dc_offset,
            frequency_hint,
            noise_floor,
            channel_correlation,
        }
    }
    
    fn calculate_rms(samples: &[f32]) -> f32 {
        if samples.is_empty() { return 0.0; }
        let sum_squares: f32 = samples.iter().map(|&s| s * s).sum();
        (sum_squares / samples.len() as f32).sqrt()
    }
    
    fn calculate_peak(samples: &[f32]) -> f32 {
        samples.iter()
            .map(|&s| s.abs())
            .fold(0.0, |a, b| a.max(b))
    }
    
    fn calculate_dc_offset(samples: &[f32]) -> f32 {
        if samples.is_empty() { return 0.0; }
        samples.iter().sum::<f32>() / samples.len() as f32
    }
    
    fn estimate_frequency(samples: &[f32], sample_rate: u32) -> Option<f32> {
        // Einfache Zero-Crossing Frequenzschätzung
        if samples.len() < 2 {
            return None;
        }
        
        let mut zero_crossings = 0;
        for i in 1..samples.len() {
            if samples[i-1] * samples[i] < 0.0 {
                zero_crossings += 1;
            }
        }
        
        if zero_crossings > 0 {
            let duration = samples.len() as f32 / sample_rate as f32;
            Some(zero_crossings as f32 / (2.0 * duration))
        } else {
            None
        }
    }
    
    fn estimate_noise_floor(samples: &[f32]) -> f32 {
        // RMS der letzten 10% der Samples als Rauschen
        if samples.len() < 10 { return 0.0; }
        let start = samples.len() * 9 / 10;
        let noise_samples = &samples[start..];
        Self::calculate_rms(noise_samples)
    }
    
    fn calculate_correlation(ch1: &[f32], ch2: &[f32]) -> f32 {
        let n = ch1.len().min(ch2.len());
        if n < 2 { return 0.0; }
        
        let mean1: f32 = ch1[..n].iter().sum::<f32>() / n as f32;
        let mean2: f32 = ch2[..n].iter().sum::<f32>() / n as f32;
        
        let mut numerator = 0.0;
        let mut denom1 = 0.0;
        let mut denom2 = 0.0;
        
        for i in 0..n {
            let diff1 = ch1[i] - mean1;
            let diff2 = ch2[i] - mean2;
            numerator += diff1 * diff2;
            denom1 += diff1 * diff1;
            denom2 += diff2 * diff2;
        }
        
        if denom1 > 0.0 && denom2 > 0.0 {
            numerator / (denom1.sqrt() * denom2.sqrt())
        } else {
            0.0
        }
    }
}
