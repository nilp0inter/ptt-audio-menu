use anyhow::{Context, Result};
use std::time::Instant;
use tokio::io::AsyncReadExt;

mod input;
mod parser;
mod transport;

use input::InputNormalizer;
use parser::{Event, Parser};
use transport::connect_rfcomm_stream;

const DEVICE_ADDR: &str = "00:02:5B:55:FF:01";

#[tokio::main]
async fn main() -> Result<()> {
    println!("device addr={DEVICE_ADDR}");
    let mut stream = connect_rfcomm_stream(DEVICE_ADDR).await?;
    let mut parser = Parser::default();
    let mut input = InputNormalizer::default();
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
