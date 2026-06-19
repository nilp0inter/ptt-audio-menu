use anyhow::{Context, Result};
use clap::Parser as ClapParser;
use std::{path::PathBuf, time::Duration, time::Instant};
use tokio::io::AsyncReadExt;

mod actions;
mod config;
mod input;
mod menu;
mod parser;
mod transport;

use actions::ActionDispatcher;
use config::{load_config, resolve_config_path};
use input::InputNormalizer;
use menu::{MenuOutcome, MenuState};
use parser::{Event, Parser};
use transport::connect_rfcomm_stream;

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
    let active_ptt_hold_threshold = Duration::from_millis(config.globals.active_ptt_hold_ms);

    println!(
        "config path={} default_tool={} active_ptt_hold_ms={}",
        config_path.display(),
        config.default_tool,
        config.globals.active_ptt_hold_ms
    );
    println!("device addr={DEVICE_ADDR}");
    let mut stream = connect_rfcomm_stream(DEVICE_ADDR).await?;
    let mut parser = Parser::default();
    let mut input = InputNormalizer::new(active_ptt_hold_threshold);
    let mut menu = MenuState::new(&config)?;
    let actions = ActionDispatcher::new(&config)?;
    let mut buf = [0u8; 1024];

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
                    if let MenuOutcome::Action { action_id } = menu_outcome {
                        let effect = actions.dispatch(&config, &mut menu, &action_id)?;
                        println!(
                            "action effect={effect:?} phase={:?} active_tool={}",
                            menu.phase(),
                            menu.active_tool(&config).id
                        );
                    }
                }
            }
        }
    }

    Ok(())
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
