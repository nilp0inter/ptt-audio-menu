use anyhow::{Context, Result};
use tokio::io::AsyncReadExt;

mod parser;
mod transport;

use parser::Parser;
use transport::connect_rfcomm_stream;

const DEVICE_ADDR: &str = "00:02:5B:55:FF:01";

#[tokio::main]
async fn main() -> Result<()> {
    println!("device addr={DEVICE_ADDR}");
    let mut stream = connect_rfcomm_stream(DEVICE_ADDR).await?;
    let mut parser = Parser::default();
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
            println!(
                "event button={} number={} action={} token={}",
                event.button, event.number, event.action, event.token
            );
        }
    }

    Ok(())
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
