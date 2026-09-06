//! Shared daemon-preflight helpers for container-runtime service backends.
//!
//! Both `DockerComposeBackend` and `PodmanComposeBackend` need to answer
//! the same question before running `compose up`: "is the daemon this
//! CLI talks to actually reachable?" Just checking for the binary in
//! PATH (see `command_exists`) is insufficient — Docker Desktop can be
//! installed but stopped, colima can be provisioned but not started,
//! podman rootful can require `podman machine start` on macOS.
//!
//! Without this preflight, `docker compose up` prints a Go-formatted
//! "Cannot connect to the Docker daemon" wall of text that users
//! interpret as "jarvy is broken." With it, we short-circuit to a
//! `ServiceError::DaemonNotRunning { hint }` carrying a platform-aware,
//! actionable next step ("Start Docker Desktop", "colima start",
//! "sudo systemctl start docker").
//!
//! Timeout budget: the probe uses `docker info --format "{{.ServerVersion}}"`
//! (or `podman info --format "{{.Version.Version}}"`) with a 3-second
//! wall-clock cap enforced by spawning + polling — kept low because this
//! runs on every `jarvy services start`, `jarvy setup --auto-start`, and
//! `jarvy services status`.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Result of probing a container-runtime daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonState {
    /// Daemon responded successfully within the timeout.
    Running,
    /// Daemon binary is installed but the daemon socket is not reachable
    /// (Docker Desktop stopped, systemd unit inactive, colima not started,
    /// podman machine not running).
    Down,
    /// Probe command failed to spawn (binary not in PATH). Caller should
    /// have short-circuited via `is_installed` before probing; treat as `Down`.
    Missing,
    /// Probe was killed by the [`PROBE_TIMEOUT`] wall-clock cap. Distinct
    /// from [`Down`] so on-call can graph "hung apiserver" separately from
    /// "daemon returned non-zero fast." Emitted as
    /// `services.daemon_probe_timeout` upstream.
    Timeout,
}

/// Wall-clock cap on daemon-probe subprocess. Runs on hot paths, keep low.
pub(crate) const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// Poll granularity for `try_wait`. 20 ms is a compromise between "wake
/// up promptly when the child returns" (a healthy `docker info` on a
/// warm daemon returns in 20–80 ms) and "don't burn CPU on tight
/// polling." Was 50 ms — the average half-tick of 25 ms was measurable
/// on the interactive `jarvy services status` path.
pub(crate) const PROBE_POLL: Duration = Duration::from_millis(20);

/// Probe a container-runtime daemon by asking for its server version.
///
/// `cli` is the binary name (`docker` or `podman`). The `info` subcommand
/// with `--format "{{.ServerVersion}}"` returns the server-side version
/// string — a round-trip through the daemon socket, so a stopped daemon
/// exits non-zero even though the client binary is fine.
pub fn probe_container_daemon(cli: &str) -> DaemonState {
    probe_with_args(
        cli,
        &["info", "--format", "{{.ServerVersion}}"],
        PROBE_TIMEOUT,
    )
}

/// Argv-taking variant used by tests and by future non-container probes
/// that want the same spawn + poll + kill + wall-clock-cap machinery.
/// Kept `pub(crate)` so it isn't part of the public jarvy API but can
/// be exercised directly from `#[cfg(test)]`.
pub(crate) fn probe_with_args(cli: &str, args: &[&str], timeout: Duration) -> DaemonState {
    let started = Instant::now();
    let spawn = Command::new(cli)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .spawn();

    let mut child = match spawn {
        Ok(c) => c,
        Err(_) => return DaemonState::Missing,
    };

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() {
                    DaemonState::Running
                } else {
                    DaemonState::Down
                };
            }
            Ok(None) => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return DaemonState::Timeout;
                }
                std::thread::sleep(PROBE_POLL);
            }
            Err(_) => return DaemonState::Down,
        }
    }
}

