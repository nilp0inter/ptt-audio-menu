use crate::{
    config::{ActionConfig, Config, TabConfig, ToolConfig},
    input::{HardwareMode, InputEvent},
};
use anyhow::{bail, Result};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuPhase {
    Active,
    Control,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MenuOutcome {
    EnteredControl {
        tab_id: String,
        item_id: Option<String>,
    },
    FocusChanged {
        tab_id: String,
        item_id: Option<String>,
    },
    ItemChanged {
        tab_id: String,
        item_id: Option<String>,
    },
    Action {
        action_id: String,
    },
    RecordingPacket {
        action_id: String,
        event: RecordingPacketEvent,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordingPacketEvent {
    Start,
    Stop,
}

#[derive(Debug)]
pub struct MenuState {
    active_tool_index: usize,
    phase: MenuPhase,
    selected_tab_index: usize,
    selected_item_index: usize,
}

impl MenuState {
    pub fn new(config: &Config) -> Result<Self> {
        let Some(active_tool_index) = config
            .tools
            .iter()
            .position(|tool| tool.id == config.default_tool)
        else {
            bail!(
                "default_tool references unknown tool '{}'",
                config.default_tool
            );
        };

        Ok(Self {
            active_tool_index,
            phase: MenuPhase::Active,
            selected_tab_index: 0,
            selected_item_index: 0,
        })
    }

    pub fn phase(&self) -> MenuPhase {
        self.phase
    }

    pub fn active_tool<'a>(&self, config: &'a Config) -> &'a ToolConfig {
        &config.tools[self.active_tool_index]
    }

    pub fn focused_tab_prompt_text<'a>(&self, config: &'a Config) -> Option<&'a str> {
        self.selected_tab(config).map(|tab| tab.label.as_str())
    }

    pub fn focused_item_prompt_text<'a>(&self, config: &'a Config) -> Option<&'a str> {
        self.selected_item(config).map(|item| item.label.as_str())
    }

    pub fn switch_tool(&mut self, config: &Config, tool_id: &str) -> Result<()> {
        let Some(active_tool_index) = config.tools.iter().position(|tool| tool.id == tool_id)
        else {
            bail!("unknown tool '{tool_id}'");
        };

        self.active_tool_index = active_tool_index;
        self.phase = MenuPhase::Active;
        self.clamp_focus(config);
        Ok(())
    }

    pub fn exit_control(&mut self) {
        self.phase = MenuPhase::Active;
    }

    pub fn push(&mut self, config: &Config, event: InputEvent) -> Vec<MenuOutcome> {
        match event {
            InputEvent::ActivePttPressed if self.phase == MenuPhase::Active => self
                .recording_active_ptt_action(config)
                .map(|action_id| recording_outcome(action_id, RecordingPacketEvent::Start))
                .into_iter()
                .collect(),
            InputEvent::ActivePttReleased if self.phase == MenuPhase::Active => self
                .recording_active_ptt_action(config)
                .map(|action_id| recording_outcome(action_id, RecordingPacketEvent::Stop))
                .into_iter()
                .collect(),
            InputEvent::ActivePtt if self.phase == MenuPhase::Active => self
                .non_recording_active_ptt_action(config)
                .map(action_outcome)
                .into_iter()
                .collect(),
            InputEvent::EnterControl => {
                self.phase = MenuPhase::Control;
                self.clamp_focus(config);
                vec![self.focus_outcome(config, MenuOutcomeKind::EnteredControl)]
            }
            InputEvent::NextTab if self.phase == MenuPhase::Control => {
                let tab_count = self.control_tab_count(config);
                if tab_count > 0 {
                    self.selected_tab_index = (self.selected_tab_index + 1) % tab_count;
                    self.selected_item_index = 0;
                }
                vec![self.focus_outcome(config, MenuOutcomeKind::FocusChanged)]
            }
            InputEvent::ScrollUp if self.phase == MenuPhase::Control => {
                self.scroll_item(config, ScrollDirection::Previous);
                vec![self.focus_outcome(config, MenuOutcomeKind::ItemChanged)]
            }
            InputEvent::ScrollDown if self.phase == MenuPhase::Control => {
                self.scroll_item(config, ScrollDirection::Next);
                vec![self.focus_outcome(config, MenuOutcomeKind::ItemChanged)]
            }
            InputEvent::Select if self.phase == MenuPhase::Control => {
                self.phase = MenuPhase::Active;
                self.focused_primary_action(config)
                    .map(action_outcome)
                    .into_iter()
                    .collect()
            }
            InputEvent::SosShort {
                mode: HardwareMode::Active,
            } if self.phase == MenuPhase::Active => self
                .active_tool(config)
                .active_hooks
                .sos_short
                .as_ref()
                .map(action_outcome)
                .into_iter()
                .collect(),
            InputEvent::SosLong {
                mode: HardwareMode::Active,
            } if self.phase == MenuPhase::Active => self
                .active_tool(config)
                .active_hooks
                .sos_long
                .as_ref()
                .map(action_outcome)
                .into_iter()
                .collect(),
            InputEvent::SosShort {
                mode: HardwareMode::Control,
            }
            | InputEvent::SosLong {
                mode: HardwareMode::Control,
            } if self.phase == MenuPhase::Control => self
                .focused_alternate_action(config)
                .map(action_outcome)
                .into_iter()
                .collect(),
            _ => Vec::new(),
        }
    }

    fn control_tab_count(&self, config: &Config) -> usize {
        config.global_tabs.len() + self.active_tool(config).tabs.len()
    }

    fn selected_tab<'a>(&self, config: &'a Config) -> Option<&'a TabConfig> {
        let global_tab_count = config.global_tabs.len();
        if self.selected_tab_index < global_tab_count {
            return config.global_tabs.get(self.selected_tab_index);
        }

        self.active_tool(config)
            .tabs
            .get(self.selected_tab_index.checked_sub(global_tab_count)?)
    }

    fn selected_item<'a>(&self, config: &'a Config) -> Option<&'a crate::config::ItemConfig> {
        self.selected_tab(config)
            .and_then(|tab| tab.items.get(self.selected_item_index))
    }

    fn focused_primary_action<'a>(&self, config: &'a Config) -> Option<&'a String> {
        self.selected_item(config).map(|item| &item.primary_action)
    }

    fn focused_alternate_action<'a>(&self, config: &'a Config) -> Option<&'a String> {
        self.selected_item(config)
            .and_then(|item| item.alternate_action.as_ref())
    }

    fn non_recording_active_ptt_action<'a>(&self, config: &'a Config) -> Option<&'a String> {
        let action_id = self.active_tool(config).active_hooks.ptt.as_ref()?;
        if is_recording_packet_action(config, action_id) {
            None
        } else {
            Some(action_id)
        }
    }

    fn recording_active_ptt_action<'a>(&self, config: &'a Config) -> Option<&'a String> {
        let action_id = self.active_tool(config).active_hooks.ptt.as_ref()?;
        if is_recording_packet_action(config, action_id) {
            Some(action_id)
        } else {
            None
        }
    }

    fn clamp_focus(&mut self, config: &Config) {
        let tab_count = self.control_tab_count(config);
        if tab_count == 0 {
            self.selected_tab_index = 0;
            self.selected_item_index = 0;
            return;
        }

        if self.selected_tab_index >= tab_count {
            self.selected_tab_index = 0;
        }

        let item_count = self
            .selected_tab(config)
            .map(|tab| tab.items.len())
            .unwrap_or(0);
        if item_count == 0 || self.selected_item_index >= item_count {
            self.selected_item_index = 0;
        }
    }

    fn scroll_item(&mut self, config: &Config, direction: ScrollDirection) {
        let Some(tab) = self.selected_tab(config) else {
            self.selected_item_index = 0;
            return;
        };

        let item_count = tab.items.len();
        if item_count == 0 {
            self.selected_item_index = 0;
            return;
        }

        self.selected_item_index %= item_count;
        match direction {
            ScrollDirection::Previous => {
                self.selected_item_index = if self.selected_item_index == 0 {
                    item_count - 1
                } else {
                    self.selected_item_index - 1
                };
            }
            ScrollDirection::Next => {
                self.selected_item_index = (self.selected_item_index + 1) % item_count;
            }
        }
    }

    fn focus_outcome(&self, config: &Config, kind: MenuOutcomeKind) -> MenuOutcome {
        let tab_id = self
            .selected_tab(config)
            .map(|tab| tab.id.clone())
            .unwrap_or_default();
        let item_id = self.selected_item(config).map(|item| item.id.clone());

        match kind {
            MenuOutcomeKind::EnteredControl => MenuOutcome::EnteredControl { tab_id, item_id },
            MenuOutcomeKind::FocusChanged => MenuOutcome::FocusChanged { tab_id, item_id },
            MenuOutcomeKind::ItemChanged => MenuOutcome::ItemChanged { tab_id, item_id },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScrollDirection {
    Previous,
    Next,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MenuOutcomeKind {
    EnteredControl,
    FocusChanged,
    ItemChanged,
}

fn action_outcome(action_id: &String) -> MenuOutcome {
    MenuOutcome::Action {
        action_id: action_id.clone(),
    }
}

fn recording_outcome(action_id: &String, event: RecordingPacketEvent) -> MenuOutcome {
    MenuOutcome::RecordingPacket {
        action_id: action_id.clone(),
        event,
    }
}

fn is_recording_packet_action(config: &Config, action_id: &str) -> bool {
    config.actions.iter().any(
        |action| matches!(action, ActionConfig::RecordingPacket(action) if action.id == action_id),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        ActionConfig, ActiveHooks, AudioConfig, CacheConfig, GlobalDefaults, InternalActionConfig,
        InternalCommand, ItemConfig, VoiceConfig,
    };
    use std::path::PathBuf;

    fn item(id: &str, primary_action: &str, alternate_action: Option<&str>) -> ItemConfig {
        ItemConfig {
            id: id.to_string(),
            label: id.to_string(),
            primary_action: primary_action.to_string(),
            alternate_action: alternate_action.map(str::to_string),
        }
    }

    fn tab(id: &str, items: Vec<ItemConfig>) -> TabConfig {
        TabConfig {
            id: id.to_string(),
            label: id.to_string(),
            items,
        }
    }

    fn action(id: &str) -> ActionConfig {
        ActionConfig::Internal(InternalActionConfig {
            id: id.to_string(),
            command: InternalCommand::Noop,
            tool: None,
            text: None,
        })
    }

    fn config() -> Config {
        Config {
            default_tool: "radio".to_string(),
            voice: VoiceConfig {
                model_path: PathBuf::from("voice.onnx"),
                config_path: PathBuf::from("voice.json"),
            },
            cache: CacheConfig::default(),
            audio: AudioConfig::default(),
            globals: GlobalDefaults::default(),
            tools: vec![ToolConfig {
                id: "radio".to_string(),
                label: "Radio".to_string(),
                active_ptt_hold_ms: None,
                active_hooks: ActiveHooks {
                    ptt: Some("talk".to_string()),
                    sos_short: Some("favorite".to_string()),
                    sos_long: Some("panic".to_string()),
                },
                tabs: vec![
                    tab(
                        "local",
                        vec![
                            item("mute", "mute", Some("mute-alt")),
                            item("squelch", "squelch", Some("squelch-alt")),
                        ],
                    ),
                    tab("empty", Vec::new()),
                ],
            }],
            global_tabs: vec![tab(
                "tools",
                vec![item("radio", "switch-radio", Some("preview-radio"))],
            )],
            actions: vec![
                action("talk"),
                action("favorite"),
                action("panic"),
                action("switch-radio"),
                action("preview-radio"),
                action("mute"),
                action("mute-alt"),
                action("squelch"),
                action("squelch-alt"),
            ],
        }
    }

    #[test]
    fn group_enters_control_on_first_global_tab() {
        let config = config();
        let mut menu = MenuState::new(&config).unwrap();

        assert_eq!(
            menu.push(&config, InputEvent::EnterControl),
            vec![MenuOutcome::EnteredControl {
                tab_id: "tools".to_string(),
                item_id: Some("radio".to_string()),
            }]
        );
        assert_eq!(menu.phase(), MenuPhase::Control);
    }

    #[test]
    fn group_cycles_tabs_in_control() {
        let config = config();
        let mut menu = MenuState::new(&config).unwrap();

        menu.push(&config, InputEvent::EnterControl);

        assert_eq!(
            menu.push(&config, InputEvent::NextTab),
            vec![MenuOutcome::FocusChanged {
                tab_id: "local".to_string(),
                item_id: Some("mute".to_string()),
            }]
        );
        assert_eq!(
            menu.push(&config, InputEvent::NextTab),
            vec![MenuOutcome::FocusChanged {
                tab_id: "empty".to_string(),
                item_id: None,
            }]
        );
        assert_eq!(
            menu.push(&config, InputEvent::NextTab),
            vec![MenuOutcome::FocusChanged {
                tab_id: "tools".to_string(),
                item_id: Some("radio".to_string()),
            }]
        );
    }

    #[test]
    fn volume_scrolls_items_in_control() {
        let config = config();
        let mut menu = MenuState::new(&config).unwrap();

        menu.push(&config, InputEvent::EnterControl);
        menu.push(&config, InputEvent::NextTab);

        assert_eq!(
            menu.push(&config, InputEvent::ScrollDown),
            vec![MenuOutcome::ItemChanged {
                tab_id: "local".to_string(),
                item_id: Some("squelch".to_string()),
            }]
        );
        assert_eq!(
            menu.push(&config, InputEvent::ScrollUp),
            vec![MenuOutcome::ItemChanged {
                tab_id: "local".to_string(),
                item_id: Some("mute".to_string()),
            }]
        );
    }

    #[test]
    fn ptt_selection_exits_control_and_returns_primary_action() {
        let config = config();
        let mut menu = MenuState::new(&config).unwrap();

        menu.push(&config, InputEvent::EnterControl);

        assert_eq!(
            menu.push(&config, InputEvent::Select),
            vec![MenuOutcome::Action {
                action_id: "switch-radio".to_string(),
            }]
        );
        assert_eq!(menu.phase(), MenuPhase::Active);
    }

    #[test]
    fn entering_control_returns_to_last_selected_tab() {
        let config = config();
        let mut menu = MenuState::new(&config).unwrap();

        menu.push(&config, InputEvent::EnterControl);
        menu.push(&config, InputEvent::NextTab);
        menu.push(&config, InputEvent::Select);

        assert_eq!(
            menu.push(&config, InputEvent::EnterControl),
            vec![MenuOutcome::EnteredControl {
                tab_id: "local".to_string(),
                item_id: Some("mute".to_string()),
            }]
        );
    }

    #[test]
    fn switch_tool_preserves_last_tab_when_still_valid() {
        let mut config = config();
        config.tools.push(ToolConfig {
            id: "other".to_string(),
            label: "Other".to_string(),
            active_ptt_hold_ms: None,
            active_hooks: ActiveHooks::default(),
            tabs: vec![tab(
                "other-local",
                vec![item("other-item", "mute", Some("mute-alt"))],
            )],
        });
        let mut menu = MenuState::new(&config).unwrap();

        menu.push(&config, InputEvent::EnterControl);
        menu.push(&config, InputEvent::NextTab);
        menu.switch_tool(&config, "other").unwrap();

        assert_eq!(
            menu.push(&config, InputEvent::EnterControl),
            vec![MenuOutcome::EnteredControl {
                tab_id: "other-local".to_string(),
                item_id: Some("other-item".to_string()),
            }]
        );
    }

    #[test]
    fn control_sos_alternate_action_stays_in_control() {
        let config = config();
        let mut menu = MenuState::new(&config).unwrap();

        menu.push(&config, InputEvent::EnterControl);

        assert_eq!(
            menu.push(
                &config,
                InputEvent::SosShort {
                    mode: HardwareMode::Control,
                },
            ),
            vec![MenuOutcome::Action {
                action_id: "preview-radio".to_string(),
            }]
        );
        assert_eq!(menu.phase(), MenuPhase::Control);
    }

    #[test]
    fn active_hooks_return_actions_without_changing_phase() {
        let config = config();
        let mut menu = MenuState::new(&config).unwrap();

        assert_eq!(
            menu.push(&config, InputEvent::ActivePtt),
            vec![MenuOutcome::Action {
                action_id: "talk".to_string(),
            }]
        );
        assert_eq!(
            menu.push(
                &config,
                InputEvent::SosLong {
                    mode: HardwareMode::Active,
                },
            ),
            vec![MenuOutcome::Action {
                action_id: "panic".to_string(),
            }]
        );
        assert_eq!(menu.phase(), MenuPhase::Active);
    }

    #[test]
    fn no_idle_timeout_transition_exists() {
        let config = config();
        let mut menu = MenuState::new(&config).unwrap();

        menu.push(&config, InputEvent::EnterControl);
        assert_eq!(menu.push(&config, InputEvent::ActivePtt), Vec::new());
        assert_eq!(menu.phase(), MenuPhase::Control);
    }
}
