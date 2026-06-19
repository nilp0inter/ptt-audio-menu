use anyhow::{Context, Result};
use clap::Parser as ClapParser;
use std::collections::HashMap;
use std::{path::PathBuf, time::Duration, time::Instant};
use tokio::io::AsyncReadExt;

mod actions;
mod audio;
mod commands;
mod config;
mod input;
mod menu;
mod parser;
mod transport;
mod tts;

use actions::{ActionDispatcher, ActionEffect};
use audio::AudioPlayer;
use commands::CommandRunner;
use config::{load_config, resolve_config_path, InternalCommand};
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
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config_path = resolve_config_path(cli.config)?;
    let config = load_config(&config_path)?;
    let tts_cache = TtsCache::new(&config)?;
    let prompt_texts = collect_prompt_texts(&config);
    let mut tts_renderer = TtsRenderer::new(&config.voice)?;
    let cached_prompts =
        prerender_prompts(&tts_cache, &mut tts_renderer, &config.voice, &prompt_texts)?;
    let prompt_audio = PromptAudioIndex::new(cached_prompts);
    let active_ptt_hold_threshold = Duration::from_millis(config.globals.active_ptt_hold_ms);

    println!(
        "config path={} default_tool={} active_ptt_hold_ms={}",
        config_path.display(),
        config.default_tool,
        config.globals.active_ptt_hold_ms
    );
    println!("tts cache dir={}", tts_cache.dir().display());
    println!("tts prompt count={}", prompt_texts.len());
    println!("device addr={DEVICE_ADDR}");
    let mut stream = connect_rfcomm_stream(DEVICE_ADDR).await?;
    let mut audio = AudioPlayer::new()?;
    let mut parser = Parser::default();
    let mut input = InputNormalizer::new(active_ptt_hold_threshold);
    let mut menu = MenuState::new(&config)?;
    let actions = ActionDispatcher::new(&config)?;
    let command_runner = CommandRunner::new();
    let mut buf = [0u8; 1024];

    play_prompt(&mut audio, &prompt_audio, &menu.active_tool(&config).label)?;

    loop {
        let n = stream.read(&mut buf).await.context("read RFCOMM stream")?;
        if n == 0 {
            println!("stream eof");
            break;
        }

        let chunk = &buf[..n];
        println!(
            "raw hex={} ascii={:?}",
            hex(chunk),
            String::from_utf8_lossy(chunk)
        );

        for event in parser.push(chunk) {
            print_raw_event(event);
            for input_event in input.push(event, Instant::now()) {
                println!("input event={input_event:?} mode={:?}", input.mode());
                for menu_outcome in menu.push(&config, input_event) {
                    println!(
                        "menu outcome={menu_outcome:?} phase={:?} active_tool={}",
                        menu.phase(),
                        menu.active_tool(&config).id
                    );
                    handle_menu_audio(&mut audio, &prompt_audio, &config, &menu, &menu_outcome)?;
                    if let MenuOutcome::Action { action_id } = menu_outcome {
                        let effect = actions.dispatch(&config, &mut menu, &action_id)?;
                        println!(
                            "action effect={effect:?} phase={:?} active_tool={}",
                            menu.phase(),
                            menu.active_tool(&config).id
                        );
                        handle_action_effect(
                            &command_runner,
                            &mut audio,
                            &prompt_audio,
                            &config,
                            &menu,
                            effect,
                        )
                        .await?;
                    }
                }
            }
        }
    }

    Ok(())
}

async fn handle_action_effect(
    runner: &CommandRunner,
    audio: &mut AudioPlayer,
    prompt_audio: &PromptAudioIndex,
    config: &config::Config,
    menu: &MenuState,
    effect: ActionEffect,
) -> Result<()> {
    match effect {
        ActionEffect::CommandQueued { command } => {
            let runner = runner.clone();
            tokio::spawn(async move {
                let action_id = command.action_id.clone();
                match runner.run(command).await {
                    Ok(execution) => println!(
                        "command action={} outcome={:?}",
                        execution.action_id, execution.outcome
                    ),
                    Err(err) => println!("command action {action_id} failed: {err:#}"),
                }
            });
        }
        ActionEffect::DeferredInternal {
            command: InternalCommand::CancelRunningAction,
            ..
        } => {
            let cancelled = runner.cancel_running().await?;
            println!("cancel_running_action cancelled={cancelled}");
        }
        ActionEffect::DeferredInternal {
            command: InternalCommand::Speak,
            action_id,
        } => {
            if let Some(text) = speak_action_text(config, &action_id) {
                play_prompt(audio, prompt_audio, text)?;
            }
        }
        ActionEffect::DeferredInternal {
            command: InternalCommand::StopAudio,
            ..
        } => {
            audio.stop_current();
        }
        ActionEffect::SwitchedTool { .. } => {
            play_prompt(audio, prompt_audio, &menu.active_tool(config).label)?;
        }
        _ => {}
    }

    Ok(())
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

fn print_raw_event(event: Event) {
    println!(
        "event button={} number={} action={} token={}",
        event.button.as_str(),
        event.number,
        event.action.as_str(),
        event.token
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
