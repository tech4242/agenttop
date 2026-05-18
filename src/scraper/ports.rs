//! Listening-port discovery via `lsof`.
//!
//! Returns a `PID -> port` map for every TCP listener on the host. If `lsof`
//! isn't installed (some minimal containers, Windows), returns an empty map —
//! the rest of the scraper handles a missing port table gracefully.
//!
//! Orphan-port detection lives in `scraper::mod` because it requires
//! cross-tick state. This file only provides the live snapshot.

use std::collections::HashMap;
use std::process::Command;

pub fn scan_listening_ports() -> HashMap<u32, u16> {
    let output = Command::new("lsof")
        .args(["-nP", "-iTCP", "-sTCP:LISTEN", "-Fpn"])
        .output();

    let Ok(output) = output else {
        return HashMap::new();
    };
    if !output.status.success() {
        return HashMap::new();
    }

    parse_lsof_f(&String::from_utf8_lossy(&output.stdout))
}

/// Parse `lsof -F pn` field output. The `-F pn` flag emits one field per
/// line, prefixed by `p` (pid) or `n` (name). Format example:
///
/// ```text
/// p12345
/// n*:8000
/// p67890
/// n127.0.0.1:5432
/// ```
fn parse_lsof_f(text: &str) -> HashMap<u32, u16> {
    let mut out = HashMap::new();
    let mut current_pid: Option<u32> = None;
    for line in text.lines() {
        let line = line.trim();
        let Some(prefix) = line.chars().next() else {
            continue;
        };
        let rest = &line[1..];
        match prefix {
            'p' => current_pid = rest.parse::<u32>().ok(),
            'n' => {
                let Some(pid) = current_pid else { continue };
                if let Some(port) = extract_port(rest) {
                    // Don't overwrite an existing entry — keep the first port
                    // we see per PID (avoids flapping when a process listens
                    // on many ports).
                    out.entry(pid).or_insert(port);
                }
            }
            _ => {}
        }
    }
    out
}

/// Extract the port from an lsof `n` field like `*:8000`, `127.0.0.1:5432`,
/// `[::1]:8080`, or `*:http`.
fn extract_port(name: &str) -> Option<u16> {
    let last_colon = name.rfind(':')?;
    let port_str = &name[last_colon + 1..];
    let port_str = port_str.split_whitespace().next().unwrap_or(port_str);
    port_str.parse::<u16>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lsof_f_field_output() {
        let sample = "p1234\nn*:8000\np5678\nn127.0.0.1:5432\np9999\nn[::1]:8080\n";
        let map = parse_lsof_f(sample);
        assert_eq!(map.get(&1234).copied(), Some(8000));
        assert_eq!(map.get(&5678).copied(), Some(5432));
        assert_eq!(map.get(&9999).copied(), Some(8080));
    }

    #[test]
    fn keeps_first_port_per_pid() {
        let sample = "p1\nn*:80\nn*:443\n";
        let map = parse_lsof_f(sample);
        assert_eq!(map.get(&1).copied(), Some(80));
    }

    #[test]
    fn skips_symbolic_service_names() {
        let sample = "p1\nn*:http\n";
        let map = parse_lsof_f(sample);
        assert!(!map.contains_key(&1));
    }

    #[test]
    fn extract_port_handles_ipv6() {
        assert_eq!(extract_port("[::1]:8080"), Some(8080));
        assert_eq!(extract_port("*:1234"), Some(1234));
        assert_eq!(extract_port("127.0.0.1:5432"), Some(5432));
        assert_eq!(extract_port("*:http"), None);
    }
}