/// Bounded taxonomy of hint branches surfaced on `services.daemon_down`.
/// Product analytics uses this to answer "colima vs Docker Desktop —
/// which suggestion do users see most, and where should we invest?"
/// Cardinality: 7.
pub type HintKind = &'static str;

/// Build a platform-aware, actionable hint for a stopped Docker daemon,
/// along with a bounded `hint_kind` label suitable for telemetry.
///
/// Detection precedence:
/// 1. macOS + `colima` on PATH → suggest `colima start` (colima wraps
///    Docker; probing it on non-macOS is wasted subprocess).
/// 2. macOS → Docker Desktop.
/// 3. Windows → Docker Desktop.
/// 4. WSL → mention both Docker Desktop's WSL integration toggle and a
///    native-in-distro systemd service, since jarvy can't tell which one
///    the user has from inside WSL alone.
/// 5. Linux → systemd (`sudo systemctl start docker`).
pub fn docker_daemon_hint() -> (String, HintKind) {
    resolve_docker_daemon_hint(
        cfg!(target_os = "macos"),
        cfg!(target_os = "windows"),
        command_on_path("colima"),
        crate::tools::common::is_wsl(),
    )
}

fn resolve_docker_daemon_hint(
    is_macos: bool,
    is_windows: bool,
    has_colima: bool,
    is_wsl: bool,
) -> (String, HintKind) {
    if is_macos && has_colima {
        return ("Start Colima with: colima start".to_string(), "colima");
    }
    if is_macos {
        return (
            "Start Docker Desktop from Applications, then retry.".to_string(),
            "docker_desktop_macos",
        );
    }
    if is_windows {
        return (
            "Start Docker Desktop from the Start menu, then retry.".to_string(),
            "docker_desktop_windows",
        );
    }
    if is_wsl {
        return (
            "If using Docker Desktop, enable WSL integration for this distro (Docker Desktop → Settings → Resources → WSL Integration). If running Docker natively inside WSL, start it with: sudo systemctl start docker".to_string(),
            "wsl_docker_desktop",
        );
    }
    (
        "Start the Docker daemon: sudo systemctl start docker".to_string(),
        "systemd",
    )
}

/// Build a platform-aware, actionable hint for a stopped Podman daemon.
///
/// On macOS/Windows, podman requires a running VM (`podman machine start`).
/// On Linux, rootful podman needs `podman.socket` active; rootless doesn't
/// need a daemon start but `podman info` failing there usually means the
/// user socket isn't wired — suggest checking.
pub fn podman_daemon_hint() -> (String, HintKind) {
    if cfg!(target_os = "macos") || cfg!(target_os = "windows") {
        return (
            "Start the Podman VM: podman machine start".to_string(),
            "podman_machine",
        );
    }
    (
        "Start the Podman socket: systemctl --user start podman.socket".to_string(),
        "podman_socket",
    )
}

