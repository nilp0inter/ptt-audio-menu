#![allow(dead_code)]

use crate::config::{ActionConfig, Config, VoiceConfig};
use anyhow::{bail, Context, Result};
use directories::BaseDirs;
use piper_rs::Piper;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    env, fs,
    io::Write,
    path::{Path, PathBuf},
};

const APP_CACHE_DIR: &str = "ptt-audio-menu";
const TTS_CACHE_DIR: &str = "tts";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TtsOutputFormat {
    WavPcm16,
}

impl TtsOutputFormat {
    fn as_str(self) -> &'static str {
        match self {
            Self::WavPcm16 => "wav-pcm16",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PiperSettings {
    pub speaker: Option<String>,
    pub length_scale: String,
    pub noise_scale: String,
    pub noise_w: String,
}

impl Default for PiperSettings {
    fn default() -> Self {
        Self {
            speaker: None,
            length_scale: "1.0".to_string(),
            noise_scale: "0.667".to_string(),
            noise_w: "0.8".to_string(),
        }
    }
}

#[derive(Debug)]
pub struct TtsCache {
    dir: PathBuf,
}

impl TtsCache {
    pub fn new(config: &Config) -> Result<Self> {
        let dir = resolve_tts_cache_dir(config.cache.tts_dir.as_deref())?;
        fs::create_dir_all(&dir)
            .with_context(|| format!("create TTS cache directory {}", dir.display()))?;
        Ok(Self { dir })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn wav_path(&self, input: &TtsCacheInput<'_>) -> PathBuf {
        self.dir.join(format!("{}.wav", input.cache_key()))
    }

    pub fn read_wav(&self, input: &TtsCacheInput<'_>) -> Result<Option<Vec<u8>>> {
        let path = self.wav_path(input);
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err).with_context(|| format!("read cached TTS WAV {}", path.display())),
        }
    }

    pub fn write_wav(&self, input: &TtsCacheInput<'_>, bytes: &[u8]) -> Result<PathBuf> {
        fs::create_dir_all(&self.dir)
            .with_context(|| format!("create TTS cache directory {}", self.dir.display()))?;
        let path = self.wav_path(input);
        fs::write(&path, bytes)
            .with_context(|| format!("write cached TTS WAV {}", path.display()))?;
        Ok(path)
    }
}

#[derive(Debug)]
pub struct CachedPrompt {
    text: String,
    path: PathBuf,
}

impl CachedPrompt {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub struct TtsRenderer {
    piper: Piper,
}

impl TtsRenderer {
    pub fn new(voice: &VoiceConfig) -> Result<Self> {
        let piper = Piper::new(&voice.model_path, &voice.config_path)
            .map_err(|err| anyhow::anyhow!(err))
            .with_context(|| {
                format!(
                    "load Piper voice model={} config={}",
                    voice.model_path.display(),
                    voice.config_path.display()
                )
            })?;
        Ok(Self { piper })
    }

    pub fn render_wav(&mut self, input: &TtsCacheInput<'_>) -> Result<Vec<u8>> {
        let speaker_id = input
            .settings
            .speaker
            .as_deref()
            .map(str::parse::<i64>)
            .transpose()
            .context("parse Piper speaker id")?;
        let length_scale = parse_piper_float("length_scale", &input.settings.length_scale)?;
        let noise_scale = parse_piper_float("noise_scale", &input.settings.noise_scale)?;
        let noise_w = parse_piper_float("noise_w", &input.settings.noise_w)?;
        let (samples, sample_rate) = self
            .piper
            .create(
                input.text,
                false,
                speaker_id,
                Some(length_scale),
                Some(noise_scale),
                Some(noise_w),
            )
            .map_err(|err| anyhow::anyhow!(err))
            .with_context(|| format!("render TTS prompt {:?}", input.text))?;
        Ok(wav_pcm16_from_f32(&samples, sample_rate, 1))
    }
}

pub fn prerender_prompts(
    cache: &TtsCache,
    renderer: &mut TtsRenderer,
    voice: &VoiceConfig,
    prompts: &[String],
) -> Result<Vec<CachedPrompt>> {
    let mut cached = Vec::with_capacity(prompts.len());
    for prompt in prompts {
        let input = TtsCacheInput::new(prompt, voice);
        let path = match cache.read_wav(&input)? {
            Some(_) => cache.wav_path(&input),
            None => {
                let wav = renderer.render_wav(&input)?;
                cache.write_wav(&input, &wav)?
            }
        };
        cached.push(CachedPrompt {
            text: prompt.clone(),
            path,
        });
    }
    Ok(cached)
}

#[derive(Clone, Debug)]
pub struct TtsCacheInput<'a> {
    pub text: &'a str,
    pub voice: &'a VoiceConfig,
    pub settings: PiperSettings,
    pub output_format: TtsOutputFormat,
    pub app_version: &'a str,
}

impl<'a> TtsCacheInput<'a> {
    pub fn new(text: &'a str, voice: &'a VoiceConfig) -> Self {
        Self {
            text,
            voice,
            settings: PiperSettings::default(),
            output_format: TtsOutputFormat::WavPcm16,
            app_version: env!("CARGO_PKG_VERSION"),
        }
    }

