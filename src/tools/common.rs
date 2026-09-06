#![allow(dead_code)] // Public API for tool installation utilities

use std::process::{Command, Output};
use std::sync::OnceLock;

use crate::network::config::NetworkConfig;
use crate::network::propagate::apply_network_config;

#[derive(thiserror::Error, Debug)]
pub enum InstallError {
    #[error("unsupported platform")]
    Unsupported,
    #[error("prerequisite missing: {0}")]
    Prereq(std::borrow::Cow<'static, str>),
    #[error("invalid permissions: {0}")]
    InvalidPermissions(&'static str),
    #[error("command failed: {cmd} (code: {code:?})\n{stderr}")]
    CommandFailed {
        cmd: String,
        code: Option<i32>,
        stderr: String,
    },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse error: {0}")]
    Parse(&'static str),
}

impl InstallError {
    /// Stable discriminant for telemetry / dashboard queries. The
    /// `Display` form may embed user-controlled stderr or package
    /// names; this returns a fixed string so dashboards can group
    /// without parsing free text. Mirrors `PackageError::kind()` and
    /// `AiHookError::kind()`.
    ///
    /// `CommandFailed` is further classified by inspecting stderr —
    /// `tap_fetch_failed` for a brew tap network/permission failure,
    /// `permission_required` for a sudo prompt, otherwise the
    /// generic `install_command_failed`.
    pub fn kind(&self) -> &'static str {
        match self {
            InstallError::Unsupported => "no_platform_installer",
            InstallError::Prereq(_) => "prereq_missing",
            InstallError::InvalidPermissions(_) => "permission_required",
            InstallError::CommandFailed { stderr, .. } => {
                let lower = stderr.to_ascii_lowercase();
                if lower.contains("tapping") && lower.contains("fatal:") {
                    "tap_fetch_failed"
                } else if lower.contains("permission denied") {
                    "permission_required"
                } else {
                    "install_command_failed"
                }
            }
            InstallError::Io(_) => "io",
            InstallError::Parse(_) => "parse",
        }
    }

    /// True for variants that mean "this tool has no install method
    /// on this platform" — distinct from a real install crash. The
    /// dispatch path routes these to `tool.unsupported` (a separate
    /// counter) rather than `tool.failed`.
    pub fn is_no_platform_installer(&self) -> bool {
        matches!(self, InstallError::Unsupported)
    }
}

// OS enum for config keys and runtime resolution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Linux,
    Macos,
    Windows,
    Bsd,
}

// Determine the current OS as our enum
pub fn current_os() -> Os {
    #[cfg(target_os = "linux")]
    {
        Os::Linux
    }
    #[cfg(target_os = "macos")]
    {
        Os::Macos
    }
    #[cfg(target_os = "windows")]
    {
        Os::Windows
    }
    #[cfg(target_os = "freebsd")]
    {
        Os::Bsd
    }
}

/// True when `proc_version` (the contents of Linux's `/proc/version`) indicates
/// a WSL kernel. Both WSL1 and WSL2 embed "microsoft" case-insensitively
/// (e.g. "5.15.153.1-microsoft-standard-WSL2" or the older "4.4.0-43-Microsoft").
pub(crate) fn proc_version_indicates_wsl(proc_version: &str) -> bool {
    proc_version.to_lowercase().contains("microsoft")
}

pub(crate) fn proc_version_file_indicates_wsl(path: &std::path::Path) -> bool {
    std::fs::read_to_string(path)
        .map(|v| proc_version_indicates_wsl(&v))
        .unwrap_or(false)
}

static IS_WSL: OnceLock<bool> = OnceLock::new();

/// Runtime detection of WSL (1 or 2), distinct from `current_os()`'s
/// compile-time OS check. A Linux-target jarvy binary reports `Os::Linux`
/// whether it's on bare-metal Linux or inside a WSL distro; this tells
/// those apart. Always false when not compiled for Linux.
pub fn is_wsl() -> bool {
    *IS_WSL.get_or_init(|| {
        #[cfg(target_os = "linux")]
        {
            proc_version_file_indicates_wsl(std::path::Path::new("/proc/version"))
        }
        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    })
}

// Global default for whether to use sudo on POSIX installs. Can be set from Config in main.
// None means: auto-detect per operation (try without sudo, then with if available).
static USE_SUDO_DEFAULT: OnceLock<Option<bool>> = OnceLock::new();

pub fn set_default_use_sudo(val: Option<bool>) {
    let _ = USE_SUDO_DEFAULT.set(val);
}

pub fn default_use_sudo() -> Option<bool> {
    if let Some(v) = USE_SUDO_DEFAULT.get() {
        *v
    } else {
        // Unset -> auto mode
        None
    }
}

/// Spawn a command, capturing its output, and emit a structured warning +
/// telemetry counter on failure. Replaces the duplicated
/// `match Command::new(...).output() { Err(e) => { eprintln!(...); return; } }`
/// pattern that proliferated in setup/provisioner during the panic-removal
/// sweep.
///
/// Returns `None` on spawn failure so callers can keep the
/// `let Some(out) = run_capture(...) else { return; };` shape that mirrors
/// the prior eprintln+return idiom but routes through tracing.
///
/// `stage` should be a bounded label (e.g. `"hard_dep_check"`,
/// `"macos_setup"`) — used as a low-cardinality telemetry attribute.
pub fn run_capture(cmd: &str, args: &[&str], stage: &str, context: &str) -> Option<Output> {
    let span = tracing::info_span!(
        "subprocess.exec",
        cmd = %cmd,
        args_count = args.len(),
        stage = %stage,
    );
    let _g = span.enter();
    let start = std::time::Instant::now();
    match Command::new(cmd).args(args).output() {
        Ok(out) => {
            tracing::debug!(
                event = "subprocess.completed",
                exit_code = out.status.code().unwrap_or(-1),
                duration_ms = start.elapsed().as_millis() as u64,
                "subprocess finished",
            );
            Some(out)
        }
        Err(e) => {
            tracing::warn!(
                event = "setup.subprocess.failed",
                stage = %stage,
                command = %cmd,
                context = %context,
                error = %e,
                duration_ms = start.elapsed().as_millis() as u64,
            );
            eprintln!("{context}: {e}");
            None
        }
    }
}

/// winget (and some other installers) prints its real diagnostic to stdout
/// instead of stderr, so fall back to stdout when stderr is empty.
pub(crate) fn command_failure_stderr(out: &Output) -> String {
    let stderr = String::from_utf8_lossy(&out.stderr);
    if stderr.trim().is_empty() {
        String::from_utf8_lossy(&out.stdout).into_owned()
    } else {
        stderr.into_owned()
    }
}

#[must_use = "this Result may contain an error that should be handled"]
pub fn run(cmd: &str, args: &[&str]) -> Result<Output, InstallError> {
    // Fast, deterministic tests: allow skipping external command execution.
    // Integration tests can opt-in via JARVY_FAST_TEST; unit tests default to skip unless explicitly overridden.
    if std::env::var_os("JARVY_FAST_TEST").is_some() {
        return Err(InstallError::Prereq(
            "skipped external command in fast test mode".into(),
        ));
    }
    #[cfg(test)]
    {
        if std::env::var_os("JARVY_RUN_EXTERNAL_CMDS_IN_TEST").is_none() {
            return Err(InstallError::Prereq(
                "external commands disabled during unit tests".into(),
            ));
        }
    }

    let out = Command::new(cmd).args(args).output().map_err(|e| {
        use std::io::ErrorKind::*;
        match e.kind() {
            NotFound => InstallError::Prereq("required command not found on PATH".into()),
            PermissionDenied => {
                InstallError::InvalidPermissions("operation requires elevated privileges")
            }
            _ => InstallError::Io(e),
        }
    })?;

    if !out.status.success() {
        return Err(InstallError::CommandFailed {
            cmd: cmd.to_string(),
            code: out.status.code(),
            stderr: command_failure_stderr(&out),
        });
    }
    Ok(out)
}

/// Run a command with network/proxy configuration applied.
///
/// This variant applies HTTP_PROXY, HTTPS_PROXY, NO_PROXY, and CA bundle
/// environment variables to the spawned process based on the NetworkConfig.
#[must_use = "this Result may contain an error that should be handled"]
pub fn run_with_network(
    cmd: &str,
    args: &[&str],
    network: Option<&NetworkConfig>,
    tool_name: &str,
) -> Result<Output, InstallError> {
    // Fast, deterministic tests: allow skipping external command execution.
    if std::env::var_os("JARVY_FAST_TEST").is_some() {
        return Err(InstallError::Prereq(
            "skipped external command in fast test mode".into(),
        ));
    }
    #[cfg(test)]
    {
        if std::env::var_os("JARVY_RUN_EXTERNAL_CMDS_IN_TEST").is_none() {
            return Err(InstallError::Prereq(
                "external commands disabled during unit tests".into(),
            ));
        }
    }

    let mut command = Command::new(cmd);
    command.args(args);

    // Apply network/proxy configuration if provided
    if let Some(net_config) = network {
        apply_network_config(&mut command, net_config, tool_name);
    }

    let out = command.output().map_err(|e| {
        use std::io::ErrorKind::*;
        match e.kind() {
            NotFound => InstallError::Prereq("required command not found on PATH".into()),
            PermissionDenied => {
                InstallError::InvalidPermissions("operation requires elevated privileges")
            }
            _ => InstallError::Io(e),
        }
    })?;

    if !out.status.success() {
        return Err(InstallError::CommandFailed {
            cmd: cmd.to_string(),
            code: out.status.code(),
            stderr: command_failure_stderr(&out),
        });
    }
    Ok(out)
}

/// Run a command, prefixing with sudo if configured and applicable (non-Windows)
#[must_use = "this Result may contain an error that should be handled"]
pub fn run_maybe_sudo(use_sudo: bool, cmd: &str, args: &[&str]) -> Result<Output, InstallError> {
    match current_os() {
        Os::Windows => run(cmd, args),
        Os::Linux | Os::Macos | Os::Bsd => {
            if use_sudo {
                // sudo <cmd> <args...>
                let mut all = Vec::with_capacity(1 + args.len());
                all.push(cmd);
                all.extend_from_slice(args);
                run("sudo", &all)
            } else {
                run(cmd, args)
            }
        }
    }
}

/// Run a command with sudo and network/proxy configuration.
///
/// Combines sudo elevation with proxy settings propagation.
#[must_use = "this Result may contain an error that should be handled"]
pub fn run_maybe_sudo_with_network(
    use_sudo: bool,
    cmd: &str,
    args: &[&str],
    network: Option<&NetworkConfig>,
    tool_name: &str,
) -> Result<Output, InstallError> {
    match current_os() {
        Os::Windows => run_with_network(cmd, args, network, tool_name),
        Os::Linux | Os::Macos | Os::Bsd => {
            if use_sudo {
                // sudo -E preserves environment (including proxy vars)
                // sudo <cmd> <args...>
                let mut all = Vec::with_capacity(2 + args.len());
                all.push("-E"); // Preserve environment
                all.push(cmd);
                all.extend_from_slice(args);
                run_with_network("sudo", &all, network, tool_name)
            } else {
                run_with_network(cmd, args, network, tool_name)
            }
        }
    }
}

/// Runtime context threaded to each tool's install handler. Carries
/// the per-install knobs that `Tool` collects from `jarvy.toml` but
/// which the static `ToolSpec` cannot know at compile time.
///
/// Constructed once per `tools::add` call in the setup phase and
/// passed by reference. Handlers that don't care about these fields
/// simply ignore them.
#[derive(Debug, Clone, Default)]
pub struct InstallContext {
    /// Distribution / flavor selector (e.g. `"temurin"`, `"zulu"`).
    /// `None` = no user preference; handler uses its default.
    pub distribution: Option<String>,
    /// When `false`, refuse to fall back to the tool's default
    /// distribution if the requested one is unsupported on this OS.
    /// Default `true` via `InstallContext::none()`.
    pub fallback: bool,
}

impl InstallContext {
    /// Default context — no distribution override, fallback enabled.
    /// This is what plain scalar-form tools (`git = "latest"`) get.
    pub const fn none() -> Self {
        Self {
            distribution: None,
            fallback: true,
        }
    }
}

/// Process-wide cache of `has()` results.
///
/// Without this, `setup` forks `apt --version`, `sudo --version`,
/// `brew --version` etc. for every package install — for a 30-tool batch
/// that's 100+ extra forks before the actual installs even start (perf
/// review F-2). Cache key is the bare command name; we assume PATH and
/// binary presence are stable for the lifetime of one `jarvy setup`.
fn has_cache() -> &'static std::sync::RwLock<std::collections::HashMap<String, bool>> {
    static CACHE: std::sync::OnceLock<std::sync::RwLock<std::collections::HashMap<String, bool>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::RwLock::new(std::collections::HashMap::new()))
}

