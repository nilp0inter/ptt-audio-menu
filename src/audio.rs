use anyhow::{Context, Result};
use kira::{
    sound::static_sound::{StaticSoundData, StaticSoundHandle},
    AudioManager, AudioManagerSettings, DefaultBackend, Tween,
};
use std::path::Path;
use tracing::info;

pub struct AudioPlayer {
    manager: AudioManager<DefaultBackend>,
    current: Option<StaticSoundHandle>,
}

fn derive_pipewire_node(device_addr: &str) -> String {
    let underscored = device_addr.replace(':', "_");
    format!("bluez_output.{}.1", underscored)
}

impl AudioPlayer {
    pub fn new(_audio_device_name: Option<&str>, device_addr: Option<&str>) -> Result<Self> {
        if let Some(addr) = device_addr {
            let node_name = derive_pipewire_node(addr);
            info!(%node_name, "routing audio to PipeWire sink");
            std::env::set_var("PIPEWIRE_NODE", &node_name);
        }

        let manager = AudioManager::<DefaultBackend>::new(AudioManagerSettings::default())
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