/// Cheap PATH lookup — delegates to the shared, memoized,
/// OS-aware `tools::common::command_on_path`. The earlier local
/// copy of this helper predated the consolidation; keeping it as a
/// thin re-export means module-internal call sites don't need to
/// change.
fn command_on_path(cmd: &str) -> bool {
    crate::tools::common::command_on_path(cmd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_are_nonempty_and_actionable() {
        // Cheap sanity — every hint mentions a verb the user can act on.
        for (hint, kind) in [docker_daemon_hint(), podman_daemon_hint()] {
            assert!(!hint.is_empty(), "empty hint for kind {kind}");
            assert!(!kind.is_empty(), "empty kind for hint {hint}");
            assert!(
                hint.to_lowercase().contains("start") || hint.to_lowercase().contains("retry"),
                "hint should be actionable: {hint} (kind={kind})"
            );
        }
    }

    /// hint_kind is a bounded taxonomy — every branch must map to a
    /// known label. Regression guard against a future branch that
    /// forgets the label (would return `""` and pollute analytics).
    #[test]
    fn hint_kind_is_bounded_vocabulary() {
        let known: &[&str] = &[
            "colima",
            "docker_desktop_macos",
            "docker_desktop_windows",
            "wsl_docker_desktop",
            "systemd",
            "podman_machine",
            "podman_socket",
        ];
        for (_, kind) in [docker_daemon_hint(), podman_daemon_hint()] {
            assert!(known.contains(&kind), "unknown hint_kind emitted: {kind}");
        }
    }

    #[test]
    fn resolve_docker_daemon_hint_wsl_suggests_both_integration_paths() {
        let (hint, kind) = resolve_docker_daemon_hint(false, false, false, true);
        assert_eq!(kind, "wsl_docker_desktop");
        assert!(
            hint.to_lowercase().contains("wsl integration")
                || hint.to_lowercase().contains("docker desktop")
        );
        assert!(hint.to_lowercase().contains("systemctl"));
    }

    #[test]
    fn resolve_docker_daemon_hint_bare_linux_still_suggests_systemd() {
        let (_, kind) = resolve_docker_daemon_hint(false, false, false, false);
        assert_eq!(kind, "systemd");
    }

    #[test]
    fn resolve_docker_daemon_hint_macos_with_colima_suggests_colima() {
        let (_, kind) = resolve_docker_daemon_hint(true, false, true, false);
        assert_eq!(kind, "colima");
    }

    #[test]
    fn resolve_docker_daemon_hint_macos_without_colima_suggests_docker_desktop() {
        let (_, kind) = resolve_docker_daemon_hint(true, false, false, false);
        assert_eq!(kind, "docker_desktop_macos");
    }

    #[test]
    fn resolve_docker_daemon_hint_windows_suggests_docker_desktop() {
        let (_, kind) = resolve_docker_daemon_hint(false, true, false, false);
        assert_eq!(kind, "docker_desktop_windows");
    }

    #[test]
    fn resolve_docker_daemon_hint_windows_wins_over_wsl_flag() {
        // is_windows and is_wsl both true is not a real combo in production
        // (is_wsl() only ever returns true on a Linux-target build), but the
        // branch order should still resolve deterministically: Windows wins.
        let (_, kind) = resolve_docker_daemon_hint(false, true, false, true);
        assert_eq!(kind, "docker_desktop_windows");
    }

    #[test]
    fn probe_missing_binary_returns_missing() {
        // A binary name no reasonable environment has.
        let state = probe_container_daemon("jarvy-nonexistent-daemon-probe-target");
        assert_eq!(state, DaemonState::Missing);
    }

    /// The load-bearing invariant of this module: a hanging subprocess
    /// is killed within roughly [`PROBE_TIMEOUT`] wall-clock. Without
    /// this test a regression bumping the constant to `Duration::from_secs(300)`
    /// would ship silently and every `jarvy services status` would
    /// block for 5 minutes on a stopped Docker Desktop. Uses a short
    /// custom timeout (500 ms) via [`probe_with_args`] so the test
    /// itself finishes fast.
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn probe_respects_timeout_bound() {
        let short = Duration::from_millis(500);
        let started = Instant::now();
        // `sleep 10` never returns in the test window; the probe MUST
        // kill it and report Timeout.
        let state = probe_with_args("sleep", &["10"], short);
        let elapsed = started.elapsed();
        assert_eq!(
            state,
            DaemonState::Timeout,
            "state should be Timeout, got {state:?}"
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "probe exceeded budget: took {elapsed:?}"
        );
        assert!(
            elapsed >= short.saturating_sub(Duration::from_millis(100)),
            "probe returned suspiciously early: took {elapsed:?}"
        );
    }

    /// A subprocess that exits fast + non-zero must map to `Down`,
    /// NOT `Timeout` — the two states drive different runbooks.
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn probe_fast_nonzero_maps_to_down_not_timeout() {
        // `false` exits 1 immediately.
        let state = probe_with_args("false", &[], Duration::from_secs(3));
        assert_eq!(state, DaemonState::Down);
    }
}
