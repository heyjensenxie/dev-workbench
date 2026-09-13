//! OS-facing listening-port adapter with process-name resolution.

use serde::Serialize;
use std::process::Command;
use sysinfo::{Pid, ProcessesToUpdate, System};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum NetworkError {
    #[error("failed to inspect ports: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortInfo {
    pub port: u16,
    pub pid: Option<u32>,
    pub process_name: Option<String>,
    pub address: String,
}

pub fn list_ports() -> Result<Vec<PortInfo>, NetworkError> {
    let mut ports = platform::list_ports()?;
    attach_process_names(&mut ports);
    Ok(ports)
}

/// Fills in process names for entries the platform adapter could not resolve.
fn attach_process_names(ports: &mut [PortInfo]) {
    if !ports
        .iter()
        .any(|item| item.process_name.is_none() && item.pid.is_some())
    {
        return;
    }
    let mut system = System::new_all();
    system.refresh_processes(ProcessesToUpdate::All, true);
    for item in ports.iter_mut() {
        if item.process_name.is_some() {
            continue;
        }
        if let Some(pid) = item.pid {
            item.process_name = system
                .process(Pid::from_u32(pid))
                .map(|process| process.name().to_string_lossy().into_owned());
        }
    }
}

/// Parses `netstat -ano -p tcp` output into listening ports.
#[cfg(any(windows, test))]
pub(crate) fn parse_netstat(text: &str) -> Vec<PortInfo> {
    let mut ports = Vec::new();
    for line in text.lines() {
        let columns: Vec<&str> = line.split_whitespace().collect();
        if columns.len() < 5
            || !columns[0].eq_ignore_ascii_case("TCP")
            || !columns[3].eq_ignore_ascii_case("LISTENING")
        {
            continue;
        }
        let address = columns[1].to_owned();
        let Some(port) = address
            .rsplit(':')
            .next()
            .and_then(|value| value.parse().ok())
        else {
            continue;
        };
        ports.push(PortInfo {
            port,
            pid: columns[4].parse().ok(),
            process_name: None,
            address,
        });
    }
    ports.sort_by_key(|item| item.port);
    ports.dedup_by_key(|item| (item.port, item.pid));
    ports
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub fn list_ports() -> Result<Vec<PortInfo>, NetworkError> {
        let output = Command::new("netstat")
            .args(["-ano", "-p", "tcp"])
            // `netstat` is a console executable. The desktop app is a GUI
            // process, so Windows would otherwise create a visible console
            // window for every port refresh.
            .creation_flags(CREATE_NO_WINDOW)
            .output()?;
        Ok(parse_netstat(&String::from_utf8_lossy(&output.stdout)))
    }
}

#[cfg(not(windows))]
mod platform {
    use super::*;

    pub fn list_ports() -> Result<Vec<PortInfo>, NetworkError> {
        let output = Command::new("lsof")
            .args(["-nP", "-iTCP", "-sTCP:LISTEN", "-Fpcn"])
            .output()?;
        let mut result = Vec::new();
        let mut pid = None;
        let mut name = None;
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            match line.chars().next() {
                Some('p') => pid = line[1..].parse().ok(),
                Some('c') => name = Some(line[1..].to_owned()),
                Some('n') => {
                    if let Some(port) = line.rsplit(':').next().and_then(|value| value.parse().ok())
                    {
                        result.push(PortInfo {
                            port,
                            pid,
                            process_name: name.clone(),
                            address: line[1..].to_owned(),
                        });
                    }
                }
                _ => {}
            }
        }
        result.sort_by_key(|item| item.port);
        result.dedup_by_key(|item| (item.port, item.pid));
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NETSTAT: &str = "\
Active Connections

  Proto  Local Address          Foreign Address        State           PID
  TCP    0.0.0.0:135            0.0.0.0:0              LISTENING       1188
  TCP    127.0.0.1:5173         0.0.0.0:0              LISTENING       4242
  TCP    [::]:135               [::]:0                 LISTENING       1188
  TCP    127.0.0.1:5173         127.0.0.1:52000        ESTABLISHED     4242
  TCP    127.0.0.1:8080         0.0.0.0:0              LISTENING       4242
";

    #[test]
    fn parses_only_listening_tcp_rows() {
        let ports = parse_netstat(NETSTAT);
        assert_eq!(
            ports.iter().map(|item| item.port).collect::<Vec<_>>(),
            vec![135, 5173, 8080]
        );
        assert_eq!(ports[0].address, "0.0.0.0:135");
        assert_eq!(ports[0].pid, Some(1188));
    }

    #[test]
    fn ignores_unparseable_rows() {
        assert!(parse_netstat("Active Connections\n\n  Proto  Local Address\n").is_empty());
        assert!(parse_netstat("  TCP    not-an-address   0.0.0.0:0   LISTENING   1").is_empty());
    }

    #[test]
    fn deduplicates_repeated_rows() {
        let duplicated = "  TCP    127.0.0.1:5173    0.0.0.0:0    LISTENING    4242\n  TCP    127.0.0.1:5173    0.0.0.0:0    LISTENING    4242\n";
        assert_eq!(parse_netstat(duplicated).len(), 1);
    }

    #[test]
    fn lists_ports_on_this_machine() {
        let ports = list_ports().expect("port listing should not fail");
        assert!(ports.windows(2).all(|pair| pair[0].port <= pair[1].port));
    }
}
