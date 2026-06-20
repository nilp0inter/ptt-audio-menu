use crate::{
    config::{Config, DailyLogParakeetConfig, PacketProcessorConfig, RecordingPacketActionConfig},
    menu::RecordingPacketEvent,
    recorder::{Recorder, Recording},
};
use anyhow::{Context, Result};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use parakeet_rs::{ParakeetTDT, TimestampMode, Transcriber};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::SystemTime,
};
use tokio::{process::Command, task::JoinHandle, time};
use tracing::{info, warn};

pub struct PacketRuntime {
    recorder: Recorder,
    config: Arc<Config>,
    workers: Vec<JoinHandle<()>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecordingActionOutcome {
    Started,
    Stopped { packet_id: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PacketMetadata {
    pub packet_id: String,
    pub action_id: String,
    pub tool_id: String,
    pub status: PacketStatus,
    pub audio_path: PathBuf,
    pub created_at: DateTime<Utc>,
    pub started_at: DateTime<Utc>,
    pub stopped_at: DateTime<Utc>,
    pub attempts: u32,
    pub next_retry_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub processed_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PacketStatus {
    Queued,
    Processing,
    Processed,
    DeadLetter,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct DailyLog {
    date: String,
    entries: Vec<DailyLogEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct DailyLogEntry {
    packet_id: String,
    audio_path: PathBuf,
    started_at: DateTime<Utc>,
    stopped_at: DateTime<Utc>,
    transcript: String,
    segments: Vec<TranscriptSegment>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct TranscriptSegment {
    text: String,
    start: f32,
    end: f32,
}

impl PacketRuntime {
    pub fn new(config: Config) -> Result<Self> {
        let config = Arc::new(config);
        let mut workers = Vec::new();

        for action in recording_actions(&config) {
            ensure_action_dirs(action)?;
            recover_processing(action)?;
            workers.push(spawn_worker(config.clone(), action.id.clone()));
        }

        Ok(Self {
            recorder: Recorder::default(),
            config,
            workers,
        })
    }

    pub fn replace_config(&mut self, config: Config) -> Result<()> {
        for worker in self.workers.drain(..) {
            worker.abort();
        }
        self.recorder = Recorder::default();
        let next = PacketRuntime::new(config)?;
        *self = next;
        Ok(())
    }

    pub fn handle_recording_event(
        &mut self,
        action_id: &str,
        tool_id: &str,
        event: RecordingPacketEvent,
    ) -> Result<RecordingActionOutcome> {
        match event {
            RecordingPacketEvent::Start => {
                self.recorder.start()?;
                Ok(RecordingActionOutcome::Started)
            }
            RecordingPacketEvent::Stop => {
                let recording = self.recorder.stop()?;
                let action = recording_action(&self.config, action_id)?;
                let packet_id = write_queued_packet(action, tool_id, recording)?;
                Ok(RecordingActionOutcome::Stopped { packet_id })
            }
        }
    }
}

impl Drop for PacketRuntime {
    fn drop(&mut self) {
        for worker in &self.workers {
            worker.abort();
        }
    }
}

fn spawn_worker(config: Arc<Config>, action_id: String) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            if let Err(err) = process_next_packet(&config, &action_id).await {
                warn!(%action_id, error = ?err, "packet worker iteration failed");
            }
            time::sleep(std::time::Duration::from_millis(1_000)).await;
        }
    })
}

async fn process_next_packet(config: &Config, action_id: &str) -> Result<()> {
    let action = recording_action(config, action_id)?;
    let Some(meta_path) = next_due_metadata(action)? else {
        return Ok(());
    };

    let mut metadata = read_metadata(&meta_path)?;
    move_packet(action, &mut metadata, PacketStatus::Processing)?;
    write_metadata_for_status(action, &metadata)?;

    metadata.attempts += 1;
    metadata.next_retry_at = None;
    write_metadata_for_status(action, &metadata)?;

    let result = process_packet(action, &metadata).await;
    match result {
        Ok(()) => {
            metadata.status = PacketStatus::Processed;
            metadata.processed_at = Some(Utc::now());
            metadata.last_error = None;
            move_packet(action, &mut metadata, PacketStatus::Processed)?;
            info!(
                action_id = %metadata.action_id,
                packet_id = %metadata.packet_id,
                "packet processed"
            );
        }
        Err(err) if metadata.attempts >= action.max_attempts => {
            metadata.status = PacketStatus::DeadLetter;
            metadata.last_error = Some(format!("{err:#}"));
            move_packet(action, &mut metadata, PacketStatus::DeadLetter)?;
            warn!(
                action_id = %metadata.action_id,
                packet_id = %metadata.packet_id,
                error = ?err,
                "packet moved to dead-letter"
            );
        }
        Err(err) => {
            metadata.status = PacketStatus::Queued;
            metadata.last_error = Some(format!("{err:#}"));
            metadata.next_retry_at = Some(Utc::now() + backoff(action, metadata.attempts));
            move_packet(action, &mut metadata, PacketStatus::Queued)?;
            warn!(
                action_id = %metadata.action_id,
                packet_id = %metadata.packet_id,
                attempts = metadata.attempts,
                "packet requeued"
            );
        }
    }

    Ok(())
}

async fn process_packet(
    action: &RecordingPacketActionConfig,
    metadata: &PacketMetadata,
) -> Result<()> {
    match &action.processor {
        PacketProcessorConfig::DailyLogParakeet(processor) => {
            process_daily_log_parakeet(processor, metadata).await
        }
    }
}

async fn process_daily_log_parakeet(
    processor: &DailyLogParakeetConfig,
    metadata: &PacketMetadata,
) -> Result<()> {
    let model_dir = processor.model_dir.clone();
    let audio_path = metadata.audio_path.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<_> {
        let mut model = ParakeetTDT::from_pretrained(model_dir, None)
            .map_err(|err| anyhow::anyhow!(err))
            .context("load Parakeet TDT model")?;
        model
            .transcribe_file(audio_path, Some(TimestampMode::Sentences))
            .map_err(|err| anyhow::anyhow!(err))
            .context("transcribe packet")
    })
    .await
    .context("join Parakeet transcription task")??;

    fs::create_dir_all(&processor.daily_json_dir).with_context(|| {
        format!(
            "create daily JSON directory {}",
            processor.daily_json_dir.display()
        )
    })?;
    fs::create_dir_all(&processor.html_dir)
        .with_context(|| format!("create HTML directory {}", processor.html_dir.display()))?;

    let day = metadata.started_at.format("%Y-%m-%d").to_string();
    let json_path = processor.daily_json_dir.join(format!("{day}.json"));
    let html_path = processor.html_dir.join(format!("{day}.html"));
    let mut log = read_daily_log(&json_path, &day)?;
    log.entries.push(DailyLogEntry {
        packet_id: metadata.packet_id.clone(),
        audio_path: metadata.audio_path.clone(),
        started_at: metadata.started_at,
        stopped_at: metadata.stopped_at,
        transcript: result.text,
        segments: result
            .tokens
            .into_iter()
            .map(|token| TranscriptSegment {
                text: token.text,
                start: token.start,
                end: token.end,
            })
            .collect(),
    });
    write_json(&json_path, &log)?;

    let status = Command::new(&processor.renderer_script)
        .arg(&json_path)
        .arg(&html_path)
        .stdin(Stdio::null())
        .status()
        .await
        .with_context(|| format!("run renderer {}", processor.renderer_script.display()))?;
    if !status.success() {
        anyhow::bail!("renderer exited with status {status}");
    }
    Ok(())
}

fn read_daily_log(path: &Path, day: &str) -> Result<DailyLog> {
    match fs::read_to_string(path) {
        Ok(source) => serde_json::from_str(&source)
            .with_context(|| format!("parse daily log {}", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(DailyLog {
            date: day.to_string(),
            entries: Vec::new(),
        }),
        Err(err) => Err(err).with_context(|| format!("read daily log {}", path.display())),
    }
}

fn write_queued_packet(
    action: &RecordingPacketActionConfig,
    tool_id: &str,
    recording: Recording,
) -> Result<String> {
    ensure_action_dirs(action)?;
    let started_at = DateTime::<Utc>::from(recording.started_at);
    let stopped_at = DateTime::<Utc>::from(recording.stopped_at);
    let packet_id = packet_id(recording.started_at);
    let audio_path = state_dir(action, PacketStatus::Queued).join(format!("{packet_id}.wav"));
    fs::write(&audio_path, recording.wav)
        .with_context(|| format!("write packet WAV {}", audio_path.display()))?;
    let metadata = PacketMetadata {
        packet_id: packet_id.clone(),
        action_id: action.id.clone(),
        tool_id: tool_id.to_string(),
        status: PacketStatus::Queued,
        audio_path,
        created_at: Utc::now(),
        started_at,
        stopped_at,
        attempts: 0,
        next_retry_at: None,
        last_error: None,
        processed_at: None,
    };
    write_metadata_for_status(action, &metadata)?;
    Ok(packet_id)
}

fn packet_id(time: SystemTime) -> String {
    DateTime::<Utc>::from(time)
        .format("%Y%m%dT%H%M%S%.9fZ")
        .to_string()
}

fn next_due_metadata(action: &RecordingPacketActionConfig) -> Result<Option<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(state_dir(action, PacketStatus::Queued))
        .with_context(|| format!("read queued directory {}", action.storage_dir.display()))?
    {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            let metadata = read_metadata(&path)?;
            if metadata
                .next_retry_at
                .map(|retry_at| retry_at <= Utc::now())
                .unwrap_or(true)
            {
                paths.push(path);
            }
        }
    }
    paths.sort();
    Ok(paths.into_iter().next())
}

fn recover_processing(action: &RecordingPacketActionConfig) -> Result<()> {
    let processing = state_dir(action, PacketStatus::Processing);
    for entry in fs::read_dir(&processing)
        .with_context(|| format!("read processing directory {}", processing.display()))?
    {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            let mut metadata = read_metadata(&path)?;
            metadata.status = PacketStatus::Queued;
            metadata.next_retry_at = None;
            metadata.last_error = Some("recovered from stale processing state".to_string());
            move_packet(action, &mut metadata, PacketStatus::Queued)?;
        }
    }
    Ok(())
}

fn move_packet(
    action: &RecordingPacketActionConfig,
    metadata: &mut PacketMetadata,
    status: PacketStatus,
) -> Result<()> {
    let old_audio_path = metadata.audio_path.clone();
    let old_meta_path = metadata_path_for_audio(&old_audio_path);
    metadata.status = status;
    let new_dir = state_dir(action, metadata.status.clone());
    fs::create_dir_all(&new_dir).with_context(|| format!("create {}", new_dir.display()))?;
    let new_audio_path = new_dir.join(format!("{}.wav", metadata.packet_id));
    let new_meta_path = new_dir.join(format!("{}.json", metadata.packet_id));

    if old_audio_path != new_audio_path {
        fs::rename(&old_audio_path, &new_audio_path).with_context(|| {
            format!(
                "move packet WAV {} -> {}",
                old_audio_path.display(),
                new_audio_path.display()
            )
        })?;
    }
    metadata.audio_path = new_audio_path;
    write_json(&new_meta_path, metadata)?;
    if old_meta_path != new_meta_path && old_meta_path.exists() {
        fs::remove_file(&old_meta_path)
            .with_context(|| format!("remove old metadata {}", old_meta_path.display()))?;
    }
    Ok(())
}

fn metadata_path_for_audio(audio_path: &Path) -> PathBuf {
    audio_path.with_extension("json")
}

fn write_metadata_for_status(
    action: &RecordingPacketActionConfig,
    metadata: &PacketMetadata,
) -> Result<()> {
    write_json(
        &state_dir(action, metadata.status.clone()).join(format!("{}.json", metadata.packet_id)),
        metadata,
    )
}

fn read_metadata(path: &Path) -> Result<PacketMetadata> {
    let source =
        fs::read_to_string(path).with_context(|| format!("read metadata {}", path.display()))?;
    serde_json::from_str(&source).with_context(|| format!("parse metadata {}", path.display()))
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let source = serde_json::to_string_pretty(value).context("serialize JSON")?;
    fs::write(path, source).with_context(|| format!("write JSON {}", path.display()))
}

fn backoff(action: &RecordingPacketActionConfig, attempts: u32) -> ChronoDuration {
    let shift = attempts.saturating_sub(1).min(30);
    let multiplier = 1_u64 << shift;
    let delay = action
        .initial_backoff_ms
        .saturating_mul(multiplier)
        .min(action.max_backoff_ms);
    ChronoDuration::milliseconds(delay as i64)
}

fn recording_actions(config: &Config) -> impl Iterator<Item = &RecordingPacketActionConfig> {
    config.actions.iter().filter_map(|action| match action {
        crate::config::ActionConfig::RecordingPacket(action) => Some(action),
        _ => None,
    })
}

fn recording_action<'a>(
    config: &'a Config,
    action_id: &str,
) -> Result<&'a RecordingPacketActionConfig> {
    recording_actions(config)
        .find(|action| action.id == action_id)
        .with_context(|| format!("unknown recording action '{action_id}'"))
}