    pub fn cache_key(&self) -> String {
        let mut hasher = Sha256::new();
        update_field(&mut hasher, "text", self.text);
        update_field(&mut hasher, "model_path", &path_key(&self.voice.model_path));
        update_field(
            &mut hasher,
            "config_path",
            &path_key(&self.voice.config_path),
        );
        update_field(
            &mut hasher,
            "speaker",
            self.settings.speaker.as_deref().unwrap_or(""),
        );
        update_field(&mut hasher, "length_scale", &self.settings.length_scale);
        update_field(&mut hasher, "noise_scale", &self.settings.noise_scale);
        update_field(&mut hasher, "noise_w", &self.settings.noise_w);
        update_field(&mut hasher, "output_format", self.output_format.as_str());
        update_field(&mut hasher, "app_version", self.app_version);
        format!("{:x}", hasher.finalize())
    }
}

pub fn collect_prompt_texts(config: &Config) -> Vec<String> {
    let mut prompts = PromptCollector::default();

    for tool in &config.tools {
        prompts.push(&tool.label);
        for tab in &tool.tabs {
            prompts.push(&tab.label);
            for item in &tab.items {
                prompts.push(&item.label);
            }
        }
    }

    for tab in &config.global_tabs {
        prompts.push(&tab.label);
        for item in &tab.items {
            prompts.push(&item.label);
        }
    }

    for action in &config.actions {
        match action {
            ActionConfig::Internal(action) => {
                if let Some(text) = action.text.as_deref() {
                    prompts.push(text);
                }
            }
            ActionConfig::Command(action) => {
                if let Some(text) = action.feedback.start.as_deref() {
                    prompts.push(text);
                }
                if let Some(text) = action.feedback.success.as_deref() {
                    prompts.push(text);
                }
                if let Some(text) = action.feedback.failure.as_deref() {
                    prompts.push(text);
                }
            }
        }
    }

    prompts.into_vec()
}

#[derive(Default)]
struct PromptCollector {
    seen: HashSet<String>,
    prompts: Vec<String>,
}

impl PromptCollector {
    fn push(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() || !self.seen.insert(text.to_string()) {
            return;
        }
        self.prompts.push(text.to_string());
    }

    fn into_vec(self) -> Vec<String> {
        self.prompts
    }
}

fn resolve_tts_cache_dir(configured_dir: Option<&Path>) -> Result<PathBuf> {
    if let Some(dir) = configured_dir {
        return Ok(dir.to_path_buf());
    }

    if let Some(xdg_cache_home) = env::var_os("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(xdg_cache_home)
            .join(APP_CACHE_DIR)
            .join(TTS_CACHE_DIR));
    }

    if let Some(base_dirs) = BaseDirs::new() {
        return Ok(base_dirs
            .cache_dir()
            .join(APP_CACHE_DIR)
            .join(TTS_CACHE_DIR));
    }

    bail!("could not resolve TTS cache path: set cache.tts_dir or HOME");
}

