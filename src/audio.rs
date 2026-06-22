use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait};
use kira::{
    backend::cpal::CpalBackendSettings,
    sound::static_sound::{StaticSoundData, StaticSoundHandle},
    AudioManager, AudioManagerSettings, DefaultBackend, Tween,
};
use std::path::Path;
use tracing::{info, warn};

pub struct AudioPlayer {
    manager: AudioManager<DefaultBackend>,
    current: Option<StaticSoundHandle>,
}

#[cfg(target_os = "linux")]
fn derive_pipewire_node(device_addr: &str) -> String {
    let underscored = device_addr.replace(':', "_");
    format!("bluez_output.{}.1", underscored)
}

#[cfg(target_os = "linux")]
fn configure_audio_sink(audio_device_name: Option<&str>, device_addr: Option<&str>) {
    if let Some(addr) = device_addr {
        let node_name = derive_pipewire_node(addr);
        info!(%node_name, "routing audio to PipeWire sink");
        std::env::set_var("PIPEWIRE_NODE", &node_name);
        return;
    }
    if let Some(name) = audio_device_name {
        warn!(
            %name,
            "explicit audio.device is set but PipeWire routing by name is not implemented on Linux; using default output"
        );
    }
}

#[cfg(target_os = "macos")]
fn resolve_cpal_output_device(name: Option<&str>) -> Option<cpal::Device> {
    let Some(name) = name else {
        return None;
    };
    let host = cpal::default_host();
    let mut devices = match host.output_devices() {
        Ok(devices) => devices,
        Err(err) => {
            warn!(error = ?err, "failed to enumerate cpal output devices");
            return None;
        }
    };
    let match_name = name.to_string();
    let found = devices.find(|device| {
        device
            .description()
            .map(|description| description.name() == match_name)
            .unwrap_or(false)
    });
    if found.is_none() {
        warn!(
            requested = %name,
            "cpal output device not found; falling back to default"
        );
    }
    found
}

#[cfg(target_os = "macos")]
fn configure_audio_sink(_audio_device_name: Option<&str>, device_addr: Option<&str>) {
    if let Some(addr) = device_addr {
        info!(%addr, "bluetooth.device address is not used for audio routing on macOS; use audio.device instead");
    }
}

impl AudioPlayer {
    pub fn new(audio_device_name: Option<&str>, device_addr: Option<&str>) -> Result<Self> {
        configure_audio_sink(audio_device_name, device_addr);

        #[cfg(target_os = "macos")]
        let backend_settings = {
            let device = resolve_cpal_output_device(audio_device_name);
            CpalBackendSettings {
                device,
                config: None,
            }
        };
        #[cfg(not(target_os = "macos"))]
        let backend_settings = CpalBackendSettings::default();

        let manager = AudioManager::<DefaultBackend>::new(AudioManagerSettings {
            backend_settings,
            ..AudioManagerSettings::default()
        })
        .context("create audio output manager")?;
        Ok(Self {
            manager,
            current: None,
        })
    }

    pub fn play_interrupting(&mut self, path: &Path) -> Result<()> {
        self.stop_current();
        let sound = StaticSoundData::from_file(path)
            .with_context(|| format!("load audio prompt {}", path.display()))?;
        let handle = self
            .manager
            .play(sound)
            .with_context(|| format!("play audio prompt {}", path.display()))?;
        self.current = Some(handle);
        Ok(())
    }

    pub fn stop_current(&mut self) {
        if let Some(mut handle) = self.current.take() {
            handle.stop(Tween::default());
        }
    }
}
