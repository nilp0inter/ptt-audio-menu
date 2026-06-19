use anyhow::{Context, Result};
use clap::Parser as ClapParser;
use std::collections::HashMap;
use std::{path::PathBuf, time::Duration, time::Instant};
use tokio::io::AsyncReadExt;
use tracing::{debug, error, info, warn};
use tracing_subscriber::{fmt, EnvFilter};

mod actions;
mod audio;
mod commands;
mod config;
mod input;
mod menu;
mod parser;
mod transport;
mod tts;

use actions::{ActionDispatcher, ActionEffect, CommandFeedback};
use audio::AudioPlayer;
use commands::{CommandExecution, CommandOutcome, CommandRunner};
use config::{load_config, resolve_config_path, Config, InternalCommand};
use input::InputNormalizer;
use menu::{MenuOutcome, MenuState};
use parser::{Event, Parser};
use transport::connect_rfcomm_stream;
use tts::{collect_prompt_texts, prerender_prompts, CachedPrompt, TtsCache, TtsRenderer};

const DEVICE_ADDR: &str = "00:02:5B:55:FF:01";

#[derive(Debug, ClapParser)]
struct Cli {
    #[arg(long)]
    config: Option<PathBuf>,

    /// Validate the resolved TOML config and exit before TTS or Bluetooth startup.
    #[arg(long)]
    check_config: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logging();
    let cli = Cli::parse();
    let config_path = resolve_config_path(cli.config)?;
    if cli.check_config {
        let config = load_config(&config_path)?;
        info!(
            config_path = %config_path.display(),
            default_tool = %config.default_tool,
            "config validation passed"
        );
        return Ok(());
    }

    let mut runtime = RuntimeState::load(config_path.clone())?;
    let active_ptt_hold_threshold =
        Duration::from_millis(runtime.config.globals.active_ptt_hold_ms);

