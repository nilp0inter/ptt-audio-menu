use anyhow::{Context, Result};
use kira::{
    sound::static_sound::{StaticSoundData, StaticSoundHandle},
    AudioManager, AudioManagerSettings, DefaultBackend, Tween,
};
use std::path::Path;

pub struct AudioPlayer {
    manager: AudioManager<DefaultBackend>,
    current: Option<StaticSoundHandle>,
}

impl AudioPlayer {
    pub fn new() -> Result<Self> {
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
