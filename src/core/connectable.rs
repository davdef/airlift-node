use std::sync::Arc;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use crate::core::processor::Processor;
use crate::core::ringbuffer::AudioRingBuffer;
use crate::core::{Consumer, Producer};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PortType {
    Audio,
    Control,
    Midi,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Port {
    pub name: String,
    pub data_type: PortType,
    pub channels: u8,
    pub sample_rate: Option<u32>,
}

impl Port {
    pub fn audio(name: &str, channels: u8, sample_rate: Option<u32>) -> Self {
        Self {
            name: name.to_string(),
            data_type: PortType::Audio,
            channels,
            sample_rate,
        }
    }
}

pub trait Connectable: Send + Sync {
    fn input_ports(&self) -> Vec<Port> {
        Vec::new()
    }

    fn output_ports(&self) -> Vec<Port> {
        Vec::new()
    }

    fn connect_input(&mut self, port: &str, _buffer: Arc<AudioRingBuffer>) -> Result<()> {
        bail!("input port '{}' is not supported", port);
    }

    fn disconnect_input(&mut self, _port: &str) -> Result<()> {
        Ok(())
    }

    fn connect_output(&mut self, port: &str, _buffer: Arc<AudioRingBuffer>) -> Result<()> {
        bail!("output port '{}' is not supported", port);
    }

    fn disconnect_output(&mut self, _port: &str) -> Result<()> {
        Ok(())
    }
}

impl<T: Producer + ?Sized> Connectable for T {
    fn output_ports(&self) -> Vec<Port> {
        vec![Port::audio("output", 2, None)]
    }

    fn connect_output(&mut self, port: &str, buffer: Arc<AudioRingBuffer>) -> Result<()> {
        if port != "output" {
            bail!("unknown output port '{}'", port);
        }
        self.attach_ring_buffer(buffer);
        Ok(())
    }
}

impl<T: Consumer + ?Sized> Connectable for T {
    fn input_ports(&self) -> Vec<Port> {
        vec![Port::audio("input", 2, None)]
    }

    fn connect_input(&mut self, port: &str, buffer: Arc<AudioRingBuffer>) -> Result<()> {
        if port != "input" {
            bail!("unknown input port '{}'", port);
        }
        self.attach_input_buffer(buffer);
        Ok(())
    }
}

impl<T: Processor + ?Sized> Connectable for T {
    fn input_ports(&self) -> Vec<Port> {
        vec![Port::audio("input", 2, None)]
    }

    fn output_ports(&self) -> Vec<Port> {
        vec![Port::audio("output", 2, None)]
    }

    fn connect_input(&mut self, port: &str, _buffer: Arc<AudioRingBuffer>) -> Result<()> {
        if port != "input" {
            bail!("unknown input port '{}'", port);
        }
        Ok(())
    }

    fn connect_output(&mut self, port: &str, _buffer: Arc<AudioRingBuffer>) -> Result<()> {
        if port != "output" {
            bail!("unknown output port '{}'", port);
        }
        Ok(())
    }
}