/// Drop one command's `has()` cache entry. The cache assumes PATH is
/// stable for the whole setup run; a fallback-route toolchain bootstrap
/// (PRD-060) deliberately breaks that assumption, so the runtime evicts
/// the entry before re-probing.
pub(crate) fn forget_has(cmd: &str) {
    if let Ok(mut write) = has_cache().write() {
        write.remove(cmd);
    }
}

fn has_uncached(cmd: &str) -> bool {
    // Presence = the binary spawned at all; exit status is deliberately
    // ignored. Cobra-style CLIs (talosctl, argocd) exit non-zero on
    // `--version` when they can't reach their server, yet the binary is
    // plainly installed. ENOENT / EACCES on spawn = absent.
    Command::new(cmd).arg("--version").output().is_ok()
}

/// Outcome of [`probe_with_timeout`]. Distinct from `Result<Output, io::Error>`
/// because `Timeout` needs to be reportable separately from a fast
/// non-zero exit — different runbooks. Keeps the helper 30 lines.
#[derive(Debug)]
pub enum ProbeResult {
    /// Child exited within the timeout. Carries the raw `Output`; the
    /// caller decides whether `status.success()` maps to "reachable."
    Completed(std::process::Output),
    /// `PROBE_TIMEOUT` wall-clock cap fired; child was killed.
    Timeout,
    /// `Command::spawn` returned `Err(io::ErrorKind::NotFound)` — binary
    /// not on PATH. Distinct from Timeout / non-zero exit.
    Missing,
    /// `Command::spawn` returned `Err(io::ErrorKind::PermissionDenied)`
    /// — binary present but not executable by this user.
    PermissionDenied,
    /// Any other IO error during spawn or wait.
    IoError(std::io::Error),
}

