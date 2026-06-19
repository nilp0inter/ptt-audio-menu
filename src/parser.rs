#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Event {
    pub token: &'static str,
    pub button: Button,
    pub action: RawAction,
    pub number: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Button {
    Ptt,
    Sos,
    Group,
    VolumeUp,
    VolumeDown,
}

impl Button {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ptt => "ptt",
            Self::Sos => "sos",
            Self::Group => "group",
            Self::VolumeUp => "volume-up",
            Self::VolumeDown => "volume-down",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RawAction {
    Pressed,
    Released,
    LongPressed,
    Clicked,
}

impl RawAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pressed => "pressed",
            Self::Released => "released",
            Self::LongPressed => "long-pressed",
            Self::Clicked => "clicked",
        }
    }
}

const EVENTS: &[(&[u8], Event)] = &[
    (
        b"+PTT=P",
        Event {
            token: "+PTT=P",
            button: Button::Ptt,
            action: RawAction::Pressed,
            number: 1,
        },
    ),
    (
        b"+PTT=R",
        Event {
            token: "+PTT=R",
            button: Button::Ptt,
            action: RawAction::Released,
            number: 1,
        },
    ),
    (
        b"C:SP*",
        Event {
            token: "C:SP*",
            button: Button::Sos,
            action: RawAction::Pressed,
            number: 4,
        },
    ),
    (
        b"C:SR*",
        Event {
            token: "C:SR*",
            button: Button::Sos,
            action: RawAction::Released,
            number: 4,
        },
    ),
    (
        b"C:SOS*",
        Event {
            token: "C:SOS*",
            button: Button::Sos,
            action: RawAction::LongPressed,
            number: 4,
        },
    ),
    (
        b"C:GP*",
        Event {
            token: "C:GP*",
            button: Button::Group,
            action: RawAction::Pressed,
            number: 6,
        },
    ),
    (
        b"C:GR*",
        Event {
            token: "C:GR*",
            button: Button::Group,
            action: RawAction::Released,
            number: 6,
        },
    ),
    (
        b"C:VP*",
        Event {
            token: "C:VP*",
            button: Button::VolumeUp,
            action: RawAction::Clicked,
            number: 2,
        },
    ),
    (
        b"C:VM*",
        Event {
            token: "C:VM*",
            button: Button::VolumeDown,
            action: RawAction::Clicked,
            number: 3,
        },
    ),
];

#[derive(Default)]
pub struct Parser {
    buffer: Vec<u8>,
}

impl Parser {
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Event> {
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
                button: Button::Ptt,
                action: RawAction::Pressed,
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
