use crate::{
    config::{
        ActionConfig, CommandActionConfig, Config, InternalCommand, RecordingPacketActionConfig,
    },
    menu::{MenuState, RecordingPacketEvent},
};
use anyhow::{bail, Result};
use std::{collections::HashMap, path::PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandRequest {
    pub action_id: String,
    pub argv: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: HashMap<String, String>,
    pub timeout_ms: Option<u64>,
    pub feedback: CommandFeedback,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CommandFeedback {
    pub start: Option<String>,
    pub success: Option<String>,
    pub failure: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordingPacketRequest {
    pub action_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ActionEffect {
    Noop {
        action_id: String,
    },
    SwitchedTool {
        action_id: String,
        tool_id: String,
    },
    ExitedControl {
        action_id: String,
    },
    DeferredInternal {
        action_id: String,
        command: InternalCommand,
    },
    CommandQueued {
        command: CommandRequest,
    },
    RecordingPacket {
        request: RecordingPacketRequest,
        event: RecordingPacketEvent,
    },
}

#[derive(Debug)]
pub struct ActionDispatcher {
    action_indexes: HashMap<String, usize>,
}

impl ActionDispatcher {
    pub fn new(config: &Config) -> Result<Self> {
        let mut action_indexes = HashMap::new();
        for (index, action) in config.actions.iter().enumerate() {
            if action_indexes
                .insert(action.id().to_string(), index)
                .is_some()
            {
                bail!("duplicate action id '{}'", action.id());
            }
        }

        Ok(Self { action_indexes })
    }

    pub fn dispatch(
        &self,
        config: &Config,
        menu: &mut MenuState,
        action_id: &str,
    ) -> Result<ActionEffect> {
        let Some(action_index) = self.action_indexes.get(action_id) else {
            bail!("unknown action '{action_id}'");
        };

        match &config.actions[*action_index] {
            ActionConfig::Internal(action) => match action.command {
                InternalCommand::Noop => Ok(ActionEffect::Noop {
                    action_id: action.id.clone(),
                }),
                InternalCommand::SwitchTool => {
                    let Some(tool_id) = action.tool.as_deref() else {
                        bail!("switch_tool action '{}' has no tool", action.id);
                    };
                    menu.switch_tool(config, tool_id)?;
                    Ok(ActionEffect::SwitchedTool {
                        action_id: action.id.clone(),
                        tool_id: tool_id.to_string(),
                    })
                }
                InternalCommand::ExitControl => {
                    menu.exit_control();
                    Ok(ActionEffect::ExitedControl {
                        action_id: action.id.clone(),
                    })
                }
                command => Ok(ActionEffect::DeferredInternal {
                    action_id: action.id.clone(),
                    command,
                }),
            },
            ActionConfig::Command(action) => Ok(command_queued_effect(action)),
            ActionConfig::RecordingPacket(_) => {
                bail!("recording action '{action_id}' requires a PTT edge")
            }
        }
    }

    pub fn dispatch_recording_packet(
        &self,
        config: &Config,
        action_id: &str,
        event: RecordingPacketEvent,
    ) -> Result<ActionEffect> {
        let Some(action_index) = self.action_indexes.get(action_id) else {
            bail!("unknown action '{action_id}'");
        };

        match &config.actions[*action_index] {
            ActionConfig::RecordingPacket(action) => Ok(recording_packet_effect(action, event)),
            _ => bail!("action '{action_id}' is not a recording_packet action"),
        }
    }
}

fn command_queued_effect(action: &CommandActionConfig) -> ActionEffect {
    ActionEffect::CommandQueued {
        command: CommandRequest {
            action_id: action.id.clone(),
            argv: action.argv.clone(),
            cwd: action.cwd.clone(),
            env: action.env.clone(),
            timeout_ms: action.timeout_ms,
            feedback: CommandFeedback {
                start: action.feedback.start.clone(),
                success: action.feedback.success.clone(),
                failure: action.feedback.failure.clone(),
            },
        },
    }
}

fn recording_packet_effect(
    action: &RecordingPacketActionConfig,
    event: RecordingPacketEvent,
) -> ActionEffect {
    ActionEffect::RecordingPacket {
        request: RecordingPacketRequest {
            action_id: action.id.clone(),
        },
        event,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        ActiveHooks, AudioConfig, BluetoothConfig, CacheConfig, CommandActionConfig,
        FeedbackConfig, GlobalDefaults, InternalActionConfig, ItemConfig, TabConfig, ToolConfig,
        VoiceConfig,
    };
    use crate::input::InputEvent;
    use crate::menu::{MenuOutcome, MenuPhase};
    use std::{collections::HashMap, path::PathBuf};

    fn internal(id: &str, command: InternalCommand, tool: Option<&str>) -> ActionConfig {
        ActionConfig::Internal(InternalActionConfig {
            id: id.to_string(),
            command,
            tool: tool.map(str::to_string),
            text: None,
        })
    }

    fn command(id: &str, argv: &[&str]) -> ActionConfig {
        ActionConfig::Command(CommandActionConfig {
            id: id.to_string(),
            argv: argv.iter().map(|arg| arg.to_string()).collect(),
            shell: None,
            cwd: None,
            env: HashMap::new(),
            timeout_ms: None,
            feedback: FeedbackConfig::default(),
        })
    }

    fn config() -> Config {
        Config {
            default_tool: "radio".to_string(),
            bluetooth: BluetoothConfig {
                device: "00:02:5B:55:FF:01".to_string(),
            },
            voice: VoiceConfig {
                model_path: PathBuf::from("voice.onnx"),
                config_path: PathBuf::from("voice.json"),
            },
            cache: CacheConfig::default(),
            audio: AudioConfig::default(),
            globals: GlobalDefaults::default(),
            tools: vec![
                ToolConfig {
                    id: "radio".to_string(),
                    label: "Radio".to_string(),
                    active_ptt_hold_ms: None,
                    active_hooks: ActiveHooks::default(),
                    tabs: vec![],
                },
                ToolConfig {
                    id: "music".to_string(),
                    label: "Music".to_string(),
                    active_ptt_hold_ms: None,
                    active_hooks: ActiveHooks::default(),
                    tabs: vec![],
                },
            ],
            global_tabs: vec![TabConfig {
                id: "tools".to_string(),
                label: "Tools".to_string(),
                items: vec![ItemConfig {
                    id: "music".to_string(),
                    label: "Music".to_string(),
                    primary_action: "switch-music".to_string(),
                    alternate_action: None,
                }],
            }],
            actions: vec![
                internal("noop", InternalCommand::Noop, None),
                internal("switch-music", InternalCommand::SwitchTool, Some("music")),
                internal("exit", InternalCommand::ExitControl, None),
                internal("speak", InternalCommand::Speak, None),
                command("run-date", &["date"]),
            ],
        }
    }

    #[test]
    fn action_lookup_rejects_unknown_action() {
        let config = config();
        let dispatcher = ActionDispatcher::new(&config).unwrap();
        let mut menu = MenuState::new(&config).unwrap();

        let err = dispatcher
            .dispatch(&config, &mut menu, "missing")
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown action 'missing'"));
    }

    #[test]
    fn noop_dispatches_without_state_change() {
        let config = config();
        let dispatcher = ActionDispatcher::new(&config).unwrap();
        let mut menu = MenuState::new(&config).unwrap();

        assert_eq!(
            dispatcher.dispatch(&config, &mut menu, "noop").unwrap(),
            ActionEffect::Noop {
                action_id: "noop".to_string()
            }
        );
        assert_eq!(menu.active_tool(&config).id, "radio");
        assert_eq!(menu.phase(), MenuPhase::Active);
    }

    #[test]
    fn switch_tool_mutates_menu_through_menu_state() {
        let config = config();
        let dispatcher = ActionDispatcher::new(&config).unwrap();
        let mut menu = MenuState::new(&config).unwrap();

        assert_eq!(
            dispatcher
                .dispatch(&config, &mut menu, "switch-music")
                .unwrap(),
            ActionEffect::SwitchedTool {
                action_id: "switch-music".to_string(),
                tool_id: "music".to_string(),
            }
        );
        assert_eq!(menu.active_tool(&config).id, "music");
        assert_eq!(menu.phase(), MenuPhase::Active);
    }

    #[test]
    fn exit_control_mutates_menu_through_menu_state() {
        let config = config();
        let dispatcher = ActionDispatcher::new(&config).unwrap();
        let mut menu = MenuState::new(&config).unwrap();

        assert_eq!(
            menu.push(&config, InputEvent::EnterControl),
            vec![MenuOutcome::EnteredControl {
                tab_id: "tools".to_string(),
                item_id: Some("music".to_string()),
            }]
        );
        assert_eq!(menu.phase(), MenuPhase::Control);

        assert_eq!(
            dispatcher.dispatch(&config, &mut menu, "exit").unwrap(),
            ActionEffect::ExitedControl {
                action_id: "exit".to_string()
            }
        );
        assert_eq!(menu.phase(), MenuPhase::Active);
    }

    #[test]
    fn command_actions_are_recognized_but_not_executed() {
        let config = config();
        let dispatcher = ActionDispatcher::new(&config).unwrap();
        let mut menu = MenuState::new(&config).unwrap();

        assert_eq!(
            dispatcher.dispatch(&config, &mut menu, "run-date").unwrap(),
            ActionEffect::CommandQueued {
                command: CommandRequest {
                    action_id: "run-date".to_string(),
                    argv: vec!["date".to_string()],
                    cwd: None,
                    env: HashMap::new(),
                    timeout_ms: None,
                    feedback: CommandFeedback::default(),
                },
            }
        );
    }

    #[test]
    fn command_actions_carry_feedback_labels() {
        let mut config = config();
        let ActionConfig::Command(action) = config.actions.last_mut().unwrap() else {
            panic!("last action should be command");
        };
        action.feedback = FeedbackConfig {
            start: Some("Starting".to_string()),
            success: Some("Done".to_string()),
            failure: Some("Failed".to_string()),
        };
        let dispatcher = ActionDispatcher::new(&config).unwrap();
        let mut menu = MenuState::new(&config).unwrap();

        assert_eq!(
            dispatcher.dispatch(&config, &mut menu, "run-date").unwrap(),
            ActionEffect::CommandQueued {
                command: CommandRequest {
                    action_id: "run-date".to_string(),
                    argv: vec!["date".to_string()],
                    cwd: None,
                    env: HashMap::new(),
                    timeout_ms: None,
                    feedback: CommandFeedback {
                        start: Some("Starting".to_string()),
                        success: Some("Done".to_string()),
                        failure: Some("Failed".to_string()),
                    },
                },
            }
        );
    }

    #[test]
    fn unsupported_internal_actions_are_deferred() {
        let config = config();
        let dispatcher = ActionDispatcher::new(&config).unwrap();
        let mut menu = MenuState::new(&config).unwrap();

        assert_eq!(
            dispatcher.dispatch(&config, &mut menu, "speak").unwrap(),
            ActionEffect::DeferredInternal {
                action_id: "speak".to_string(),
                command: InternalCommand::Speak,
            }
        );
    }
}