/// Spawn `<cmd> <args>` and either return its output or kill it when
/// `timeout` elapses. Poll interval is 20 ms. Used by:
///
/// - `commands::diagnose` k8s-liveness probes — `minikube status`,
///   `kind get clusters`, `k3d cluster list` have no CLI-level
///   timeout flag, so a stale cluster hangs the diagnose command
///   until the child exits (tens of seconds for minikube). This
///   helper enforces the "hard 2-second budget" the diagnose doc
///   comment claims.
/// - future non-container preflights that want the same spawn +
///   poll + kill + wall-clock-cap machinery without depending on
///   `services::preflight` (which returns a services-specific
///   `DaemonState`).
///
/// stdout / stderr are captured (piped). stdin is `null`.
pub fn probe_with_timeout(cmd: &str, args: &[&str], timeout: std::time::Duration) -> ProbeResult {
    use std::time::Instant;
    let started = Instant::now();
    let spawn = Command::new(cmd)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .stdin(std::process::Stdio::null())
        .spawn();
    let mut child = match spawn {
        Ok(c) => c,
        Err(e) => {
            return match e.kind() {
                std::io::ErrorKind::NotFound => ProbeResult::Missing,
                std::io::ErrorKind::PermissionDenied => ProbeResult::PermissionDenied,
                _ => ProbeResult::IoError(e),
            };
        }
    };
    loop {
        match child.try_wait() {
            Ok(Some(_)) => match child.wait_with_output() {
                Ok(out) => return ProbeResult::Completed(out),
                Err(e) => return ProbeResult::IoError(e),
            },
            Ok(None) => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return ProbeResult::Timeout;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(e) => return ProbeResult::IoError(e),
        }
    }
}

/// Return true iff `cmd` resolves on `PATH`. Different from [`has`]:
/// - `has` spawns `<cmd> --version` — proves the binary is *runnable*
///   (exit code deliberately ignored; see `has_uncached`). Right for
///   tools jarvy is about to invoke.
/// - `command_on_path` shells out to `which` (POSIX) / `where`
///   (Windows) — proves the binary is *reachable*. Right for
///   detection paths where we care about presence but don't want to
///   pay the cost (or side effects) of invoking `--version`.
///
/// Backed by the same process-lifetime `RwLock<HashMap>` cache as
/// `has`, keyed on `cmd` + a discriminator so the two questions
/// don't collide. Consolidates three previous copies that had
/// drifted (one broken on Windows because it used `which`
/// unconditionally — see `services::mod`, `services::preflight`,
/// `dotfiles`).
pub fn command_on_path(cmd: &str) -> bool {
    let key = format!("__path::{cmd}");
    if let Ok(read) = has_cache().read()
        && let Some(&hit) = read.get(&key)
    {
        return hit;
    }
    let result = command_on_path_uncached(cmd);
    if let Ok(mut write) = has_cache().write() {
        write.insert(key, result);
    }
    result
}

fn command_on_path_uncached(cmd: &str) -> bool {
    let (probe, arg) = if cfg!(target_os = "windows") {
        ("where", cmd)
    } else {
        ("which", cmd)
    };
    Command::new(probe)
        .arg(arg)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Canonical error message emitted when a cargo-installed tool
/// (`bacon`, `cargo-nextest`, `release-plz`, …) finds no `cargo` on
/// PATH. Deduped from three per-tool `InstallError::Prereq(...)` sites
/// that had drifted into slightly-different wordings; centralising
/// them here means a future edit to the hint (say, adding a `rustup`
/// install pointer) lands once, not three-plus times.
pub(crate) const RUST_TOOLCHAIN_MISSING_HINT: &str = "cargo not found — install the Rust toolchain first \
     (add `rust = \"latest\"` under `[provisioner]` and re-run `jarvy setup`).";

/// `cargo install --locked <crate>`. Shared install path for Rust-
/// native CLIs that ship no first-party PM packaging (`bacon`,
/// `cargo-nextest`, `release-plz`, …).
///
/// - `--locked` forces cargo to use the crate's committed `Cargo.lock`
///   so the produced binary is reproducible against upstream CI. Drop
///   the flag and cargo re-resolves the entire dep graph, defeating
///   the supply-chain guarantee.
/// - Depends on `cargo` being on PATH; the canonical `rust` tool
///   under `[provisioner]` is the intended dependency (declared via
///   `depends_on: &["rust"]` on each tool that uses this helper).
///
/// Uniform error surface: `Err(InstallError::Prereq)` when cargo is
/// missing, with the shared `RUST_TOOLCHAIN_MISSING_HINT` message, so
/// telemetry `error_kind = "prereq_missing"` groups these failures
/// with any other cargo-dependent tool that adopts the helper later.
pub fn install_via_cargo_install(crate_name: &'static str) -> Result<(), InstallError> {
    if !has("cargo") {
        return Err(InstallError::Prereq(RUST_TOOLCHAIN_MISSING_HINT.into()));
    }
    let args = cargo_install_argv(crate_name);
    run("cargo", &args)?;
    Ok(())
}

/// Argv builder for `cargo install --locked <crate>`. Extracted so
/// the `--locked` supply-chain contract is testable at unit-test
/// tier without needing a real cargo binary (QA F6). The docstring
/// on `install_via_cargo_install` calls out `--locked` as
/// non-negotiable — this fn is the single place where the flag lives.
///
/// Returns a fixed-length array so the test can assert on shape
/// without lifetimes leaking into the caller.
pub fn cargo_install_argv(crate_name: &'static str) -> [&'static str; 3] {
    ["install", "--locked", crate_name]
}

pub fn has(cmd: &str) -> bool {
    if let Ok(read) = has_cache().read()
        && let Some(&hit) = read.get(cmd)
    {
        return hit;
    }
    let result = has_uncached(cmd);
    if let Ok(mut write) = has_cache().write() {
        write.insert(cmd.to_string(), result);
    }
    result
}

// Require a single command to exist on PATH, otherwise return a Prereq error with remediation.
pub fn require(cmd: &str, remediation: &'static str) -> Result<(), InstallError> {
    if has(cmd) {
        Ok(())
    } else {
        Err(InstallError::Prereq(remediation.into()))
    }
}

// Require one of multiple candidates (e.g., apt or apt-get)
pub fn require_any<'a>(
    candidates: &[&'a str],
    remediation: &'static str,
) -> Result<&'a str, InstallError> {
    for c in candidates {
        if has(c) {
            return Ok(*c);
        }
    }
    Err(InstallError::Prereq(remediation.into()))
}