    info!(
        config_path = %config_path.display(),
        default_tool = %runtime.config.default_tool,
        active_ptt_hold_ms = runtime.config.globals.active_ptt_hold_ms,
        "loaded config"
    );
    info!(tts_cache_dir = %runtime.tts_cache_dir.display(), "resolved TTS cache");
    info!(prompt_count = runtime.prompt_count, "prepared TTS prompts");
    info!(device_addr = DEVICE_ADDR, "connecting device");
    let mut stream = connect_rfcomm_stream(DEVICE_ADDR).await?;
    let mut audio = AudioPlayer::new()?;
    let mut parser = Parser::default();
    let mut input = InputNormalizer::new(active_ptt_hold_threshold);
    let mut menu = MenuState::new(&runtime.config)?;
    let command_runner = CommandRunner::new();
    let (command_tx, mut command_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut buf = [0u8; 1024];

    play_prompt(
        &mut audio,
        &runtime.prompt_audio,
        &menu.active_tool(&runtime.config).label,
    )?;

    loop {
        tokio::select! {
            read_result = stream.read(&mut buf) => {
                let n = read_result.context("read RFCOMM stream")?;
                if n == 0 {
                    info!("RFCOMM stream ended");
                    break;
                }
                process_chunk(
                    &buf[..n],
                    &mut parser,
                    &mut input,
                    &mut menu,
                    &mut runtime,
                    &command_runner,
                    &command_tx,
                    &mut audio,
                )
                .await?;
            }
            Some(completion) = command_rx.recv() => {
                handle_command_completion(&mut audio, &runtime.prompt_audio, completion)?;
            }
        }
    }

    Ok(())
}

async fn process_chunk(
    chunk: &[u8],
    parser: &mut Parser,
    input: &mut InputNormalizer,
    menu: &mut MenuState,
    runtime: &mut RuntimeState,
    command_runner: &CommandRunner,
    command_tx: &tokio::sync::mpsc::UnboundedSender<CommandCompletion>,
    audio: &mut AudioPlayer,
) -> Result<()> {
    debug!(
        hex = %hex(chunk),
        ascii = ?String::from_utf8_lossy(chunk),
        "received RFCOMM chunk"
    );

    for event in parser.push(chunk) {
        log_raw_event(event);
        for input_event in input.push(event, Instant::now()) {
            debug!(event = ?input_event, mode = ?input.mode(), "normalized input event");
            for menu_outcome in menu.push(&runtime.config, input_event) {
                debug!(
                    outcome = ?menu_outcome,
                    phase = ?menu.phase(),
                    active_tool = %menu.active_tool(&runtime.config).id,
                    "menu outcome"
                );
                handle_menu_audio(
                    audio,
                    &runtime.prompt_audio,
                    &runtime.config,
                    menu,
                    &menu_outcome,
                )?;
                if let MenuOutcome::Action { action_id } = menu_outcome {
                    let effect = runtime
                        .actions
                        .dispatch(&runtime.config, menu, &action_id)?;
                    debug!(
                        effect = ?effect,
                        phase = ?menu.phase(),
                        active_tool = %menu.active_tool(&runtime.config).id,
                        "action effect"
                    );
                    handle_action_effect(
                        command_runner,
                        command_tx,
                        audio,
                        runtime,
                        menu,
                        input,
                        effect,
                    )
                    .await?;
                }
            }
        }
    }

    Ok(())
}

async fn handle_action_effect(
    runner: &CommandRunner,
    command_tx: &tokio::sync::mpsc::UnboundedSender<CommandCompletion>,
    audio: &mut AudioPlayer,
    runtime: &mut RuntimeState,
    menu: &mut MenuState,
    input: &mut InputNormalizer,
    effect: ActionEffect,
) -> Result<()> {
    match effect {
        ActionEffect::CommandQueued { command } => {
            if let Some(text) = command.feedback.start.as_deref() {
                play_prompt(audio, &runtime.prompt_audio, text)?;
            }
            let runner = runner.clone();
            let command_tx = command_tx.clone();
            tokio::spawn(async move {
                let action_id = command.action_id.clone();
                let feedback = command.feedback.clone();
                let result = runner.run(command).await;
                if command_tx
                    .send(CommandCompletion {
                        action_id,
                        feedback,
                        result,
                    })
                    .is_err()
                {
                    warn!("command completion receiver dropped");
                }
            });
        }
        ActionEffect::DeferredInternal {
            command: InternalCommand::CancelRunningAction,
            ..
        } => {
            let cancelled = runner.cancel_running().await?;
            info!(cancelled, "cancel_running_action completed");
        }
        ActionEffect::DeferredInternal {
            command: InternalCommand::Speak,
            action_id,
        } => {
            if let Some(text) = speak_action_text(&runtime.config, &action_id) {
                play_prompt(audio, &runtime.prompt_audio, text)?;
            }
        }
        ActionEffect::DeferredInternal {
            command: InternalCommand::StopAudio,
            ..
        } => {
            audio.stop_current();
        }
        ActionEffect::DeferredInternal {
            command: InternalCommand::ReloadConfig,
            action_id,
        } => match RuntimeState::load(runtime.config_path.clone()) {
            Ok(next_runtime) => {
                *runtime = next_runtime;
                *menu = MenuState::new(&runtime.config)?;
                *input = InputNormalizer::new(Duration::from_millis(
                    runtime.config.globals.active_ptt_hold_ms,
                ));
                play_prompt(
                    audio,
                    &runtime.prompt_audio,
                    &menu.active_tool(&runtime.config).label,
                )?;
                info!(
                    action_id = %action_id,
                    config_path = %runtime.config_path.display(),
                    prompt_count = runtime.prompt_count,
                    "reloaded config"
                );
            }
            Err(err) => {
                if let Some(text) = speak_action_text(&runtime.config, &action_id) {
                    let _ = play_prompt(audio, &runtime.prompt_audio, text);
                }
                error!(
                    action_id = %action_id,
                    config_path = %runtime.config_path.display(),
                    error = ?err,
                    "reload_config failed"
                );
                std::process::exit(1);
            }
        },
        ActionEffect::SwitchedTool { .. } => {
            play_prompt(
                audio,
                &runtime.prompt_audio,
                &menu.active_tool(&runtime.config).label,
            )?;
        }
        _ => {}
    }

    Ok(())
}

fn handle_command_completion(
    audio: &mut AudioPlayer,
    prompt_audio: &PromptAudioIndex,
    completion: CommandCompletion,
) -> Result<()> {
    match completion.result {
        Ok(execution) => {
            info!(
                action_id = %execution.action_id,
                outcome = ?execution.outcome,
                "command completed"
            );
            let text = command_outcome_feedback(&completion.feedback, &execution);
            if let Some(text) = text {
                play_prompt(audio, prompt_audio, text)?;
            }
        }
        Err(err) => {
            warn!(
                action_id = %completion.action_id,
                error = ?err,
                "command failed"
            );
            if let Some(text) = completion.feedback.failure.as_deref() {
                play_prompt(audio, prompt_audio, text)?;
            }
        }
    }
    Ok(())
}

fn command_outcome_feedback<'a>(
    feedback: &'a CommandFeedback,
    execution: &CommandExecution,
) -> Option<&'a str> {
    match execution.outcome {
        CommandOutcome::Success => feedback.success.as_deref(),
        CommandOutcome::Failed { .. } | CommandOutcome::TimedOut | CommandOutcome::Cancelled => {
            feedback.failure.as_deref()
        }
    }
}

