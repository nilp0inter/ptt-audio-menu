use crate::parser::{Button, Event, RawAction};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HardwareMode {
    Active,
    Control,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputEvent {
    ActivePtt,
    EnterControl,
    NextTab,
    ScrollUp,
    ScrollDown,
    Select,
    SosShort { mode: HardwareMode },
    SosLong { mode: HardwareMode },
}

#[derive(Debug)]
pub struct InputNormalizer {
    mode: HardwareMode,
    active_ptt_hold_threshold: Duration,
    active_ptt_pressed_at: Option<Instant>,
    sos_long_seen: bool,
}

impl InputNormalizer {
    pub fn new(active_ptt_hold_threshold: Duration) -> Self {
        Self {
            mode: HardwareMode::Active,
            active_ptt_hold_threshold,
            active_ptt_pressed_at: None,
            sos_long_seen: false,
        }
    }

    pub fn mode(&self) -> HardwareMode {
        self.mode
    }

    pub fn push(&mut self, event: Event, now: Instant) -> Vec<InputEvent> {
        match (event.button, event.action) {
            (Button::Ptt, RawAction::Pressed) => self.push_ptt_pressed(now),
            (Button::Ptt, RawAction::Released) => self.push_ptt_released(now),
            (Button::Group, RawAction::Pressed) => self.push_group_pressed(),
            (Button::Sos, RawAction::Pressed) => {
                self.sos_long_seen = false;
                Vec::new()
            }
            (Button::Sos, RawAction::LongPressed) => {
                self.sos_long_seen = true;
                vec![InputEvent::SosLong { mode: self.mode }]
            }
            (Button::Sos, RawAction::Released) => {
                if self.sos_long_seen {
                    self.sos_long_seen = false;
                    Vec::new()
                } else {
                    vec![InputEvent::SosShort { mode: self.mode }]
                }
            }
            (Button::VolumeUp, RawAction::Clicked) if self.mode == HardwareMode::Control => {
                vec![InputEvent::ScrollUp]
            }
            (Button::VolumeDown, RawAction::Clicked) if self.mode == HardwareMode::Control => {
                vec![InputEvent::ScrollDown]
            }
            _ => Vec::new(),
        }
    }

    fn push_ptt_pressed(&mut self, now: Instant) -> Vec<InputEvent> {
        match self.mode {
            HardwareMode::Active => {
                self.active_ptt_pressed_at = Some(now);
                Vec::new()
            }
            HardwareMode::Control => {
                self.mode = HardwareMode::Active;
                self.active_ptt_pressed_at = None;
                vec![InputEvent::Select]
            }
        }
    }

    fn push_ptt_released(&mut self, now: Instant) -> Vec<InputEvent> {
        if self.mode != HardwareMode::Active {
            return Vec::new();
        }

        let Some(pressed_at) = self.active_ptt_pressed_at.take() else {
            return Vec::new();
        };

        if now.duration_since(pressed_at) >= self.active_ptt_hold_threshold {
            vec![InputEvent::ActivePtt]
        } else {
            Vec::new()
        }
    }

    fn push_group_pressed(&mut self) -> Vec<InputEvent> {
        match self.mode {
            HardwareMode::Active => {
                self.mode = HardwareMode::Control;
                self.active_ptt_pressed_at = None;
                vec![InputEvent::EnterControl]
            }
            HardwareMode::Control => vec![InputEvent::NextTab],
        }
    }
}

impl Default for InputNormalizer {
    fn default() -> Self {
        Self::new(Duration::from_millis(350))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const THRESHOLD: Duration = Duration::from_millis(350);

    fn event(button: Button, action: RawAction) -> Event {
        Event {
            token: "",
            button,
            action,
            number: 0,
        }
    }

    fn at(offset: Duration) -> Instant {
        Instant::now() + offset
    }

    #[test]
    fn group_enters_control_then_cycles_tabs() {
        let mut normalizer = InputNormalizer::new(THRESHOLD);
        let now = at(Duration::ZERO);

        assert_eq!(
            normalizer.push(event(Button::Group, RawAction::Pressed), now),
            vec![InputEvent::EnterControl]
        );
        assert_eq!(normalizer.mode(), HardwareMode::Control);

        assert_eq!(
            normalizer.push(event(Button::Group, RawAction::Pressed), now),
            vec![InputEvent::NextTab]
        );
        assert_eq!(normalizer.mode(), HardwareMode::Control);
    }

    #[test]
    fn volume_scrolls_only_in_control_mode() {
        let mut normalizer = InputNormalizer::new(THRESHOLD);
        let now = at(Duration::ZERO);

        assert!(normalizer
            .push(event(Button::VolumeUp, RawAction::Clicked), now)
            .is_empty());
        normalizer.push(event(Button::Group, RawAction::Pressed), now);

        assert_eq!(
            normalizer.push(event(Button::VolumeUp, RawAction::Clicked), now),
            vec![InputEvent::ScrollUp]
        );
        assert_eq!(
            normalizer.push(event(Button::VolumeDown, RawAction::Clicked), now),
            vec![InputEvent::ScrollDown]
        );
    }

    #[test]
    fn control_ptt_selects_immediately_and_returns_to_active() {
        let mut normalizer = InputNormalizer::new(THRESHOLD);
        let now = at(Duration::ZERO);

        normalizer.push(event(Button::Group, RawAction::Pressed), now);
        assert_eq!(
            normalizer.push(event(Button::Ptt, RawAction::Pressed), now),
            vec![InputEvent::Select]
        );
        assert_eq!(normalizer.mode(), HardwareMode::Active);
    }

    #[test]
    fn active_ptt_suppresses_short_taps() {
        let mut normalizer = InputNormalizer::new(THRESHOLD);
        let start = at(Duration::ZERO);

        assert!(normalizer
            .push(event(Button::Ptt, RawAction::Pressed), start)
            .is_empty());
        assert!(normalizer
            .push(
                event(Button::Ptt, RawAction::Released),
                start + Duration::from_millis(349)
            )
            .is_empty());
    }

    #[test]
    fn active_ptt_emits_after_hold_threshold() {
        let mut normalizer = InputNormalizer::new(THRESHOLD);
        let start = at(Duration::ZERO);

        normalizer.push(event(Button::Ptt, RawAction::Pressed), start);
        assert_eq!(
            normalizer.push(event(Button::Ptt, RawAction::Released), start + THRESHOLD),
            vec![InputEvent::ActivePtt]
        );
    }

    #[test]
    fn sos_short_is_suppressed_after_long_signal() {
        let mut normalizer = InputNormalizer::new(THRESHOLD);
        let now = at(Duration::ZERO);

        assert!(normalizer
            .push(event(Button::Sos, RawAction::Pressed), now)
            .is_empty());
        assert_eq!(
            normalizer.push(event(Button::Sos, RawAction::LongPressed), now),
            vec![InputEvent::SosLong {
                mode: HardwareMode::Active
            }]
        );
        assert!(normalizer
            .push(event(Button::Sos, RawAction::Released), now)
            .is_empty());
    }

    #[test]
    fn control_sos_alternate_action_stays_in_control() {
        let mut normalizer = InputNormalizer::new(THRESHOLD);
        let now = at(Duration::ZERO);

        normalizer.push(event(Button::Group, RawAction::Pressed), now);
        normalizer.push(event(Button::Sos, RawAction::Pressed), now);

        assert_eq!(
            normalizer.push(event(Button::Sos, RawAction::Released), now),
            vec![InputEvent::SosShort {
                mode: HardwareMode::Control
            }]
        );
        assert_eq!(normalizer.mode(), HardwareMode::Control);
    }
}