/// Process-wide cache of `<cmd> --version` stdout. Probing version is
/// the second-largest fork source after `has()` because every tool's
/// presence-check + every `cmd_satisfies` lookup re-forks `<cmd>
/// --version`. For a 30-tool batch with several version requirements
/// per cmd, that's another 30-60 forks. Cache the raw stdout once per
/// command (perf P1, review item 21) and let `cmd_satisfies` reuse it.
fn version_cache() -> &'static std::sync::RwLock<std::collections::HashMap<String, Option<String>>>
{
    static CACHE: std::sync::OnceLock<
        std::sync::RwLock<std::collections::HashMap<String, Option<String>>>,
    > = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::RwLock::new(std::collections::HashMap::new()))
}

/// Probe `cmd` for version output, trying `--version`, `-V`, then the
/// `version` subcommand — each capped at 5 s via [`probe_with_timeout`].
///
/// Tolerant by design (doctor false-positive fix):
/// - Non-zero exit does NOT discard output. Cobra-style CLIs (talosctl,
///   argocd) print their client version yet exit non-zero when they
///   can't reach a server.
/// - stdout + stderr combined — many CLIs banner on stderr.
/// - Some CLIs answer only `version` as a subcommand (cue), hence the
///   flag chain.
///
/// Return priority:
/// 1. successful exit whose output parses as a version — stops the chain
/// 2. any exit whose output parses as a version
/// 3. successful exit without a parseable version (substring fallback in
///    `version_satisfies` still gets a shot)
/// 4. `None` — binary absent, unspawnable, or nothing usable came back
pub fn cmd_version_output(cmd: &str) -> Option<String> {
    if let Ok(read) = version_cache().read()
        && let Some(hit) = read.get(cmd)
    {
        return hit.clone();
    }
    let probed = cmd_version_output_uncached(cmd);
    if let Ok(mut write) = version_cache().write() {
        write.insert(cmd.to_string(), probed.clone());
    }
    probed
}

fn cmd_version_output_uncached(cmd: &str) -> Option<String> {
    const FLAGS: [&str; 3] = ["--version", "-V", "version"];
    let timeout = std::time::Duration::from_secs(5);
    let mut parseable_any_exit: Option<String> = None;
    let mut unparseable_success: Option<String> = None;
    for flag in FLAGS {
        match probe_with_timeout(cmd, &[flag], timeout) {
            ProbeResult::Completed(out) => {
                let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
                if !out.stderr.is_empty() {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(&String::from_utf8_lossy(&out.stderr));
                }
                if text.trim().is_empty() {
                    continue;
                }
                let parseable = super::version::extract_version(&text).is_some();
                if out.status.success() && parseable {
                    return Some(text);
                }
                if parseable {
                    parseable_any_exit.get_or_insert(text);
                } else if out.status.success() {
                    unparseable_success.get_or_insert(text);
                }
            }
            ProbeResult::Missing | ProbeResult::PermissionDenied => return None,
            ProbeResult::Timeout | ProbeResult::IoError(_) => {}
        }
    }
    parseable_any_exit.or(unparseable_success)
}

/// Check if a command's version satisfies the given requirement.
///
/// Uses proper semantic versioning comparison instead of substring matching.
/// Supports requirements like:
/// - `"latest"` or `"*"`: Always passes
/// - `"3.10"`: Matches 3.10.x
/// - `"3.10.0"`: Exact match
/// - `">= 3.10"`: Minimum version
/// - `">= 3.10, < 4.0"`: Range expression
pub fn cmd_satisfies(cmd: &str, requirement: &str) -> bool {
    match cmd_version_output(cmd) {
        Some(out) => super::version::version_satisfies(&out, requirement),
        None => false,
    }
}

