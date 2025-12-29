use crate::core::device_scanner::*;
use anyhow::{Context, Result};
use std::ffi::CStr;

pub struct AlsaDeviceScanner;

impl DeviceScanner for AlsaDeviceScanner {
    fn scan_devices(&self) -> Result<Vec<AudioDeviceInfo>> {
        use alsa::{device_name::HintIter, Direction};

        let mut devices = Vec::new();

        // Scan PCM devices using ALSA hints
        let hints = HintIter::new(None, CStr::from_bytes_with_nul(b"pcm\0").unwrap())
            .context("Failed to get ALSA device hints")?;

        for hint in hints {
            let Some(name) = hint.name else { continue };
            let desc = hint.desc.unwrap_or_default();
            let Some(direction) = hint.direction else {
                continue;
            };

            // Determine device type
            let device_type = match direction {
                Direction::Capture => DeviceType::Input,
                Direction::Playback => DeviceType::Output,
            };

            // Try to get hardware parameters
            let supported_formats = match self.get_supported_formats(&name) {
                Ok(fmts) => fmts,
                Err(e) => {
                    log::debug!("Failed to get formats for {}: {}", name, e);
                    Vec::new()
                }
            };

            let max_channels = self.get_max_channels(&name).unwrap_or(2);

            devices.push(AudioDeviceInfo {
                id: name.clone(),
                name: desc.clone(),
                description: format!("ALSA: {}", desc),
                device_type,
                supported_formats,
                default_format: None,
                max_channels,
                supported_rates: vec![44100, 48000, 96000, 192000],
                is_default: name == "default" || name.contains("default"),
            });
        }

        Ok(devices)
    }

    fn test_device(&self, device_id: &str, _test_duration_ms: u64) -> Result<DeviceTestResult> {
        use alsa::{pcm::PCM, Direction};

        log::info!("Testing ALSA device: {}", device_id);

        // Try to open the device
        let pcm = PCM::new(device_id, Direction::Capture, false)
            .with_context(|| format!("Failed to open device: {}", device_id))?;

        let hw_params = pcm.hw_params_current()?;

        // Collect format information
        let channels = hw_params.get_channels_max()? as u8;
        let min_rate = hw_params.get_rate_min()?;
        let max_rate = hw_params.get_rate_max()?;

        let mut warnings = Vec::new();

        // Basic test: can we configure it?
        let mut test_passed = false;
        let mut detected_format = None;

        // Try common formats
        for rate in &[44100, 48000, 96000] {
            if *rate >= min_rate && *rate <= max_rate {
                test_passed = true;
                detected_format = Some(AudioFormat {
                    sample_rate: *rate,
                    channels: channels.min(2),
                    sample_type: SampleType::SignedInteger,
                    bit_depth: 16,
                });
                break;
            }
        }

        if !test_passed {
            warnings.push(format!(
                "No supported sample rate found. Min: {}, Max: {}",
                min_rate, max_rate
            ));
        }

        Ok(DeviceTestResult {
            device_id: device_id.to_string(),
            test_passed,
            detected_format,
            channel_peaks: vec![0.0; channels as usize],
            channel_rms: vec![0.0; channels as usize],
            noise_level: 0.0,
            clipping_detected: false,
            estimated_latency_ms: Some(10.0),
            warnings,
            errors: Vec::new(),
        })
    }
}

impl AlsaDeviceScanner {
    fn get_supported_formats(&self, device_id: &str) -> Result<Vec<AudioFormat>> {
        use alsa::{pcm::PCM, Direction};

        let mut formats = Vec::new();

        // Try to open device to query formats
        let pcm = PCM::new(device_id, Direction::Capture, false)?;
        let hw_params = pcm.hw_params_current()?;

        // Common sample rates
        let sample_rates = [44100, 48000, 96000, 192000];

        for &rate in &sample_rates {
            if hw_params.test_rate(rate).is_ok() {
                formats.push(AudioFormat {
                    sample_rate: rate,
                    channels: 2,
                    sample_type: SampleType::SignedInteger,
                    bit_depth: 16,
                });
            }
        }

        Ok(formats)
    }

    fn get_max_channels(&self, device_id: &str) -> Result<u8> {
        use alsa::{pcm::PCM, Direction};

        let pcm = PCM::new(device_id, Direction::Capture, false)?;
        let hw_params = pcm.hw_params_current()?;
        Ok(hw_params.get_channels_max()? as u8)
    }
}
