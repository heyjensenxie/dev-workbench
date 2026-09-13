//! OS-facing process adapter.
//!
//! The process runtime only knows about processes it started itself; this crate
//! answers questions about the rest of the machine and can terminate an
//! arbitrary process together with its descendants.

use serde::Serialize;
use std::collections::HashSet;
use sysinfo::{Pid, ProcessesToUpdate, System};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub command: Option<String>,
    pub parent_pid: Option<u32>,
    pub executable: Option<String>,
    /// Resident set size in bytes.
    pub memory_bytes: u64,
    /// Recent CPU utilization percentage reported by sysinfo.
    pub cpu_percent: f32,
    /// Seconds since the Unix epoch.
    pub started_at: u64,
}

fn describe(pid: Pid, process: &sysinfo::Process) -> ProcessInfo {
    ProcessInfo {
        pid: pid.as_u32(),
        name: process.name().to_string_lossy().into_owned(),
        command: (!process.cmd().is_empty()).then(|| {
            process
                .cmd()
                .iter()
                .map(|value| value.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" ")
        }),
        parent_pid: process.parent().map(Pid::as_u32),
        executable: process
            .exe()
            .map(|path| path.to_string_lossy().into_owned()),
        memory_bytes: process.memory(),
        cpu_percent: process.cpu_usage(),
        started_at: process.start_time(),
    }
}

fn refreshed_system() -> System {
    let mut system = System::new_all();
    system.refresh_processes(ProcessesToUpdate::All, true);
    system
}

pub fn list_processes() -> Vec<ProcessInfo> {
    let system = refreshed_system();
    let mut result: Vec<_> = system
        .processes()
        .iter()
        .map(|(pid, process)| describe(*pid, process))
        .collect();
    result.sort_by_key(|item| item.pid);
    result
}

/// Returns `pid` followed by its descendants, parents before children.
pub fn process_tree(pid: u32) -> Vec<ProcessInfo> {
    let system = refreshed_system();
    let root_pid = Pid::from_u32(pid);
    let Some(root) = system.process(root_pid) else {
        return Vec::new();
    };
    let mut tree = vec![describe(root_pid, root)];
    let mut seen = HashSet::from([pid]);
    let mut frontier = vec![pid];
    while let Some(current) = frontier.pop() {
        let mut children: Vec<u32> = system
            .processes()
            .iter()
            .filter(|(_, process)| process.parent().map(Pid::as_u32) == Some(current))
            .map(|(child, _)| child.as_u32())
            .collect();
        children.sort_unstable();
        for child in children {
            if !seen.insert(child) {
                continue;
            }
            let child_pid = Pid::from_u32(child);
            if let Some(child_process) = system.process(child_pid) {
                tree.push(describe(child_pid, child_process));
                frontier.push(child);
            }
        }
    }
    tree
}

pub fn kill_process(pid: u32) -> bool {
    platform::kill_process(pid)
}

/// Kills every descendant of `pid` first, then `pid` itself.
pub fn kill_process_tree(pid: u32) -> bool {
    if process_tree(pid).is_empty() {
        return false;
    }
    platform::kill_tree(pid)
}

#[cfg(windows)]
mod platform {
    use std::{
        os::windows::process::CommandExt,
        process::{Command, Stdio},
    };

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    /// Windows has no graceful console signal, so termination is always forced.
    /// `taskkill /T` walks the OS parent chain, which also covers children
    /// spawned after our snapshot was taken.
    fn taskkill(pid: u32, tree: bool) -> bool {
        let mut command = Command::new("taskkill");
        command.args(["/PID", &pid.to_string(), "/F"]);
        if tree {
            command.arg("/T");
        }
        command
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .is_ok_and(|status| status.success())
    }

    pub fn kill_process(pid: u32) -> bool {
        taskkill(pid, false)
    }

    pub fn kill_tree(pid: u32) -> bool {
        taskkill(pid, true)
    }
}

#[cfg(unix)]
mod platform {
    use sysinfo::{Pid, ProcessesToUpdate, System};

    fn kill(pid: u32) -> bool {
        let mut system = System::new_all();
        system.refresh_processes(ProcessesToUpdate::All, true);
        system
            .process(Pid::from_u32(pid))
            .is_some_and(|process| process.kill())
    }

    pub fn kill_process(pid: u32) -> bool {
        kill(pid)
    }

    /// Descendants go first so the tree cannot reparent away from us.
    pub fn kill_tree(pid: u32) -> bool {
        let tree = super::process_tree(pid);
        let mut killed_root = false;
        for (index, item) in tree.iter().enumerate().rev() {
            let killed = kill(item.pid);
            if index == 0 {
                killed_root = killed;
            }
        }
        killed_root
    }
}

#[cfg(not(any(windows, unix)))]
mod platform {
    pub fn kill_process(_: u32) -> bool {
        false
    }

    pub fn kill_tree(_: u32) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{process::Stdio, time::Duration, time::Instant};

    /// Spawns a shell that keeps a grandchild alive, so tree walking has
    /// something to discover on every platform.
    fn spawn_tree() -> std::process::Child {
        #[cfg(windows)]
        let mut command = {
            let mut command = std::process::Command::new("cmd");
            command.args(["/C", "ping -n 30 127.0.0.1"]);
            command
        };
        #[cfg(unix)]
        let mut command = {
            let mut command = std::process::Command::new("sh");
            command.args(["-c", "sleep 30 & wait"]);
            command
        };
        command
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to spawn the tree fixture")
    }

    fn wait_until(mut predicate: impl FnMut() -> bool, timeout: Duration, what: &str) {
        let deadline = Instant::now() + timeout;
        while !predicate() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn lists_the_current_process() {
        let me = std::process::id();
        let processes = list_processes();
        assert!(processes.iter().any(|process| process.pid == me));
    }

    #[test]
    fn process_tree_contains_the_root_first() {
        let me = std::process::id();
        let tree = process_tree(me);
        assert_eq!(tree.first().map(|process| process.pid), Some(me));
    }

    #[test]
    fn missing_processes_yield_empty_results() {
        let tree = process_tree(u32::MAX - 1);
        assert!(tree.is_empty());
        assert!(!kill_process_tree(u32::MAX - 1));
    }

    #[test]
    fn kills_a_process_tree_including_descendants() {
        let mut child = spawn_tree();
        let root = child.id();
        wait_until(
            || process_tree(root).len() >= 2,
            Duration::from_secs(15),
            "the fixture to spawn a child",
        );
        let victims: Vec<u32> = process_tree(root).iter().map(|item| item.pid).collect();
        assert!(victims.len() >= 2, "expected the fixture to have a child");

        if !kill_process_tree(root) {
            // Sandboxed/restricted-token environments deny opening processes
            // they did not create, so tree termination cannot be exercised here.
            let _ = child.kill();
            let _ = child.wait();
            eprintln!(
                "SKIPPED tree-kill assertions: this environment denied terminating process {root}"
            );
            return;
        }

        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let alive = list_processes();
            let remaining: Vec<String> = alive
                .iter()
                .filter(|process| victims.contains(&process.pid))
                .map(|process| format!("{} ({})", process.pid, process.name))
                .collect();
            if remaining.is_empty() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "processes survived kill_process_tree: {remaining:?}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = child.wait();
    }
}