pub fn plan_sudo_attempts(use_sudo: Option<bool>, sudo_available: bool) -> Vec<bool> {
    match use_sudo {
        Some(flag) => vec![flag],
        None => {
            if sudo_available {
                vec![false, true]
            } else {
                vec![false]
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PackageManager {
    Apt,
    Dnf,
    Yum,
    Zypper,
    Pacman,
    Apk,
    Brew,
    BrewCask, // Homebrew casks (GUI apps)
    Winget,
    Choco,
    Pkg, // FreeBSD pkg
}

#[cfg(target_os = "linux")]
pub fn detect_linux_pm() -> Option<PackageManager> {
    use std::fs;

    // (Optional) use /etc/os-release to bias choices when you need vendor repos
    // ID / ID_LIKE fields are the standard signals.  [oai_citation:0‡Freedesktop](https://www.freedesktop.org/software/systemd/man/os-release.html?utm_source=chatgpt.com) [oai_citation:1‡Debian Manpages](https://manpages.debian.org/trixie/systemd/os-release.5.en.html?utm_source=chatgpt.com)
    let _os_release = fs::read_to_string("/etc/os-release").unwrap_or_default();

    if has("apt-get") || has("apt") {
        return Some(PackageManager::Apt);
    }
    if has("dnf") {
        return Some(PackageManager::Dnf);
    }
    if has("yum") {
        return Some(PackageManager::Yum);
    }
    if has("zypper") {
        return Some(PackageManager::Zypper);
    }
    if has("pacman") {
        return Some(PackageManager::Pacman);
    }
    if has("apk") {
        return Some(PackageManager::Apk);
    }
    None
}

#[cfg(target_os = "freebsd")]
pub fn detect_bsd_pm() -> Option<PackageManager> {
    if has("pkg") {
        return Some(PackageManager::Pkg);
    }
    None
}

#[cfg(test)]
mod sudo_plan_tests {
    use super::plan_sudo_attempts;

    #[test]
    fn plan_some_true_only_true() {
        let v = plan_sudo_attempts(Some(true), true);
        assert_eq!(v, vec![true]);
    }

    #[test]
    fn plan_some_false_only_false() {
        let v = plan_sudo_attempts(Some(false), true);
        assert_eq!(v, vec![false]);
    }

    #[test]
    fn plan_none_with_sudo_available() {
        let v = plan_sudo_attempts(None, true);
        assert_eq!(v, vec![false, true]);
    }

    #[test]
    fn plan_none_without_sudo_available() {
        let v = plan_sudo_attempts(None, false);
        assert_eq!(v, vec![false]);
    }
}

#[cfg(test)]
mod install_error_kind_tests {
    use super::*;

    #[test]
    fn unsupported_is_classified_as_no_platform_installer() {
        let e = InstallError::Unsupported;
        assert_eq!(e.kind(), "no_platform_installer");
        assert!(e.is_no_platform_installer());
    }

    #[test]
    fn prereq_is_classified_as_prereq_missing() {
        let e = InstallError::Prereq("Homebrew not found".into());
        assert_eq!(e.kind(), "prereq_missing");
        assert!(!e.is_no_platform_installer());
    }

    #[test]
    fn command_failed_with_tap_fatal_classifies_as_tap_fetch() {
        let e = InstallError::CommandFailed {
            cmd: "brew".to_string(),
            code: Some(1),
            stderr: "==> Tapping nats-io/nats-tools\nfatal: unable to access".to_string(),
        };
        assert_eq!(e.kind(), "tap_fetch_failed");
    }

    #[test]
    fn command_failed_with_permission_denied_classifies_correctly() {
        let e = InstallError::CommandFailed {
            cmd: "apt".to_string(),
            code: Some(13),
            stderr: "E: Permission denied (13)".to_string(),
        };
        assert_eq!(e.kind(), "permission_required");
    }

    #[test]
    fn command_failed_generic_classifies_as_install_command_failed() {
        let e = InstallError::CommandFailed {
            cmd: "brew".to_string(),
            code: Some(1),
            stderr: "Error: formula not found".to_string(),
        };
        assert_eq!(e.kind(), "install_command_failed");
    }

    /// Uniform error message + classification when cargo is missing.
    /// QA F6 supply-chain contract: `--locked` MUST be in the argv.
    /// The docstring on `install_via_cargo_install` explains why
    /// dropping the flag defeats reproducibility — a refactor that
    /// silently omits it (someone "cleaning up" to
    /// `cargo install <crate>`) fails this test at unit-test tier
    /// instead of shipping. Also pins the crate-name is the last arg
    /// so a reorder ("cargo install --locked foo" → "cargo install
    /// foo --locked" — which cargo tolerates but changes semantics
    /// in some cargo-alias wrappers) trips the check.
    #[test]
    fn cargo_install_argv_pins_locked_flag() {
        for crate_name in ["bacon", "cargo-nextest", "release-plz"] {
            let args = cargo_install_argv(crate_name);
            assert_eq!(
                args,
                ["install", "--locked", crate_name],
                "cargo_install_argv({crate_name}) must be [install, --locked, {crate_name}]"
            );
        }
    }

    /// Pinned so the three Rust-native cargo-install tools (bacon,
    /// cargo-nextest, release-plz) — plus any new tool that adopts
    /// the helper — surface an identical, telemetry-groupable
    /// prereq-missing error. Prior to the extraction each tool had a
    /// slightly-different wording that broke `error_kind` grouping.
    #[test]
    #[serial_test::serial]
    #[allow(unsafe_code)]
    fn install_via_cargo_install_prereq_when_cargo_absent() {
        // PATH manipulation is process-global — serialise with other
        // env-sensitive tests. Restore on scope exit via RAII.
        struct PathGuard(Option<std::ffi::OsString>);
        impl Drop for PathGuard {
            fn drop(&mut self) {
                match self.0.take() {
                    Some(orig) => unsafe { std::env::set_var("PATH", orig) },
                    None => unsafe { std::env::remove_var("PATH") },
                }
            }
        }
        let _guard = PathGuard(std::env::var_os("PATH"));
        unsafe { std::env::set_var("PATH", "") };
        let e = install_via_cargo_install("bacon").expect_err("cargo absent → must Err(Prereq)");
        assert_eq!(e.kind(), "prereq_missing");
        assert_eq!(
            e.to_string(),
            format!("prerequisite missing: {RUST_TOOLCHAIN_MISSING_HINT}"),
            "Prereq message must be the canonical hint"
        );
    }
}

#[cfg(test)]
mod command_failure_stderr_tests {
    use super::*;

    #[cfg(unix)]
    fn dummy_exit_status(code: i32) -> std::process::ExitStatus {
        std::os::unix::process::ExitStatusExt::from_raw(code << 8)
    }

    #[cfg(windows)]
    fn dummy_exit_status(code: i32) -> std::process::ExitStatus {
        std::os::windows::process::ExitStatusExt::from_raw(code as u32)
    }

    #[test]
    fn command_failure_stderr_falls_back_to_stdout_when_stderr_empty() {
        let out = Output {
            status: dummy_exit_status(1),
            stdout: b"winget's real diagnostic".to_vec(),
            stderr: Vec::new(),
        };
        assert_eq!(command_failure_stderr(&out), "winget's real diagnostic");
    }

    #[test]
    fn command_failure_stderr_prefers_stderr_when_present() {
        let out = Output {
            status: dummy_exit_status(1),
            stdout: b"noise on stdout".to_vec(),
            stderr: b"the real stderr message".to_vec(),
        };
        assert_eq!(command_failure_stderr(&out), "the real stderr message");
    }

    #[test]
    fn command_failure_stderr_falls_back_to_stdout_when_stderr_is_whitespace_only() {
        let out = Output {
            status: dummy_exit_status(1),
            stdout: b"winget's real diagnostic".to_vec(),
            stderr: b"\n".to_vec(),
        };
        assert_eq!(command_failure_stderr(&out), "winget's real diagnostic");
    }
}

#[cfg(test)]
mod wsl_detect_tests {
    use super::{is_wsl, proc_version_file_indicates_wsl, proc_version_indicates_wsl};

    #[test]
    #[cfg(not(target_os = "linux"))]
    fn is_wsl_does_not_panic_and_respects_platform_gate() {
        assert!(!is_wsl(), "is_wsl() must be false on non-Linux targets");
    }

    #[test]
    fn proc_version_indicates_wsl_detects_wsl2() {
        assert!(proc_version_indicates_wsl(
            "Linux version 5.15.153.1-microsoft-standard-WSL2 (root@0c9e6c\
             a0be04) (gcc (GCC) 11.2.0, GNU ld (GNU Binutils) 2.37) #1 SMP \
             Fri Mar 29 15:34:35 UTC 2024"
        ));
    }

    #[test]
    fn proc_version_indicates_wsl_detects_wsl1() {
        assert!(proc_version_indicates_wsl(
            "Linux version 4.4.0-43-Microsoft (Microsoft@Microsoft.com) \
             (gcc version 5.4.0 (GCC) ) #1-Microsoft Wed Dec 31 14:42:53 \
             PST 2014"
        ));
    }

    #[test]
    fn proc_version_indicates_wsl_rejects_bare_linux() {
        assert!(!proc_version_indicates_wsl(
            "Linux version 5.15.0-91-generic (buildd@lcy02-amd64-119) \
             (gcc (Ubuntu 11.4.0-1ubuntu1~22.04) 11.4.0, GNU ld (GNU \
             Binutils for Ubuntu) 2.38) #101-Ubuntu SMP Tue Nov 14 \
             13:30:08 UTC 2023"
        ));
    }

    #[test]
    fn proc_version_file_unreadable_is_not_wsl() {
        let dir = tempfile::TempDir::new().unwrap();
        let missing = dir.path().join("proc_version_missing");
        assert!(!proc_version_file_indicates_wsl(&missing));
    }
}

#[cfg(test)]
mod batch_install_tests {
    use super::*;

    #[test]
    fn empty_list_returns_empty_result() {
        let result = PkgOps::batch_install(PackageManager::Brew, &[], None);
        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.succeeded.is_empty());
        assert!(result.failed.is_empty());
    }

    #[test]
    fn batch_install_result_default() {
        let result = BatchInstallResult {
            succeeded: vec!["foo".to_string()],
            failed: vec![("bar".to_string(), "error".to_string())],
        };
        assert_eq!(result.succeeded.len(), 1);
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.succeeded[0], "foo");
        assert_eq!(result.failed[0].0, "bar");
    }

    #[test]
    fn package_manager_has_required_traits() {
        // Test that PackageManager can be used as HashMap key
        let mut map = std::collections::HashMap::new();
        map.insert(PackageManager::Brew, vec!["jq", "ripgrep"]);
        map.insert(PackageManager::Apt, vec!["git", "curl"]);
        assert_eq!(map.len(), 2);
        assert!(map.contains_key(&PackageManager::Brew));
        assert!(map.contains_key(&PackageManager::Apt));
    }

    #[test]
    fn package_manager_equality() {
        assert_eq!(PackageManager::Brew, PackageManager::Brew);
        assert_ne!(PackageManager::Brew, PackageManager::Apt);
        assert_eq!(PackageManager::BrewCask, PackageManager::BrewCask);
        assert_ne!(PackageManager::Winget, PackageManager::Choco);
    }
}

#[allow(dead_code)]
pub struct PkgOps {
    name: &'static str,
}

/// Result of a batch installation operation.
#[derive(Debug)]
pub struct BatchInstallResult {
    /// Packages that were successfully installed
    pub succeeded: Vec<String>,
    /// Packages that failed to install (package name, error message)
    pub failed: Vec<(String, String)>,
}

impl PkgOps {
    /// Install multiple packages in a single batch operation.
    ///
    /// This is more efficient than installing packages one-by-one because:
    /// - Single dependency resolution pass
    /// - Single lock acquisition
    /// - Package manager can optimize internally
    ///
    /// Returns a BatchInstallResult indicating which packages succeeded/failed.
    /// On batch failure, individual packages are retried.
    pub fn batch_install(
        pm: PackageManager,
        packages: &[&str],
        use_sudo: Option<bool>,
    ) -> Result<BatchInstallResult, InstallError> {
        if packages.is_empty() {
            return Ok(BatchInstallResult {
                succeeded: vec![],
                failed: vec![],
            });
        }

        // Single package: use regular install
        if packages.len() == 1 {
            match Self::install(pm, packages[0], use_sudo) {
                Ok(()) => {
                    return Ok(BatchInstallResult {
                        succeeded: vec![packages[0].to_string()],
                        failed: vec![],
                    });
                }
                Err(e) => {
                    return Ok(BatchInstallResult {
                        succeeded: vec![],
                        failed: vec![(packages[0].to_string(), format!("{}", e))],
                    });
                }
            }
        }

        // Try batch install first
        let batch_result = Self::try_batch_install(pm, packages, use_sudo);

        match batch_result {
            Ok(()) => {
                // All packages installed successfully
                Ok(BatchInstallResult {
                    succeeded: packages.iter().map(|s| s.to_string()).collect(),
                    failed: vec![],
                })
            }
            Err(_) => {
                // Batch failed, retry individually to find which packages failed
                let mut succeeded = Vec::with_capacity(packages.len());
                let mut failed = Vec::with_capacity(packages.len());

                for pkg in packages {
                    match Self::install(pm, pkg, use_sudo) {
                        Ok(()) => succeeded.push(pkg.to_string()),
                        Err(e) => failed.push((pkg.to_string(), format!("{}", e))),
                    }
                }

                Ok(BatchInstallResult { succeeded, failed })
            }
        }
    }

    /// Attempt to install multiple packages in a single command.
    /// Returns Ok if all packages installed, Err if the command failed.
    fn try_batch_install(
        pm: PackageManager,
        packages: &[&str],
        use_sudo: Option<bool>,
    ) -> Result<(), InstallError> {
        match pm {
            PackageManager::Apt => {
                let apt = require_any(&["apt-get", "apt"], "apt is required to install packages")?;
                let mut args = vec!["install", "-y"];
                args.extend(packages);
                Self::run_with_sudo_strategy(use_sudo, apt, &args)
            }
            PackageManager::Dnf => {
                require("dnf", "dnf is required to install packages")?;
                let mut args = vec!["install", "-y"];
                args.extend(packages);
                Self::run_with_sudo_strategy(use_sudo, "dnf", &args)
            }
            PackageManager::Yum => {
                require("yum", "yum is required to install packages")?;
                let mut args = vec!["install", "-y"];
                args.extend(packages);
                Self::run_with_sudo_strategy(use_sudo, "yum", &args)
            }
            PackageManager::Zypper => {
                require("zypper", "zypper is required to install packages")?;
                let mut args = vec!["--non-interactive", "install", "--no-confirm"];
                args.extend(packages);
                Self::run_with_sudo_strategy(use_sudo, "zypper", &args)
            }
            PackageManager::Pacman => {
                require("pacman", "pacman is required to install packages")?;
                let mut args = vec!["--noconfirm", "-S"];
                args.extend(packages);
                Self::run_with_sudo_strategy(use_sudo, "pacman", &args)
            }
            PackageManager::Apk => {
                require("apk", "apk is required to install packages")?;
                let mut args = vec!["add"];
                args.extend(packages);
                Self::run_with_sudo_strategy(use_sudo, "apk", &args)
            }
            PackageManager::Brew => {
                require("brew", "Homebrew is required to install packages")?;
                let mut args = vec!["install"];
                args.extend(packages);
                run("brew", &args)?;
                Ok(())
            }
            PackageManager::Winget => {
                // winget doesn't support true batch install, but we can chain commands
                // For now, install sequentially since winget is internally sequential anyway
                require("winget", "Winget is required to install packages")?;
                for pkg in packages {
                    run("winget", &["install", "-e", "--id", pkg])?;
                }
                Ok(())
            }
            PackageManager::BrewCask => {
                require("brew", "Homebrew is required to install casks")?;
                let mut args = vec!["install", "--cask"];
                args.extend(packages);
                run("brew", &args)?;
                Ok(())
            }
            PackageManager::Choco => {
                crate::tools::chocolatey::ensure_installed()?;
                let mut args = vec!["install", "-y"];
                args.extend(packages);
                run("choco", &args)?;
                Ok(())
            }
            PackageManager::Pkg => {
                require("pkg", "FreeBSD pkg is required to install packages")?;
                let mut args = vec!["install", "-y"];
                args.extend(packages);
                Self::run_with_sudo_strategy(use_sudo, "pkg", &args)
            }
        }
    }

    /// Helper to run a command with sudo fallback strategy.
    fn run_with_sudo_strategy(
        use_sudo: Option<bool>,
        cmd: &str,
        args: &[&str],
    ) -> Result<(), InstallError> {
        match use_sudo {
            Some(flag) => {
                if flag {
                    require("sudo", "sudo is required to install packages")?;
                }
                run_maybe_sudo(flag, cmd, args)?;
                Ok(())
            }
            None => {
                if let Err(e) = run_maybe_sudo(false, cmd, args) {
                    if has("sudo") {
                        run_maybe_sudo(true, cmd, args)?;
                        Ok(())
                    } else {
                        Err(e)
                    }
                } else {
                    Ok(())
                }
            }
        }
    }

    /// Install packages using Homebrew cask (for GUI apps) in batch.
    pub fn batch_install_cask(packages: &[&str]) -> Result<BatchInstallResult, InstallError> {
        if packages.is_empty() {
            return Ok(BatchInstallResult {
                succeeded: vec![],
                failed: vec![],
            });
        }

        require("brew", "Homebrew is required to install casks")?;

        let mut args = vec!["install", "--cask"];
        args.extend(packages);

        match run("brew", &args) {
            Ok(_) => Ok(BatchInstallResult {
                succeeded: packages.iter().map(|s| s.to_string()).collect(),
                failed: vec![],
            }),
            Err(_) => {
                // Retry individually
                let mut succeeded = Vec::with_capacity(packages.len());
                let mut failed = Vec::with_capacity(packages.len());
                for pkg in packages {
                    match run("brew", &["install", "--cask", pkg]) {
                        Ok(_) => succeeded.push(pkg.to_string()),
                        Err(e) => failed.push((pkg.to_string(), format!("{}", e))),
                    }
                }
                Ok(BatchInstallResult { succeeded, failed })
            }
        }
    }

    /// Install packages using Chocolatey in batch.
    pub fn batch_install_choco(packages: &[&str]) -> Result<BatchInstallResult, InstallError> {
        if packages.is_empty() {
            return Ok(BatchInstallResult {
                succeeded: vec![],
                failed: vec![],
            });
        }

        crate::tools::chocolatey::ensure_installed()?;

        let mut args = vec!["install", "-y"];
        args.extend(packages);

        match run("choco", &args) {
            Ok(_) => Ok(BatchInstallResult {
                succeeded: packages.iter().map(|s| s.to_string()).collect(),
                failed: vec![],
            }),
            Err(_) => {
                // Retry individually
                let mut succeeded = Vec::with_capacity(packages.len());
                let mut failed = Vec::with_capacity(packages.len());
                for pkg in packages {
                    match run("choco", &["install", "-y", pkg]) {
                        Ok(_) => succeeded.push(pkg.to_string()),
                        Err(e) => failed.push((pkg.to_string(), format!("{}", e))),
                    }
                }
                Ok(BatchInstallResult { succeeded, failed })
            }
        }
    }

    pub fn update(pm: PackageManager, use_sudo: Option<bool>) -> Result<(), InstallError> {
        match pm {
            PackageManager::Apt => {
                // Ensure prerequisites exist before attempting the update
                let apt = require_any(&["apt-get", "apt"], "apt is required to update packages")?;
                // Decide sudo strategy
                match use_sudo {
                    Some(flag) => {
                        if flag {
                            require("sudo", "sudo is required to update packages")?;
                        }
                        run_maybe_sudo(flag, apt, &["update"])?;
                    }
                    None => {
                        // Try without sudo first
                        if let Err(e) = run_maybe_sudo(false, apt, &["update"]) {
                            // Retry with sudo if available
                            if has("sudo") {
                                run_maybe_sudo(true, apt, &["update"])?;
                            } else {
                                return Err(e);
                            }
                        }
                    }
                }
            }
            PackageManager::Dnf => { /* dnf auto-refreshes; optional */ }
            PackageManager::Yum => { /* optional */ }
            PackageManager::Zypper => {
                require("zypper", "zypper is required to update packages")?;
                match use_sudo {
                    Some(flag) => {
                        if flag {
                            require("sudo", "sudo is required to update packages")?;
                        }
                        run_maybe_sudo(flag, "zypper", &["--non-interactive", "refresh"])?;
                    }
                    None => {
                        if let Err(e) =
                            run_maybe_sudo(false, "zypper", &["--non-interactive", "refresh"])
                        {
                            if has("sudo") {
                                run_maybe_sudo(true, "zypper", &["--non-interactive", "refresh"])?;
                            } else {
                                return Err(e);
                            }
                        }
                    }
                }
            }
            PackageManager::Pacman => {
                require("pacman", "pacman is required to update packages")?;
                match use_sudo {
                    Some(flag) => {
                        if flag {
                            require("sudo", "sudo is required to update packages")?;
                        }
                        run_maybe_sudo(flag, "pacman", &["-Sy"])?;
                    }
                    None => {
                        if let Err(e) = run_maybe_sudo(false, "pacman", &["-Sy"]) {
                            if has("sudo") {
                                run_maybe_sudo(true, "pacman", &["-Sy"])?;
                            } else {
                                return Err(e);
                            }
                        }
                    }
                }
            }
            PackageManager::Apk => { /* `apk add` refreshes on demand */ }
            PackageManager::Pkg => {
                require("pkg", "FreeBSD pkg is required to update packages")?;
                match use_sudo {
                    Some(flag) => {
                        if flag {
                            require("sudo", "sudo is required to update packages")?;
                        }
                        run_maybe_sudo(flag, "pkg", &["update"])?;
                    }
                    None => {
                        if let Err(e) = run_maybe_sudo(false, "pkg", &["update"]) {
                            if has("sudo") {
                                run_maybe_sudo(true, "pkg", &["update"])?;
                            } else {
                                return Err(e);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub fn install(
        pm: PackageManager,
        pkg: &str,
        use_sudo: Option<bool>,
    ) -> Result<(), InstallError> {
        match pm {
            PackageManager::Apt => {
                let apt = require_any(&["apt-get", "apt"], "apt is required to install packages")?;
                match use_sudo {
                    Some(flag) => {
                        if flag {
                            require("sudo", "sudo is required to install packages")?;
                        }
                        run_maybe_sudo(flag, apt, &["install", "-y", pkg])?;
                    }
                    None => {
                        if let Err(e) = run_maybe_sudo(false, apt, &["install", "-y", pkg]) {
                            if has("sudo") {
                                run_maybe_sudo(true, apt, &["install", "-y", pkg])?;
                            } else {
                                return Err(e);
                            }
                        }
                    }
                }
            }
            PackageManager::Dnf => {
                require("dnf", "dnf is required to install packages")?;
                match use_sudo {
                    Some(flag) => {
                        if flag {
                            require("sudo", "sudo is required to install packages")?;
                        }
                        run_maybe_sudo(flag, "dnf", &["install", "-y", pkg])?;
                    }
                    None => {
                        if let Err(e) = run_maybe_sudo(false, "dnf", &["install", "-y", pkg]) {
                            if has("sudo") {
                                run_maybe_sudo(true, "dnf", &["install", "-y", pkg])?;
                            } else {
                                return Err(e);
                            }
                        }
                    }
                }
            }
            PackageManager::Yum => {
                require("yum", "yum is required to install packages")?;
                match use_sudo {
                    Some(flag) => {
                        if flag {
                            require("sudo", "sudo is required to install packages")?;
                        }
                        run_maybe_sudo(flag, "yum", &["install", "-y", pkg])?;
                    }
                    None => {
                        if let Err(e) = run_maybe_sudo(false, "yum", &["install", "-y", pkg]) {
                            if has("sudo") {
                                run_maybe_sudo(true, "yum", &["install", "-y", pkg])?;
                            } else {
                                return Err(e);
                            }
                        }
                    }
                }
            }
            PackageManager::Zypper => {
                require("zypper", "zypper is required to install packages")?;
                match use_sudo {
                    Some(flag) => {
                        if flag {
                            require("sudo", "sudo is required to install packages")?;
                        }
                        run_maybe_sudo(
                            flag,
                            "zypper",
                            &["--non-interactive", "install", "--no-confirm", pkg],
                        )?;
                    }
                    None => {
                        if let Err(e) = run_maybe_sudo(
                            false,
                            "zypper",
                            &["--non-interactive", "install", "--no-confirm", pkg],
                        ) {
                            if has("sudo") {
                                run_maybe_sudo(
                                    true,
                                    "zypper",
                                    &["--non-interactive", "install", "--no-confirm", pkg],
                                )?;
                            } else {
                                return Err(e);
                            }
                        }
                    }
                }
            }
            PackageManager::Pacman => {
                require("pacman", "pacman is required to install packages")?;
                match use_sudo {
                    Some(flag) => {
                        if flag {
                            require("sudo", "sudo is required to install packages")?;
                        }
                        run_maybe_sudo(flag, "pacman", &["--noconfirm", "-S", pkg])?;
                    }
                    None => {
                        if let Err(e) = run_maybe_sudo(false, "pacman", &["--noconfirm", "-S", pkg])
                        {
                            if has("sudo") {
                                run_maybe_sudo(true, "pacman", &["--noconfirm", "-S", pkg])?;
                            } else {
                                return Err(e);
                            }
                        }
                    }
                }
            }
            PackageManager::Apk => {
                require("apk", "apk is required to install packages")?;
                match use_sudo {
                    Some(flag) => {
                        if flag {
                            require("sudo", "sudo is required to install packages")?;
                        }
                        run_maybe_sudo(flag, "apk", &["add", pkg])?;
                    }
                    None => {
                        if let Err(e) = run_maybe_sudo(false, "apk", &["add", pkg]) {
                            if has("sudo") {
                                run_maybe_sudo(true, "apk", &["add", pkg])?;
                            } else {
                                return Err(e);
                            }
                        }
                    }
                }
            }
            // These package managers generally do not require sudo by design here
            PackageManager::Brew => {
                require("brew", "Homebrew is required to install packages")?;
                run("brew", &["install", pkg])?;
            }
            PackageManager::BrewCask => {
                require("brew", "Homebrew is required to install casks")?;
                run("brew", &["install", "--cask", pkg])?;
            }
            PackageManager::Winget => {
                require("winget", "Winget is required to install packages")?;
                run("winget", &["install", "-e", "--id", pkg])?;
            }
            PackageManager::Choco => {
                crate::tools::chocolatey::ensure_installed()?;
                run("choco", &["install", "-y", pkg])?;
            }
            PackageManager::Pkg => {
                require("pkg", "FreeBSD pkg is required to install packages")?;
                match use_sudo {
                    Some(flag) => {
                        if flag {
                            require("sudo", "sudo is required to install packages")?;
                        }
                        run_maybe_sudo(flag, "pkg", &["install", "-y", pkg])?;
                    }
                    None => {
                        if let Err(e) = run_maybe_sudo(false, "pkg", &["install", "-y", pkg]) {
                            if has("sudo") {
                                run_maybe_sudo(true, "pkg", &["install", "-y", pkg])?;
                            } else {
                                return Err(e);
                            }
                        }
                    }
                }
            }
        };
        Ok(())
    }
}

/// Doctor false-positive regression tests: version probing must tolerate
/// non-zero exits (talosctl/argocd print the client version then exit
/// non-zero when their server is unreachable) and `version`-subcommand-only
/// CLIs (cue). Fake shell scripts at unique absolute paths dodge the
/// process-wide caches.
#[cfg(all(test, unix))]
mod version_probe_tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    fn fake_cli(dir: &tempfile::TempDir, name: &str, script: &str) -> String {
        let path = dir.path().join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        write!(f, "#!/bin/sh\n{script}").unwrap();
        f.sync_all().unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.to_string_lossy().into_owned()
    }

    // Serialized with `install_via_cargo_install_prereq_when_cargo_absent`
    // (below in this module tree) — that test empties $PATH globally for
    // its duration, which races with the fake-CLI spawn here under
    // parallel test runners and made this test flake reliably in the
    // main.rs `[[bin]]` unittest binary under `cargo llvm-cov`.
    #[test]
    #[serial_test::serial]
    fn nonzero_exit_version_output_still_parses() {
        let dir = tempfile::tempdir().unwrap();
        // argocd-style: prints version, exits non-zero (server unreachable)
        let cli = fake_cli(&dir, "fake-argocd", "echo \"fake: v9.9.9\"\nexit 20\n");
        let out = cmd_version_output(&cli).expect("output despite non-zero exit");
        assert!(out.contains("v9.9.9"));
        assert!(cmd_satisfies(&cli, "9.9"), "9.9.9 must satisfy 9.9");
        assert!(has(&cli), "binary spawned → present, exit code irrelevant");
    }

    #[test]
    #[serial_test::serial]
    fn version_subcommand_only_cli_is_probed() {
        let dir = tempfile::tempdir().unwrap();
        // cue-style: --version / -V fail, only `version` subcommand answers
        let cli = fake_cli(
            &dir,
            "fake-cue",
            "if [ \"$1\" = \"version\" ]; then echo \"fake version v0.16.1\"; exit 0; fi\n\
             echo \"unknown flag\" >&2\nexit 1\n",
        );
        assert!(cmd_satisfies(&cli, "~0.16"), "0.16.1 must satisfy ~0.16");
    }

    #[test]
    fn missing_binary_returns_none() {
        assert_eq!(
            cmd_version_output("/nonexistent/definitely-not-a-cli"),
            None
        );
    }
}
