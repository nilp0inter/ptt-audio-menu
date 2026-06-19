use anyhow::{bail, Context, Result};
use bluer::{
    rfcomm::{Profile, Role},
    Address, Session,
};
use futures::StreamExt;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::time::timeout;
use uuid::Uuid;

const DEVICE_ADDR: &str = "00:02:5B:55:FF:01";
const SPP_UUID: Uuid = Uuid::from_u128(0x00001101_0000_1000_8000_00805f9b34fb);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const PROFILE_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[tokio::main]
async fn main() -> Result<()> {
    let session = Session::new().await.context("create BlueZ session")?;
    let adapter = session
        .default_adapter()
        .await
        .context("get default Bluetooth adapter")?;
    adapter
        .set_powered(true)
        .await
        .context("power Bluetooth adapter")?;

    let device_addr: Address = DEVICE_ADDR
        .parse()
        .context("parse hardcoded device address")?;
    let device = adapter
        .device(device_addr)
        .context("get BlueZ device handle")?;

    let profile = Profile {
        uuid: SPP_UUID,
        role: Some(Role::Client),
        auto_connect: Some(false),
        require_authentication: Some(false),
        require_authorization: Some(false),
        name: Some("ptt-audio-menu SPP client".to_string()),
        ..Default::default()
    };

    let mut profile_handle = session
        .register_profile(profile)
        .await
        .context("register RFCOMM Serial Port profile")?;

    println!("device addr={DEVICE_ADDR}");
    println!("profile uuid={SPP_UUID}");
    println!("connecting profile");
    let mut connect_task = tokio::spawn({
        let device = device.clone();
        async move { device.connect_profile(&SPP_UUID).await }
    });

    println!("waiting for RFCOMM profile connection");
    let request = timeout(PROFILE_REQUEST_TIMEOUT, async {
        tokio::select! {
            request = profile_handle.next() => {
                request.context("profile connection stream ended before NewConnection")
            }
            connect_result = &mut connect_task => {
                match connect_result {
                    Ok(Ok(())) => bail!("BlueZ profile connection returned before NewConnection"),
                    Ok(Err(err)) => Err(err).context("request BlueZ profile connection"),
                    Err(err) => Err(err).context("join BlueZ profile connection task"),
                }
            }
        }
    })
    .await
    .context("timed out waiting for RFCOMM profile NewConnection")?
    .context("wait for RFCOMM profile NewConnection")?;

    println!(
        "accepted device={} version={:?} features={:?}",
        request.device(),
        request.version(),
        request.features()
    );
    let mut stream = request
        .accept()
        .context("accept RFCOMM profile connection")?;

    timeout(CONNECT_TIMEOUT, connect_task)
        .await
        .context("timed out completing BlueZ profile connection")?
        .context("join BlueZ profile connection task")?
        .context("complete BlueZ profile connection")?;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Event {
    token: &'static str,
    button: &'static str,
    action: &'static str,
    number: u8,
}

const EVENTS: &[(&[u8], Event)] = &[
    (
        b"+PTT=P",
        Event {
            token: "+PTT=P",
            button: "ptt",
            action: "pressed",
            number: 1,
        },
    ),
    (
        b"+PTT=R",
        Event {
            token: "+PTT=R",
            button: "ptt",
            action: "released",
            number: 1,
        },
    ),
    (
        b"C:SP*",
        Event {
            token: "C:SP*",
            button: "sos",
            action: "pressed",
            number: 4,
        },
    ),
    (
        b"C:SR*",
        Event {
            token: "C:SR*",
            button: "sos",
            action: "released",
            number: 4,
        },
    ),
    (
        b"C:SOS*",
        Event {
            token: "C:SOS*",
            button: "sos",
            action: "long-pressed",
            number: 4,
        },
    ),
    (
        b"C:GP*",
        Event {
            token: "C:GP*",
            button: "group",
            action: "pressed",
            number: 6,
        },
    ),
    (
        b"C:GR*",
        Event {
            token: "C:GR*",
            button: "group",
            action: "released",
            number: 6,
        },
    ),
    (
        b"C:VP*",
        Event {
            token: "C:VP*",
            button: "volume-up",
            action: "clicked",
            number: 2,
        },
    ),
    (
        b"C:VM*",
        Event {
            token: "C:VM*",
            button: "volume-down",
            action: "clicked",
            number: 3,
        },
    ),
];

#[derive(Default)]
struct Parser {
    buffer: Vec<u8>,
}

impl Parser {
    fn push(&mut self, bytes: &[u8]) -> Vec<Event> {
        self.buffer.extend_from_slice(bytes);
        let mut events = Vec::new();

        loop {
            if self.buffer.is_empty() {
                break;
            }

            if let Some((token, event)) = self.match_at_start() {
                let mut drain_len = token.len();
                if self.buffer.get(drain_len) == Some(&0) {
                    drain_len += 1;
                }
                self.buffer.drain(..drain_len);
                events.push(event);
                continue;
            }

            if is_prefix_of_known_token(&self.buffer) {
                break;
            }

            if let Some(index) = first_token_index(&self.buffer) {
                self.buffer.drain(..index);
            } else {
                let keep = longest_known_prefix_suffix(&self.buffer);
                let drain_to = self.buffer.len().saturating_sub(keep);
                self.buffer.drain(..drain_to);
                break;
            }
        }

        events
    }

    fn match_at_start(&self) -> Option<(&'static [u8], Event)> {
        EVENTS
            .iter()
            .find_map(|(token, event)| self.buffer.starts_with(token).then_some((*token, *event)))
    }
}

fn is_prefix_of_known_token(bytes: &[u8]) -> bool {
    EVENTS.iter().any(|(token, _)| token.starts_with(bytes))
}

fn first_token_index(bytes: &[u8]) -> Option<usize> {
    (0..bytes.len()).find(|&index| {
        EVENTS
            .iter()
            .any(|(token, _)| bytes[index..].starts_with(token))
    })
}

fn longest_known_prefix_suffix(bytes: &[u8]) -> usize {
    let max = EVENTS
        .iter()
        .map(|(token, _)| token.len().saturating_sub(1))
        .max()
        .unwrap_or(0)
        .min(bytes.len());

    (1..=max)
        .rev()
        .find(|&len| is_prefix_of_known_token(&bytes[bytes.len() - len..]))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(chunks: &[&[u8]]) -> Vec<Event> {
        let mut parser = Parser::default();
        chunks.iter().flat_map(|chunk| parser.push(chunk)).collect()
    }

    #[test]
    fn parses_single_complete_token() {
        assert_eq!(
            parse(&[b"+PTT=P"]),
            vec![Event {
                token: "+PTT=P",
                button: "ptt",
                action: "pressed",
                number: 1
            }]
        );
    }

    #[test]
    fn parses_concatenated_tokens() {
        let events = parse(&[b"+PTT=P+PTT=RC:GP*C:GR*"]);
        assert_eq!(
            events.iter().map(|event| event.token).collect::<Vec<_>>(),
            ["+PTT=P", "+PTT=R", "C:GP*", "C:GR*"]
        );
    }

    #[test]
    fn parses_tokens_split_across_reads() {
        let events = parse(&[b"+PT", b"T=PC:S", b"R*"]);
        assert_eq!(
            events.iter().map(|event| event.token).collect::<Vec<_>>(),
            ["+PTT=P", "C:SR*"]
        );
    }

    #[test]
    fn ignores_optional_nul_after_tokens() {
        let events = parse(&[b"C:SOS*\0C:VM*\0C:VP*\0"]);
        assert_eq!(
            events.iter().map(|event| event.token).collect::<Vec<_>>(),
            ["C:SOS*", "C:VM*", "C:VP*"]
        );
    }

    #[test]
    fn skips_noise_before_valid_token() {
        let events = parse(&[b"noise C:SP*"]);
        assert_eq!(
            events.iter().map(|event| event.token).collect::<Vec<_>>(),
            ["C:SP*"]
        );
    }

    #[test]
    fn retains_incomplete_suffix() {
        let mut parser = Parser::default();
        assert!(parser.push(b"abcC:S").is_empty());
        let events = parser.push(b"OS*");
        assert_eq!(
            events.iter().map(|event| event.token).collect::<Vec<_>>(),
            ["C:SOS*"]
        );
    }
}