fn handle_menu_audio(
    audio: &mut AudioPlayer,
    prompt_audio: &PromptAudioIndex,
    config: &config::Config,
    menu: &MenuState,
    outcome: &MenuOutcome,
) -> Result<()> {
    match outcome {
        MenuOutcome::EnteredControl { .. } | MenuOutcome::FocusChanged { .. } => {
            if let Some(text) = menu.focused_prompt_text(config) {
                play_prompt(audio, prompt_audio, text)?;
            }
        }
        MenuOutcome::Action { .. } => {}
    }
    Ok(())
}

fn play_prompt(audio: &mut AudioPlayer, prompt_audio: &PromptAudioIndex, text: &str) -> Result<()> {
    if let Some(path) = prompt_audio.path_for(text) {
        audio.play_interrupting(path)?;
    }
    Ok(())
}

fn speak_action_text<'a>(config: &'a config::Config, action_id: &str) -> Option<&'a str> {
    config.actions.iter().find_map(|action| match action {
        config::ActionConfig::Internal(action) if action.id == action_id => action.text.as_deref(),
        _ => None,
    })
}

#[derive(Debug)]
struct RuntimeState {
    config_path: PathBuf,
    config: Config,
    tts_cache_dir: PathBuf,
    prompt_count: usize,
    prompt_audio: PromptAudioIndex,
    actions: ActionDispatcher,
}

impl RuntimeState {
    fn load(config_path: PathBuf) -> Result<Self> {
        let config = load_config(&config_path)?;
        let tts_cache = TtsCache::new(&config)?;
        let tts_cache_dir = tts_cache.dir().to_path_buf();
        let prompt_texts = collect_prompt_texts(&config);
        let mut tts_renderer = TtsRenderer::new(&config.voice)?;
        let cached_prompts =
            prerender_prompts(&tts_cache, &mut tts_renderer, &config.voice, &prompt_texts)?;
        let prompt_count = prompt_texts.len();
        let prompt_audio = PromptAudioIndex::new(cached_prompts);
        let actions = ActionDispatcher::new(&config)?;

        Ok(Self {
            config_path,
            config,
            tts_cache_dir,
            prompt_count,
            prompt_audio,
            actions,
        })
    }
}

#[derive(Debug)]
struct CommandCompletion {
    action_id: String,
    feedback: CommandFeedback,
    result: Result<CommandExecution>,
}

#[derive(Debug)]
struct PromptAudioIndex {
    paths: HashMap<String, PathBuf>,
}

impl PromptAudioIndex {
    fn new(prompts: Vec<CachedPrompt>) -> Self {
        let paths = prompts
            .into_iter()
            .map(|prompt| (prompt.text().to_string(), prompt.path().to_path_buf()))
            .collect();
        Self { paths }
    }

    fn path_for(&self, text: &str) -> Option<&std::path::Path> {
        self.paths.get(text.trim()).map(PathBuf::as_path)
    }
}

fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stdout)
        .init();
}

fn log_raw_event(event: Event) {
    debug!(
        button = event.button.as_str(),
        number = event.number,
        action = event.action.as_str(),
        token = event.token,
        "parsed raw event"
    );
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn execution(outcome: CommandOutcome) -> CommandExecution {
        CommandExecution {
            action_id: "run".to_string(),
            outcome,
        }
    }

    #[test]
    fn command_outcome_feedback_uses_success_or_failure_label() {
        let feedback = CommandFeedback {
            start: Some("Starting".to_string()),
            success: Some("Done".to_string()),
            failure: Some("Failed".to_string()),
        };

        assert_eq!(
            command_outcome_feedback(&feedback, &execution(CommandOutcome::Success)),
            Some("Done")
        );
        assert_eq!(
            command_outcome_feedback(
                &feedback,
                &execution(CommandOutcome::Failed { code: Some(1) })
            ),
            Some("Failed")
        );
        assert_eq!(
            command_outcome_feedback(&feedback, &execution(CommandOutcome::TimedOut)),
            Some("Failed")
        );
        assert_eq!(
            command_outcome_feedback(&feedback, &execution(CommandOutcome::Cancelled)),
            Some("Failed")
        );
    }
}
