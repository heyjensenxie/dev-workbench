//! Managed child-process runtime.
//!
//! The runtime owns every process it starts, tracks its process id, streams
//! line-oriented output to the caller, and terminates the whole process tree
//! (a service usually spawns compiler, bundler, or watcher children) on stop.

use serde::{Deserialize, Serialize};
use std::{collections::HashMap, process::Stdio, sync::Arc, time::Duration};
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::Mutex,
    time::timeout,
};

/// Grace period granted to a service tree before it is force-killed.
pub const DEFAULT_STOP_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Error)]
pub enum ProcessError {
    #[error("service is already running: {0}")]
    AlreadyRunning(String),
    #[error("service is not running: {0}")]
    NotRunning(String),
    #[error("failed to spawn process: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("spawned process did not expose a process id")]
    MissingPid,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessSpec {
    pub id: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub cwd: Option<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessLog {
    pub service_id: String,
    pub stream: String,
    pub line: String,
    pub timestamp: u64,
}

/// A service process currently owned by the runtime.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunningProcess {
    pub service_id: String,
    pub pid: u32,
}

struct ManagedChild {
    pid: u32,
    child: Child,
    /// Log readers; aborted on stop so a surviving grandchild holding the
    /// inherited pipe handles cannot keep them alive.
    readers: Vec<tokio::task::JoinHandle<()>>,
}

impl ManagedChild {
    /// Drops the log readers without waiting for the pipes to reach EOF.
    fn stop_readers(&mut self) {
        for reader in self.readers.drain(..) {
            reader.abort();
        }
    }
}

#[derive(Default)]
pub struct ProcessRuntime {
    children: Mutex<HashMap<String, ManagedChild>>,
}

impl ProcessRuntime {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Spawns `spec` and streams its stdout/stderr through `emit`.
    pub async fn start<F>(&self, spec: ProcessSpec, emit: F) -> Result<u32, ProcessError>
    where
        F: Fn(ProcessLog) + Send + Sync + Clone + 'static,
    {
        let mut children = self.children.lock().await;
        if let Some(existing) = children.get_mut(&spec.id)
            && existing.child.try_wait()?.is_none()
        {
            return Err(ProcessError::AlreadyRunning(spec.id));
        }
        if let Some(mut stale) = children.remove(&spec.id) {
            stale.stop_readers();
        }
        let mut command = Command::new(&spec.command);
        command
            .args(&spec.args)
            .envs(&spec.env)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(cwd) = &spec.cwd {
            command.current_dir(cwd);
        }
        platform::configure(&mut command);
        let mut child = command.spawn()?;
        let pid = child.id().ok_or(ProcessError::MissingPid)?;
        let mut readers = Vec::new();
        if let Some(stdout) = child.stdout.take() {
            readers.push(stream_lines(
                spec.id.clone(),
                "stdout",
                stdout,
                emit.clone(),
            ));
        }
        if let Some(stderr) = child.stderr.take() {
            readers.push(stream_lines(spec.id.clone(), "stderr", stderr, emit));
        }
        children.insert(
            spec.id,
            ManagedChild {
                pid,
                child,
                readers,
            },
        );
        Ok(pid)
    }

    /// Terminates the process tree owned by `id`.
    pub async fn stop(&self, id: &str) -> Result<(), ProcessError> {
        let mut managed = self
            .children
            .lock()
            .await
            .remove(id)
            .ok_or_else(|| ProcessError::NotRunning(id.to_owned()))?;
        terminate(&mut managed, DEFAULT_STOP_TIMEOUT).await;
        Ok(())
    }

    /// Terminates every owned process tree and reports how many were stopped.
    pub async fn stop_all(&self) -> usize {
        let drained: Vec<ManagedChild> = {
            let mut children = self.children.lock().await;
            std::mem::take(&mut *children).into_values().collect()
        };
        let stopped = drained.len();
        for mut managed in drained {
            terminate(&mut managed, DEFAULT_STOP_TIMEOUT).await;
        }
        stopped
    }

    /// Lists live services, reaping children that exited on their own.
    pub async fn running(&self) -> Vec<RunningProcess> {
        let mut children = self.children.lock().await;
        let mut alive = Vec::with_capacity(children.len());
        let mut finished = Vec::new();
        for (id, managed) in children.iter_mut() {
            match managed.child.try_wait() {
                Ok(Some(_)) => finished.push(id.clone()),
                _ => alive.push(RunningProcess {
                    service_id: id.clone(),
                    pid: managed.pid,
                }),
            }
        }
        for id in finished {
            if let Some(mut managed) = children.remove(&id) {
                managed.stop_readers();
            }
        }
        drop(children);
        alive.sort_by(|left, right| left.service_id.cmp(&right.service_id));
        alive
    }

