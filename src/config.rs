#![allow(dead_code)]

use anyhow::{bail, Context, Result};
use directories::BaseDirs;
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
};

const APP_CONFIG_DIR: &str = "ptt-audio-menu";
const CONFIG_FILE: &str = "config.toml";

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub default_tool: String,
    pub bluetooth: BluetoothConfig,
    pub voice: VoiceConfig,
    #[serde(default)]
    pub cache: CacheConfig,
    #[serde(default)]
    pub audio: AudioConfig,
    #[serde(default)]
    pub globals: GlobalDefaults,
    #[serde(default)]
    pub tools: Vec<ToolConfig>,
    #[serde(default)]
    pub global_tabs: Vec<TabConfig>,
    #[serde(default)]
    pub actions: Vec<ActionConfig>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BluetoothConfig {
    pub device: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceConfig {
    pub model_path: PathBuf,
    pub config_path: PathBuf,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheConfig {
    pub tts_dir: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioConfig {
    pub device: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlobalDefaults {
    #[serde(default = "default_active_ptt_hold_ms")]
    pub active_ptt_hold_ms: u64,
    #[serde(default)]
    pub active_ptt_trigger: ActivePttTrigger,
}

impl Default for GlobalDefaults {
    fn default() -> Self {
        Self {
            active_ptt_hold_ms: default_active_ptt_hold_ms(),
            active_ptt_trigger: ActivePttTrigger::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ActivePttTrigger {
    Press,
    HoldToggle,
    #[default]
    ReleaseAfterHold,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolConfig {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub active_ptt_hold_ms: Option<u64>,
    #[serde(default)]
    pub active_hooks: ActiveHooks,
    #[serde(default)]
    pub tabs: Vec<TabConfig>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActiveHooks {
    pub ptt: Option<String>,
    pub sos_short: Option<String>,
    pub sos_long: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TabConfig {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub items: Vec<ItemConfig>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemConfig {
    pub id: String,
    pub label: String,
    pub primary_action: String,
    #[serde(default)]
    pub alternate_action: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ActionConfig {
    Internal(InternalActionConfig),
    Command(CommandActionConfig),
    RecordingPacket(RecordingPacketActionConfig),
}

impl ActionConfig {
    pub fn id(&self) -> &str {
        match self {
            Self::Internal(action) => &action.id,
            Self::Command(action) => &action.id,
            Self::RecordingPacket(action) => &action.id,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InternalActionConfig {
    pub id: String,
    pub command: InternalCommand,
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum InternalCommand {
    SwitchTool,
    Speak,
    Noop,
    ExitControl,
    ReloadConfig,
    StopAudio,
    CancelRunningAction,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandActionConfig {
    pub id: String,
    #[serde(default)]
    pub argv: Vec<String>,
    #[serde(default)]
    pub shell: Option<String>,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub feedback: FeedbackConfig,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordingPacketActionConfig {
    pub id: String,
    pub storage_dir: PathBuf,
    pub processor: PacketProcessorConfig,
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,
    #[serde(default = "default_initial_backoff_ms")]
    pub initial_backoff_ms: u64,
    #[serde(default = "default_max_backoff_ms")]
    pub max_backoff_ms: u64,
    #[serde(default)]
    pub feedback: RecordingFeedbackConfig,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PacketProcessorConfig {
    DailyLogParakeet(DailyLogParakeetConfig),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DailyLogParakeetConfig {
    pub model_dir: PathBuf,
    pub daily_json_dir: PathBuf,
    pub html_dir: PathBuf,
    pub renderer_script: PathBuf,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedbackConfig {
    pub start: Option<String>,
    pub success: Option<String>,
    pub failure: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordingFeedbackConfig {
    pub start: Option<String>,
    pub stop: Option<String>,
    pub enqueued: Option<String>,
    pub failure: Option<String>,
}

pub fn resolve_config_path(config_arg: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = config_arg {
        return Ok(path);
    }

    let xdg_config_home = env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
    let home_dir = BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf());
    resolve_config_path_from(config_arg, xdg_config_home.as_deref(), home_dir.as_deref())
}

fn resolve_config_path_from(
    config_arg: Option<PathBuf>,
    xdg_config_home: Option<&Path>,
    home_dir: Option<&Path>,
) -> Result<PathBuf> {
    if let Some(path) = config_arg {
        return Ok(path);
    }

    if let Some(path) = xdg_config_home {
        return Ok(path.join(APP_CONFIG_DIR).join(CONFIG_FILE));
    }

    if let Some(path) = home_dir {
        return Ok(path.join(".config").join(APP_CONFIG_DIR).join(CONFIG_FILE));
    }

    bail!("could not resolve config path: pass --config or set HOME");
}

pub fn load_config(path: &Path) -> Result<Config> {
    let source =
        fs::read_to_string(path).with_context(|| format!("read config {}", path.display()))?;
    let config: Config =
        toml::from_str(&source).with_context(|| format!("parse config {}", path.display()))?;
    config.validate()?;
    Ok(config)
}

impl Config {
    pub fn validate(&self) -> Result<()> {
        validate_slug("default_tool", &self.default_tool)?;
        validate_bluetooth_address("bluetooth.device", &self.bluetooth.device)?;
        validate_existing_file("voice.model_path", &self.voice.model_path)?;
        validate_existing_file("voice.config_path", &self.voice.config_path)?;

        let tool_ids = validate_unique_ids("tool", self.tools.iter().map(|tool| tool.id.as_str()))?;
        if !tool_ids.contains(self.default_tool.as_str()) {
            bail!(
                "default_tool references unknown tool '{}'",
                self.default_tool
            );
        }

        let action_ids =
            validate_unique_ids("action", self.actions.iter().map(|action| action.id()))?;

        for action in &self.actions {
            validate_action(action, &tool_ids)?;
        }

        validate_tabs("global tab", &self.global_tabs, &action_ids)?;

        for tool in &self.tools {
            validate_tool(tool, &action_ids)?;
        }

        Ok(())
    }
}

fn validate_tool(tool: &ToolConfig, action_ids: &HashSet<&str>) -> Result<()> {
    validate_action_ref(
        "active_hooks.ptt",
        tool.active_hooks.ptt.as_deref(),
        action_ids,
    )?;
    validate_action_ref(
        "active_hooks.sos_short",
        tool.active_hooks.sos_short.as_deref(),
        action_ids,
    )?;
    validate_action_ref(
        "active_hooks.sos_long",
        tool.active_hooks.sos_long.as_deref(),
        action_ids,
    )?;
    validate_tabs(&format!("tool '{}' tab", tool.id), &tool.tabs, action_ids)
}

fn validate_tabs(scope: &str, tabs: &[TabConfig], action_ids: &HashSet<&str>) -> Result<()> {
    validate_unique_ids(scope, tabs.iter().map(|tab| tab.id.as_str()))?;

    for tab in tabs {
        validate_unique_ids(
            &format!("{scope} '{}' item", tab.id),
            tab.items.iter().map(|item| item.id.as_str()),
        )?;

        for item in &tab.items {
            validate_action_ref(
                "item.primary_action",
                Some(&item.primary_action),
                action_ids,
            )?;
            validate_action_ref(
                "item.alternate_action",
                item.alternate_action.as_deref(),
                action_ids,
            )?;
        }
    }

    Ok(())
}

fn validate_action(action: &ActionConfig, tool_ids: &HashSet<&str>) -> Result<()> {
    match action {
        ActionConfig::Internal(action) => {
            if action.command == InternalCommand::SwitchTool {
                let Some(tool) = action.tool.as_deref() else {
                    bail!("internal action '{}' switch_tool requires tool", action.id);
                };
                if !tool_ids.contains(tool) {
                    bail!(
                        "internal action '{}' switch_tool references unknown tool '{}'",
                        action.id,
                        tool
                    );
                }
            }
        }
        ActionConfig::Command(action) => {
            if action.shell.is_some() {
                bail!(
                    "command action '{}' uses shell; command actions require argv",
                    action.id
                );
            }
            if action.argv.is_empty() {
                bail!("command action '{}' requires non-empty argv", action.id);
            }
            if action.argv.iter().any(|arg| arg.is_empty()) {
                bail!(
                    "command action '{}' contains an empty argv element",
                    action.id
                );
            }
        }
        ActionConfig::RecordingPacket(action) => {
            if action.max_attempts == 0 {
                bail!("recording action '{}' requires max_attempts > 0", action.id);
            }
            if action.initial_backoff_ms == 0 {
                bail!(
                    "recording action '{}' requires initial_backoff_ms > 0",
                    action.id
                );
            }
            if action.max_backoff_ms < action.initial_backoff_ms {
                bail!(
                    "recording action '{}' requires max_backoff_ms >= initial_backoff_ms",
                    action.id
                );
            }
            match &action.processor {
                PacketProcessorConfig::DailyLogParakeet(processor) => {
                    validate_existing_dir("processor.model_dir", &processor.model_dir)?;
                    validate_existing_file(
                        "processor.renderer_script",
                        &processor.renderer_script,
                    )?;
                }
            }
        }
    }

    Ok(())
}

fn validate_action_ref(label: &str, id: Option<&str>, action_ids: &HashSet<&str>) -> Result<()> {
    let Some(id) = id else {
        return Ok(());
    };
    validate_slug(label, id)?;
    if !action_ids.contains(id) {
        bail!("{label} references unknown action '{id}'");
    }
    Ok(())
}

fn validate_unique_ids<'a>(
    namespace: &str,
    ids: impl IntoIterator<Item = &'a str>,
) -> Result<HashSet<&'a str>> {
    let mut seen = HashSet::new();
    for id in ids {
        validate_slug(namespace, id)?;
        if !seen.insert(id) {
            bail!("duplicate {namespace} id '{id}'");
        }
    }
    Ok(seen)
}

fn validate_slug(label: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        || value.starts_with('-')
        || value.ends_with('-')
        || value.contains("--")
    {
        bail!("{label} '{value}' is not a strict lowercase slug");
    }
    Ok(())
}

fn validate_bluetooth_address(label: &str, value: &str) -> Result<()> {
    value
        .parse::<bluer::Address>()
        .with_context(|| format!("{label} '{value}' is not a valid Bluetooth address"))?;
    Ok(())
}

fn validate_existing_file(label: &str, path: &Path) -> Result<()> {
    if !path.is_file() {
        bail!("{label} '{}' is not an existing file", path.display());
    }
    Ok(())
}

fn validate_existing_dir(label: &str, path: &Path) -> Result<()> {
    if !path.is_dir() {
        bail!("{label} '{}' is not an existing directory", path.display());
    }
    Ok(())
}

fn default_active_ptt_hold_ms() -> u64 {
    350
}

fn default_max_attempts() -> u32 {
    3
}

fn default_initial_backoff_ms() -> u64 {
    1_000
}

fn default_max_backoff_ms() -> u64 {
    60_000
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    struct Fixture {
        _dir: TempDir,
        model_path: PathBuf,
        voice_config_path: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = TempDir::new().expect("create tempdir");
            let model_path = dir.path().join("voice.onnx");
            let voice_config_path = dir.path().join("voice.json");
            fs::write(&model_path, "").expect("write model");
            fs::write(&voice_config_path, "{}").expect("write voice config");
            Self {
                _dir: dir,
                model_path,
                voice_config_path,
            }
        }

        fn toml(&self) -> String {
            format!(
                r#"
default_tool = "radio"

[bluetooth]
device = "00:02:5B:55:FF:01"

[voice]
model_path = "{}"
config_path = "{}"

[[tools]]
id = "radio"
label = "Radio"

[tools.active_hooks]
ptt = "talk"

[[tools.tabs]]
id = "local"
label = "Local"

[[tools.tabs.items]]
id = "mute"
label = "Mute"
primary_action = "say-muted"
alternate_action = "nothing"

[[global_tabs]]
id = "tools"
label = "Tools"

[[global_tabs.items]]
id = "radio"
label = "Radio"
primary_action = "switch-radio"

[[actions]]
id = "talk"
type = "internal"
command = "noop"

[[actions]]
id = "say-muted"
type = "internal"
command = "speak"
text = "Muted"

[[actions]]
id = "nothing"
type = "internal"
command = "noop"

[[actions]]
id = "switch-radio"
type = "internal"
command = "switch_tool"
tool = "radio"

[[actions]]
id = "run-date"
type = "command"
argv = ["date"]
"#,
                self.model_path.display(),
                self.voice_config_path.display()
            )
        }

        fn config(&self) -> Config {
            toml::from_str(&self.toml()).expect("parse fixture config")
        }
    }

    #[test]
    fn resolves_explicit_config_path_first() {
        let path = PathBuf::from("/tmp/custom.toml");
        assert_eq!(
            resolve_config_path_from(Some(path.clone()), Some(Path::new("/xdg")), None).unwrap(),
            path
        );
    }

    #[test]
    fn resolves_xdg_config_home_before_home() {
        assert_eq!(
            resolve_config_path_from(None, Some(Path::new("/xdg")), Some(Path::new("/home/me")))
                .unwrap(),
            PathBuf::from("/xdg/ptt-audio-menu/config.toml")
        );
    }

    #[test]
    fn valid_config_passes_validation() {
        Fixture::new().config().validate().unwrap();
    }

    #[test]
    fn missing_bluetooth_device_fails_to_parse() {
        let fixture = Fixture::new();
        let toml = fixture
            .toml()
            .replace("[bluetooth]\ndevice = \"00:02:5B:55:FF:01\"\n\n", "");
        let err = toml::from_str::<Config>(&toml).unwrap_err().to_string();
        assert!(err.contains("missing field `bluetooth`"));
    }

    #[test]
    fn invalid_bluetooth_device_fails_validation() {
        let fixture = Fixture::new();
        let mut config = fixture.config();
        config.bluetooth.device = "B02PTT-FF01".to_string();

        let err = config.validate().unwrap_err().to_string();
        assert!(err.contains("bluetooth.device"));
    }

    #[test]
    fn parses_active_ptt_trigger() {
        let fixture = Fixture::new();
        let toml = fixture.toml().replace(
            "[voice]",
            "[globals]\nactive_ptt_trigger = \"hold_toggle\"\n\n[voice]",
        );
        let config: Config = toml::from_str(&toml).expect("parse trigger config");

        assert_eq!(
            config.globals.active_ptt_trigger,
            ActivePttTrigger::HoldToggle
        );
        config.validate().unwrap();
    }

    #[test]
    fn missing_default_tool_fails_to_parse() {
        let fixture = Fixture::new();
        let toml = fixture.toml().replace("default_tool = \"radio\"\n", "");
        let err = toml::from_str::<Config>(&toml).unwrap_err().to_string();
        assert!(err.contains("missing field `default_tool`"));
    }

    #[test]
    fn duplicate_tool_ids_fail_validation() {
        let fixture = Fixture::new();
        let mut config = fixture.config();
        config.tools.push(config.tools[0].clone());

        let err = config.validate().unwrap_err().to_string();
        assert!(err.contains("duplicate tool id 'radio'"));
    }

    #[test]
    fn invalid_slug_ids_fail_validation() {
        let fixture = Fixture::new();
        let mut config = fixture.config();
        config.tools[0].id = "Radio".to_string();

        let err = config.validate().unwrap_err().to_string();
        assert!(err.contains("not a strict lowercase slug"));
    }

    #[test]
    fn unknown_action_references_fail_validation() {
        let fixture = Fixture::new();
        let mut config = fixture.config();
        config.tools[0].tabs[0].items[0].primary_action = "missing".to_string();

        let err = config.validate().unwrap_err().to_string();
        assert!(err.contains("unknown action 'missing'"));
    }

    #[test]
    fn invalid_piper_paths_fail_validation() {
        let fixture = Fixture::new();
        let mut config = fixture.config();
        config.voice.model_path = fixture.model_path.with_file_name("missing.onnx");

        let err = config.validate().unwrap_err().to_string();
        assert!(err.contains("voice.model_path"));
    }

    #[test]
    fn command_action_rejects_shell_string_only_config() {
        let fixture = Fixture::new();
        let toml = fixture.toml().replace(
            r#"[[actions]]
id = "run-date"
type = "command"
argv = ["date"]"#,
            r#"[[actions]]
id = "run-date"
type = "command"
shell = "date""#,
        );
        let config: Config = toml::from_str(&toml).expect("parse shell config");

        let err = config.validate().unwrap_err().to_string();
        assert!(err.contains("uses shell"));
    }
}