fn update_field(hasher: &mut Sha256, name: &str, value: &str) {
    hasher.update(name.as_bytes());
    hasher.update([0]);
    hasher.update(value.len().to_le_bytes());
    hasher.update(value.as_bytes());
    hasher.update([0xff]);
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn parse_piper_float(label: &str, value: &str) -> Result<f32> {
    value
        .parse()
        .with_context(|| format!("parse Piper setting {label}={value:?}"))
}

fn wav_pcm16_from_f32(samples: &[f32], sample_rate: u32, channels: u16) -> Vec<u8> {
    let samples_i16: Vec<i16> = samples
        .iter()
        .map(|sample| {
            (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX))
                .round()
                .clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16
        })
        .collect();
    wav_pcm16_from_i16(&samples_i16, sample_rate, channels)
}

fn wav_pcm16_from_i16(samples: &[i16], sample_rate: u32, channels: u16) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let byte_rate = sample_rate * u32::from(channels) * 2;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.write_all(b"RIFF").expect("write to Vec");
    out.write_all(&(36 + data_len).to_le_bytes())
        .expect("write to Vec");
    out.write_all(b"WAVEfmt ").expect("write to Vec");
    out.write_all(&16u32.to_le_bytes()).expect("write to Vec");
    out.write_all(&1u16.to_le_bytes()).expect("write to Vec");
    out.write_all(&channels.to_le_bytes())
        .expect("write to Vec");
    out.write_all(&sample_rate.to_le_bytes())
        .expect("write to Vec");
    out.write_all(&byte_rate.to_le_bytes())
        .expect("write to Vec");
    out.write_all(&(channels * 2).to_le_bytes())
        .expect("write to Vec");
    out.write_all(&16u16.to_le_bytes()).expect("write to Vec");
    out.write_all(b"data").expect("write to Vec");
    out.write_all(&data_len.to_le_bytes())
        .expect("write to Vec");
    for sample in samples {
        out.write_all(&sample.to_le_bytes()).expect("write to Vec");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        ActiveHooks, AudioConfig, CacheConfig, CommandActionConfig, FeedbackConfig, GlobalDefaults,
        InternalActionConfig, InternalCommand, ItemConfig, TabConfig, ToolConfig,
    };
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn make_config(
        cache_dir: Option<PathBuf>,
        model_path: PathBuf,
        voice_config_path: PathBuf,
    ) -> Config {
        Config {
            default_tool: "radio".to_string(),
            voice: VoiceConfig {
                model_path,
                config_path: voice_config_path,
            },
            cache: CacheConfig { tts_dir: cache_dir },
            audio: AudioConfig::default(),
            globals: GlobalDefaults::default(),
            tools: vec![],
            global_tabs: vec![],
            actions: vec![],
        }
    }

    #[test]
    fn same_full_input_reuses_cache_path() {
        let dir = TempDir::new().expect("create tempdir");
        let config = make_config(
            Some(dir.path().join("tts")),
            PathBuf::from("/voices/en.onnx"),
            PathBuf::from("/voices/en.json"),
        );
        let cache = TtsCache::new(&config).expect("create cache");
        let input = TtsCacheInput::new("Radio", &config.voice);

        assert_eq!(cache.wav_path(&input), cache.wav_path(&input));
        assert!(cache.wav_path(&input).starts_with(cache.dir()));
        assert_eq!(cache.wav_path(&input).extension().unwrap(), "wav");
    }

    #[test]
    fn changed_text_model_settings_and_format_change_cache_path() {
        let dir = TempDir::new().expect("create tempdir");
        let config = make_config(
            Some(dir.path().join("tts")),
            PathBuf::from("/voices/en.onnx"),
            PathBuf::from("/voices/en.json"),
        );
        let cache = TtsCache::new(&config).expect("create cache");
        let base = TtsCacheInput::new("Radio", &config.voice);

        let changed_text = TtsCacheInput::new("Music", &config.voice);
        assert_ne!(cache.wav_path(&base), cache.wav_path(&changed_text));

        let changed_model_config = make_config(
            Some(dir.path().join("tts")),
            PathBuf::from("/voices/es.onnx"),
            PathBuf::from("/voices/es.json"),
        );
        let changed_model = TtsCacheInput::new("Radio", &changed_model_config.voice);
        assert_ne!(cache.wav_path(&base), cache.wav_path(&changed_model));

        let mut changed_settings = TtsCacheInput::new("Radio", &config.voice);
        changed_settings.settings.length_scale = "0.9".to_string();
        assert_ne!(cache.wav_path(&base), cache.wav_path(&changed_settings));

        let mut changed_version = TtsCacheInput::new("Radio", &config.voice);
        changed_version.app_version = "next-version";
        assert_ne!(cache.wav_path(&base), cache.wav_path(&changed_version));
    }

    #[test]
    fn write_and_read_wav_bytes() {
        let dir = TempDir::new().expect("create tempdir");
        let config = make_config(
            Some(dir.path().join("tts")),
            PathBuf::from("/voices/en.onnx"),
            PathBuf::from("/voices/en.json"),
        );
        let cache = TtsCache::new(&config).expect("create cache");
        let input = TtsCacheInput::new("Radio", &config.voice);

        assert!(cache.read_wav(&input).expect("read miss").is_none());
        let path = cache.write_wav(&input, b"RIFFwav").expect("write wav");
        assert!(path.is_file());
        assert_eq!(
            cache.read_wav(&input).expect("read hit"),
            Some(b"RIFFwav".to_vec())
        );
    }

    #[test]
    fn wav_writer_outputs_pcm16_header_and_samples() {
        let wav = wav_pcm16_from_i16(&[0, i16::MAX, i16::MIN], 22_050, 1);

        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 6);
        assert_eq!(&wav[44..46], &0i16.to_le_bytes());
        assert_eq!(&wav[46..48], &i16::MAX.to_le_bytes());
        assert_eq!(&wav[48..50], &i16::MIN.to_le_bytes());
    }

    #[test]
    fn collects_unique_prompt_texts_in_stable_order() {
        let config = Config {
            default_tool: "radio".to_string(),
            voice: VoiceConfig {
                model_path: PathBuf::from("/voices/en.onnx"),
                config_path: PathBuf::from("/voices/en.json"),
            },
            cache: CacheConfig::default(),
            audio: AudioConfig::default(),
            globals: GlobalDefaults::default(),
            tools: vec![ToolConfig {
                id: "radio".to_string(),
                label: "Radio".to_string(),
                active_ptt_hold_ms: None,
                active_hooks: ActiveHooks::default(),
                tabs: vec![TabConfig {
                    id: "local".to_string(),
                    label: "Local".to_string(),
                    items: vec![
                        ItemConfig {
                            id: "mute".to_string(),
                            label: "Mute".to_string(),
                            primary_action: "say-muted".to_string(),
                            alternate_action: None,
                        },
                        ItemConfig {
                            id: "radio-copy".to_string(),
                            label: "Radio".to_string(),
                            primary_action: "noop".to_string(),
                            alternate_action: None,
                        },
                    ],
                }],
            }],
            global_tabs: vec![TabConfig {
                id: "tools".to_string(),
                label: "Tools".to_string(),
                items: vec![ItemConfig {
                    id: "radio".to_string(),
                    label: "Radio".to_string(),
                    primary_action: "switch-radio".to_string(),
                    alternate_action: None,
                }],
            }],
            actions: vec![
                ActionConfig::Internal(InternalActionConfig {
                    id: "say-muted".to_string(),
                    command: InternalCommand::Speak,
                    tool: None,
                    text: Some("Muted".to_string()),
                }),
                ActionConfig::Internal(InternalActionConfig {
                    id: "noop".to_string(),
                    command: InternalCommand::Noop,
                    tool: None,
                    text: Some("   ".to_string()),
                }),
                ActionConfig::Command(CommandActionConfig {
                    id: "run".to_string(),
                    argv: vec!["date".to_string()],
                    shell: None,
                    cwd: None,
                    env: HashMap::new(),
                    timeout_ms: None,
                    feedback: FeedbackConfig {
                        start: Some("Running".to_string()),
                        success: Some("Done".to_string()),
                        failure: Some("Muted".to_string()),
                    },
                }),
            ],
        };

        assert_eq!(
            collect_prompt_texts(&config),
            vec![
                "Radio".to_string(),
                "Local".to_string(),
                "Mute".to_string(),
                "Tools".to_string(),
                "Muted".to_string(),
                "Running".to_string(),
                "Done".to_string(),
            ]
        );
    }
}