    pub async fn is_running(&self, id: &str) -> bool {
        let mut children = self.children.lock().await;
        match children.get_mut(id) {
            Some(managed) => match managed.child.try_wait() {
                Ok(Some(_)) => {
                    if let Some(mut exited) = children.remove(id) {
                        exited.stop_readers();
                    }
                    false
                }
                _ => true,
            },
            None => false,
        }
    }
}

/// Signals the whole tree, waits for `grace`, then force-kills what is left.
async fn terminate(managed: &mut ManagedChild, grace: Duration) {
    // Sweep the tree while the parent/child links are still intact; killing the
    // root first would orphan its children.
    if !platform::signal_tree(managed.pid, false) {
        // Hardened environments can deny cross-process termination. We always
        // own the child handle, so the service itself can still be stopped.
        let _ = managed.child.start_kill();
    }
    if timeout(grace, managed.child.wait()).await.is_err() {
        platform::signal_tree(managed.pid, true);
        let _ = managed.child.start_kill();
        let _ = managed.child.wait().await;
    }
    // A grandchild that outlived its parent keeps the inherited pipe handles
    // open, so the readers must be stopped rather than awaited.
    managed.stop_readers();
}

fn stream_lines<R, F>(
    service_id: String,
    stream: &'static str,
    reader: R,
    emit: F,
) -> tokio::task::JoinHandle<()>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
    F: Fn(ProcessLog) + Send + Sync + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            emit(ProcessLog {
                service_id: service_id.clone(),
                stream: stream.into(),
                line,
                timestamp,
            });
        }
    })
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::{os::windows::process::CommandExt, process::Command as StdCommand};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub fn configure(command: &mut Command) {
        command.creation_flags(CREATE_NO_WINDOW);
    }

    /// `taskkill /T` walks the parent/child chain recorded by the OS, which is
    /// the only reliable way to reach grandchildren of a Windows service. There
    /// is no graceful console signal to send first, so termination is forced.
    pub fn signal_tree(pid: u32, _force: bool) -> bool {
        StdCommand::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .is_ok_and(|status| status.success())
    }
}

#[cfg(unix)]
mod platform {
    use super::*;
    use std::process::Command as StdCommand;

    pub fn configure(command: &mut Command) {
        // Detach each service into its own process group so the entire tree can
        // be signalled with a single negative pid.
        command.process_group(0);
    }

    pub fn signal_tree(pid: u32, force: bool) -> bool {
        let signal = if force { "-KILL" } else { "-TERM" };
        StdCommand::new("kill")
            .args([signal, &format!("-{pid}")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }
}

#[cfg(not(any(windows, unix)))]
mod platform {
    use super::*;

    pub fn configure(_: &mut Command) {}

    pub fn signal_tree(_: u32, _: bool) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sleeper(id: &str) -> ProcessSpec {
        // Deliberately a direct child: the runtime is responsible for the
        // process it spawned, while tree sweeping is covered by the
        // `workbench-system` adapter tests.
        #[cfg(windows)]
        let (command, args) = (
            "ping".to_owned(),
            vec!["-n".to_owned(), "30".to_owned(), "127.0.0.1".to_owned()],
        );
        #[cfg(unix)]
        let (command, args) = ("sleep".to_owned(), vec!["30".to_owned()]);
        ProcessSpec {
            id: id.to_owned(),
            command,
            args,
            cwd: None,
            env: HashMap::new(),
        }
    }

    fn echoer(id: &str) -> ProcessSpec {
        #[cfg(windows)]
        let (command, args) = (
            "cmd".to_owned(),
            vec!["/C".to_owned(), "echo dev-workbench".to_owned()],
        );
        #[cfg(unix)]
        let (command, args) = ("echo".to_owned(), vec!["dev-workbench".to_owned()]);
        ProcessSpec {
            id: id.to_owned(),
            command,
            args,
            cwd: None,
            env: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn starts_tracks_and_stops_a_service() {
        let runtime = ProcessRuntime::new();
        let pid = runtime.start(sleeper("api"), |_| {}).await.unwrap();
        assert!(pid > 0);
        assert!(runtime.is_running("api").await);
        assert_eq!(runtime.running().await.len(), 1);

        runtime.stop("api").await.unwrap();
        assert!(!runtime.is_running("api").await);
        assert!(runtime.running().await.is_empty());
        assert!(matches!(
            runtime.stop("api").await,
            Err(ProcessError::NotRunning(id)) if id == "api"
        ));
    }

    #[tokio::test]
    async fn rejects_a_second_start_of_the_same_service() {
        let runtime = ProcessRuntime::new();
        runtime.start(sleeper("api"), |_| {}).await.unwrap();
        assert!(matches!(
            runtime.start(sleeper("api"), |_| {}).await,
            Err(ProcessError::AlreadyRunning(id)) if id == "api"
        ));
        assert_eq!(runtime.stop_all().await, 1);
    }

    #[tokio::test]
    async fn stop_all_terminates_every_owned_tree() {
        let runtime = ProcessRuntime::new();
        runtime.start(sleeper("api"), |_| {}).await.unwrap();
        runtime.start(sleeper("web"), |_| {}).await.unwrap();
        assert_eq!(runtime.running().await.len(), 2);

        assert_eq!(runtime.stop_all().await, 2);
        assert!(runtime.running().await.is_empty());
        assert_eq!(runtime.stop_all().await, 0);
    }

    #[tokio::test]
    async fn reports_a_finished_service_as_not_running() {
        let runtime = ProcessRuntime::new();
        runtime.start(echoer("once"), |_| {}).await.unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while runtime.is_running("once").await {
            assert!(
                std::time::Instant::now() < deadline,
                "echo process never exited"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    #[tokio::test]
    async fn streams_stdout_lines_to_the_emitter() {
        let runtime = ProcessRuntime::new();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        runtime
            .start(echoer("once"), move |log| {
                let _ = sender.send(log);
            })
            .await
            .unwrap();

        let log = timeout(Duration::from_secs(10), receiver.recv())
            .await
            .expect("timed out waiting for a log line")
            .expect("log channel closed");
        assert_eq!(log.service_id, "once");
        assert_eq!(log.stream, "stdout");
        assert_eq!(log.line.trim(), "dev-workbench");
    }
}