fn ensure_action_dirs(action: &RecordingPacketActionConfig) -> Result<()> {
    for status in [
        PacketStatus::Queued,
        PacketStatus::Processing,
        PacketStatus::Processed,
        PacketStatus::DeadLetter,
    ] {
        fs::create_dir_all(state_dir(action, status)).with_context(|| {
            format!(
                "create packet state dirs under {}",
                action.storage_dir.display()
            )
        })?;
    }
    Ok(())
}

fn state_dir(action: &RecordingPacketActionConfig, status: PacketStatus) -> PathBuf {
    action.storage_dir.join(match status {
        PacketStatus::Queued => "queued",
        PacketStatus::Processing => "processing",
        PacketStatus::Processed => "processed",
        PacketStatus::DeadLetter => "dead-letter",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RecordingFeedbackConfig;
    use tempfile::TempDir;

    fn action(dir: &Path) -> RecordingPacketActionConfig {
        RecordingPacketActionConfig {
            id: "daily-log-record".to_string(),
            storage_dir: dir.join("packets"),
            processor: PacketProcessorConfig::DailyLogParakeet(DailyLogParakeetConfig {
                model_dir: dir.to_path_buf(),
                daily_json_dir: dir.join("daily"),
                html_dir: dir.join("html"),
                renderer_script: dir.join("render.py"),
            }),
            max_attempts: 3,
            initial_backoff_ms: 1_000,
            max_backoff_ms: 60_000,
            feedback: RecordingFeedbackConfig::default(),
        }
    }

    #[test]
    fn packet_id_is_filesystem_safe() {
        let id = packet_id(SystemTime::UNIX_EPOCH);
        assert_eq!(id, "19700101T000000.000000000Z");
        assert!(id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.'));
    }

    #[test]
    fn writes_packet_wav_and_metadata_to_queued() {
        let dir = TempDir::new().unwrap();
        let action = action(dir.path());
        let recording = Recording {
            started_at: SystemTime::UNIX_EPOCH,
            stopped_at: SystemTime::UNIX_EPOCH,
            wav: b"wav".to_vec(),
        };

        let packet_id = write_queued_packet(&action, "daily-log", recording).unwrap();
        let audio = state_dir(&action, PacketStatus::Queued).join(format!("{packet_id}.wav"));
        let meta = state_dir(&action, PacketStatus::Queued).join(format!("{packet_id}.json"));

        assert_eq!(fs::read(audio).unwrap(), b"wav");
        let metadata = read_metadata(&meta).unwrap();
        assert_eq!(metadata.status, PacketStatus::Queued);
        assert_eq!(metadata.tool_id, "daily-log");
    }

    #[test]
    fn backoff_caps_at_max_delay() {
        let dir = TempDir::new().unwrap();
        let mut action = action(dir.path());
        action.initial_backoff_ms = 1_000;
        action.max_backoff_ms = 2_500;

        assert_eq!(backoff(&action, 1), ChronoDuration::milliseconds(1_000));
        assert_eq!(backoff(&action, 2), ChronoDuration::milliseconds(2_000));
        assert_eq!(backoff(&action, 3), ChronoDuration::milliseconds(2_500));
    }
}
