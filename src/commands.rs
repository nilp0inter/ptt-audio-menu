use crate::actions::CommandRequest;
use anyhow::{bail, Context, Result};
use std::{process::ExitStatus, sync::Arc, time::Duration};
use tokio::{process::Command, sync::Mutex, time};

#[derive(Clone, Debug)]
pub struct CommandRunner {
    serial: Arc<Mutex<()>>,
    running: Arc<Mutex<Option<RunningCommand>>>,
}

#[derive(Clone, Debug)]
struct RunningCommand {
    action_id: String,
    process_group_id: i32,
    cancel_requested: bool,
}

#[derive(Clone, Debug)]
pub struct CommandExecution {
    pub action_id: String,
    pub outcome: CommandOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandOutcome {
    Success,
    Failed { code: Option<i32> },
    TimedOut,
    Cancelled,
}

impl CommandRunner {
    pub fn new() -> Self {
        Self {
            serial: Arc::new(Mutex::new(())),
            running: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn run(&self, request: CommandRequest) -> Result<CommandExecution> {
        let _serial = self.serial.lock().await;
        let mut child = spawn_command(&request)?;
        let child_id = child
            .id()
            .with_context(|| format!("spawn command action '{}'", request.action_id))?;
        let process_group_id = child_id as i32;

        {
            let mut running = self.running.lock().await;
            *running = Some(RunningCommand {
                action_id: request.action_id.clone(),
                process_group_id,
                cancel_requested: false,
            });
        }

        let outcome = if let Some(timeout_ms) = request.timeout_ms {
            match time::timeout(Duration::from_millis(timeout_ms), child.wait()).await {
                Ok(status) => status_outcome(status?),
                Err(_) => {
                    kill_process_group(process_group_id)?;
                    let _ = child.wait().await;
                    CommandOutcome::TimedOut
                }
            }
        } else {
            status_outcome(child.wait().await?)
        };

        let cancel_requested = {
            let mut running = self.running.lock().await;
            let cancel_requested = running
                .as_ref()
                .filter(|command| command.action_id == request.action_id)
                .map(|command| command.cancel_requested)
                .unwrap_or(false);
            *running = None;
            cancel_requested
        };

        let outcome = if cancel_requested {
            CommandOutcome::Cancelled
        } else {
            outcome
        };

        Ok(CommandExecution {
            action_id: request.action_id,
            outcome,
        })
    }

    pub async fn cancel_running(&self) -> Result<bool> {
        let mut running = self.running.lock().await;
        let Some(command) = running.as_mut() else {
            return Ok(false);
        };

        command.cancel_requested = true;
        kill_process_group(command.process_group_id)?;
        Ok(true)
    }
}

fn spawn_command(request: &CommandRequest) -> Result<tokio::process::Child> {
    let Some(program) = request.argv.first() else {
        bail!("command action '{}' has empty argv", request.action_id);
    };

    let mut command = Command::new(program);
    command.args(&request.argv[1..]);
    if let Some(cwd) = &request.cwd {
        command.current_dir(cwd);
    }
    command.envs(&request.env);

    #[cfg(unix)]
    {
        command.process_group(0);
    }

    command
        .spawn()
        .with_context(|| format!("spawn command action '{}'", request.action_id))
}

fn status_outcome(status: ExitStatus) -> CommandOutcome {
    if status.success() {
        CommandOutcome::Success
    } else {
        CommandOutcome::Failed {
            code: status.code(),
        }
    }
}

#[cfg(unix)]
fn kill_process_group(process_group_id: i32) -> Result<()> {
    let result = unsafe { libc::kill(-process_group_id, libc::SIGTERM) };
    if result == -1 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() != Some(libc::ESRCH) {
            return Err(err).context("terminate command process group");
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn kill_process_group(_process_group_id: i32) -> Result<()> {
    bail!("command process-group cancellation is only supported on Unix");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::HashMap,
        fs,
        path::{Path, PathBuf},
    };
    use tempfile::TempDir;
    use tokio::time::Instant;

    fn request(id: &str, argv: &[&str]) -> CommandRequest {
        CommandRequest {
            action_id: id.to_string(),
            argv: argv.iter().map(|arg| arg.to_string()).collect(),
            cwd: None,
            env: HashMap::new(),
            timeout_ms: None,
        }
    }

    fn shell_request(id: &str, script: &str) -> CommandRequest {
        request(id, &["/bin/sh", "-c", script])
    }

    fn display(path: &Path) -> String {
        path.display().to_string()
    }

    #[tokio::test]
    async fn command_runner_runs_commands_serially() {
        let dir = TempDir::new().unwrap();
        let log = dir.path().join("log");
        let runner = CommandRunner::new();
        let first = shell_request(
            "first",
            &format!(
                "printf 'first-start\n' >> {}; sleep 0.2; printf 'first-end\n' >> {}",
                display(&log),
                display(&log)
            ),
        );
        let second = shell_request("second", &format!("printf 'second\n' >> {}", display(&log)));

        let first_task = tokio::spawn({
            let runner = runner.clone();
            async move { runner.run(first).await.unwrap() }
        });
        time::sleep(Duration::from_millis(30)).await;
        let second_task = tokio::spawn({
            let runner = runner.clone();
            async move { runner.run(second).await.unwrap() }
        });

        assert_eq!(first_task.await.unwrap().outcome, CommandOutcome::Success);
        assert_eq!(second_task.await.unwrap().outcome, CommandOutcome::Success);
        assert_eq!(
            fs::read_to_string(log).unwrap(),
            "first-start\nfirst-end\nsecond\n"
        );
    }

    #[tokio::test]
    async fn command_runner_applies_timeout() {
        let runner = CommandRunner::new();
        let mut request = shell_request("slow", "sleep 5");
        request.timeout_ms = Some(50);

        let started = Instant::now();
        let execution = runner.run(request).await.unwrap();

        assert_eq!(execution.action_id, "slow");
        assert_eq!(execution.outcome, CommandOutcome::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[tokio::test]
    async fn command_runner_cancels_running_command_group() {
        let dir = TempDir::new().unwrap();
        let marker = dir.path().join("marker");
        let runner = CommandRunner::new();
        let command = shell_request(
            "cancel-me",
            &format!(
                "trap 'exit 7' TERM; while true; do date >> {}; sleep 1; done",
                display(&marker)
            ),
        );

        let task = tokio::spawn({
            let runner = runner.clone();
            async move { runner.run(command).await.unwrap() }
        });

        wait_for_file(&marker).await;
        assert!(runner.cancel_running().await.unwrap());
        assert_eq!(task.await.unwrap().outcome, CommandOutcome::Cancelled);
    }

    #[tokio::test]
    async fn command_runner_uses_cwd_and_env() {
        let dir = TempDir::new().unwrap();
        let out = dir.path().join("out");
        let mut request = shell_request("env", "printf '%s:%s' \"$PWD\" \"$VALUE\" > out");
        request.cwd = Some(PathBuf::from(dir.path()));
        request.env.insert("VALUE".to_string(), "ok".to_string());

        let execution = CommandRunner::new().run(request).await.unwrap();

        assert_eq!(execution.outcome, CommandOutcome::Success);
        assert_eq!(
            fs::read_to_string(out).unwrap(),
            format!("{}:ok", dir.path().display())
        );
    }

    async fn wait_for_file(path: &Path) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if path.exists() {
                return;
            }
            time::sleep(Duration::from_millis(20)).await;
        }
        panic!("timed out waiting for {}", path.display());
    }
}
