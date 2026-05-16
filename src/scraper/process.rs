//! Process enumeration via `sysinfo` — replaces shelling out to `ps`.
//!
//! Cross-platform: works on macOS and Linux equally. Holds a `System` instance
//! between ticks so refreshes are incremental.

use std::collections::HashMap;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};

#[derive(Debug, Clone)]
pub struct ProcInfo {
    #[allow(dead_code)]
    pub pid: u32,
    #[allow(dead_code)]
    pub ppid: u32,
    /// Argv joined with spaces — display string.
    pub cmd: String,
    /// Program name (last component of argv[0]).
    pub name: String,
    pub rss_kb: u64,
    #[allow(dead_code)]
    pub cwd: Option<String>,
}

pub struct ProcessScanner {
    sys: System,
    /// PID → ProcInfo for the current snapshot.
    cache: HashMap<u32, ProcInfo>,
    /// PPID → child PIDs.
    children_by_ppid: HashMap<u32, Vec<u32>>,
}

impl Default for ProcessScanner {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessScanner {
    pub fn new() -> Self {
        let refresh = RefreshKind::nothing().with_processes(
            ProcessRefreshKind::nothing()
                .with_cmd(sysinfo::UpdateKind::Always)
                .with_memory()
                .with_cwd(sysinfo::UpdateKind::Always),
        );
        let mut sys = System::new_with_specifics(refresh);
        sys.refresh_specifics(refresh);

        let mut scanner = Self {
            sys,
            cache: HashMap::new(),
            children_by_ppid: HashMap::new(),
        };
        scanner.rebuild_caches();
        scanner
    }

    pub fn refresh(&mut self) {
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_cmd(sysinfo::UpdateKind::Always)
                .with_memory()
                .with_cwd(sysinfo::UpdateKind::Always),
        );
        self.rebuild_caches();
    }

    fn rebuild_caches(&mut self) {
        self.cache.clear();
        self.children_by_ppid.clear();

        for (pid, proc) in self.sys.processes() {
            let pid_u32 = pid.as_u32();
            let ppid_u32 = proc.parent().map(|p| p.as_u32()).unwrap_or(0);
            let cmd = proc
                .cmd()
                .iter()
                .map(|s| s.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(" ");
            let name = proc.name().to_string_lossy().into_owned();
            let cwd = proc
                .cwd()
                .map(|p| p.to_string_lossy().into_owned());

            self.cache.insert(
                pid_u32,
                ProcInfo {
                    pid: pid_u32,
                    ppid: ppid_u32,
                    cmd,
                    name,
                    rss_kb: proc.memory() / 1024,
                    cwd,
                },
            );
            self.children_by_ppid.entry(ppid_u32).or_default().push(pid_u32);
        }
    }

    pub fn is_alive(&self, pid: u32) -> bool {
        self.cache.contains_key(&pid)
    }

    pub fn get(&self, pid: u32) -> Option<&ProcInfo> {
        self.cache.get(&pid)
    }

    /// Return all PIDs in the snapshot whose command-line contains `needle`.
    /// Used by future code paths that want to discover agents by process
    /// name rather than session-file presence.
    #[allow(dead_code)]
    pub fn find_by_cmd_substring(&self, needle: &str) -> Vec<u32> {
        self.cache
            .values()
            .filter(|p| p.cmd.contains(needle) || p.name.contains(needle))
            .map(|p| p.pid)
            .collect()
    }

    /// Recursively collect all descendants of `pid` (not including `pid` itself).
    pub fn descendants(&self, pid: u32) -> Vec<u32> {
        let mut out = Vec::new();
        let mut stack = vec![pid];
        while let Some(current) = stack.pop() {
            if let Some(kids) = self.children_by_ppid.get(&current) {
                for &kid in kids {
                    out.push(kid);
                    stack.push(kid);
                }
            }
        }
        out
    }

    #[allow(dead_code)]
    pub fn snapshot(&self) -> &HashMap<u32, ProcInfo> {
        &self.cache
    }
}

/// Convenience: convert raw sysinfo `Pid` to u32. Used in tests.
#[allow(dead_code)]
pub(crate) fn pid_to_u32(p: Pid) -> u32 {
    p.as_u32()
}
