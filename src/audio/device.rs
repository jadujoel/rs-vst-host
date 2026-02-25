//! Audio device enumeration and stream management using `cpal`.
//!
//! Provides audio output device selection and stream configuration for
//! real-time audio processing.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Host, SampleRate, Stream, StreamConfig};
use tracing::{debug, info};

/// Audio device configuration.
#[derive(Debug, Clone)]
pub struct AudioConfig {
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Number of output channels.
    pub channels: u16,
    /// Preferred buffer size in frames (0 = use default).
    pub buffer_size: u32,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            sample_rate: 44100,
            channels: 2,
            buffer_size: 0,
        }
    }
}

/// Information about an audio output device.
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// Device name.
    pub name: String,
    /// Whether this is the default output device.
    pub is_default: bool,
}

/// Audio device manager wrapping cpal.
pub struct AudioDevice {
    host: Host,
}

impl AudioDevice {
    /// Create a new audio device manager using the default host.
    pub fn new() -> Self {
        Self {
            host: cpal::default_host(),
        }
    }

    /// List available audio output devices.
    pub fn list_output_devices(&self) -> Vec<DeviceInfo> {
        let default_name = self
            .host
            .default_output_device()
            .and_then(|d| d.name().ok());

        let devices = match self.host.output_devices() {
            Ok(devs) => devs,
            Err(e) => {
                tracing::warn!(error = %e, "Failed to enumerate output devices");
                return Vec::new();
            }
        };

        devices
            .filter_map(|d| {
                let name = d.name().ok()?;
                let is_default = default_name.as_deref() == Some(&name);
                Some(DeviceInfo { name, is_default })
            })
            .collect()
    }

    /// Get the default output device.
    pub fn default_output_device(&self) -> Option<Device> {
        self.host.default_output_device()
    }

    /// Get an output device by name, or the default if name is None.
    pub fn get_output_device(&self, name: Option<&str>) -> Option<Device> {
        match name {
            Some(n) => {
                let devices = self.host.output_devices().ok()?;
                devices.into_iter().find(|d| d.name().ok().as_deref() == Some(n))
            }
            None => self.default_output_device(),
        }
    }

    /// Get the default output configuration for a device.
    pub fn default_config(device: &Device) -> Result<AudioConfig, String> {
        let config = device
            .default_output_config()
            .map_err(|e| format!("Failed to get default output config: {}", e))?;

        Ok(AudioConfig {
            sample_rate: config.sample_rate().0,
            channels: config.channels(),
            buffer_size: 0,
        })
    }

    /// Build an output stream with the given configuration and callbacks.
    ///
    /// The `data_callback` receives interleaved f32 samples to fill.
    /// The `error_callback` is called on stream errors.
    pub fn build_output_stream<D, E>(
        device: &Device,
        config: &AudioConfig,
        data_callback: D,
        error_callback: E,
    ) -> Result<Stream, String>
    where
        D: FnMut(&mut [f32], &cpal::OutputCallbackInfo) + Send + 'static,
        E: FnMut(cpal::StreamError) + Send + 'static,
    {
        let stream_config = StreamConfig {
            channels: config.channels,
            sample_rate: SampleRate(config.sample_rate),
            buffer_size: if config.buffer_size > 0 {
                cpal::BufferSize::Fixed(config.buffer_size)
            } else {
                cpal::BufferSize::Default
            },
        };

        info!(
            sample_rate = config.sample_rate,
            channels = config.channels,
            "Building output stream"
        );

        let stream = device
            .build_output_stream(&stream_config, data_callback, error_callback, None)
            .map_err(|e| format!("Failed to build output stream: {}", e))?;

        Ok(stream)
    }

    /// Start playing on a stream.
    pub fn play(stream: &Stream) -> Result<(), String> {
        stream
            .play()
            .map_err(|e| format!("Failed to start stream: {}", e))?;
        debug!("Audio stream started");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_config_default() {
        let config = AudioConfig::default();
        assert_eq!(config.sample_rate, 44100);
        assert_eq!(config.channels, 2);
        assert_eq!(config.buffer_size, 0);
    }

    #[test]
    fn test_audio_device_new() {
        // Just verify we can create an AudioDevice without panicking.
        // Actual device availability depends on the system.
        let _device = AudioDevice::new();
    }

    #[test]
    fn test_list_output_devices() {
        let device = AudioDevice::new();
        // On CI/headless systems this may return empty, which is fine.
        let devices = device.list_output_devices();
        // Just verify it doesn't panic
        for d in &devices {
            assert!(!d.name.is_empty());
        }
    }
}
