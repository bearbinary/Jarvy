//! Unified telemetry module for Jarvy
//!
//! This module provides a single API for all telemetry: logs, metrics, and traces.
//! It replaces both PostHog analytics and the limited OTEL setup in analytics.rs.
//!
//! ## Configuration
//!
//! Telemetry is opt-out and enabled by default. Configure via:
//! - `~/.jarvy/config.toml` [telemetry] section
//! - Environment variables (JARVY_TELEMETRY, JARVY_OTLP_ENDPOINT, etc.)
//!
//! ## Events
//!
//! Use the event functions to emit structured telemetry:
//! - `tool_requested()`, `tool_installed()`, `tool_failed()`, `tool_not_supported()`

#![allow(dead_code)] // Public API for telemetry - many functions reserved for future use
//! - `hook_started()`, `hook_completed()`, `hook_failed()`
//! - `command_executed()`, `setup_completed()`

use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Gauge, Histogram, MeterProvider};
use opentelemetry_otlp::{Protocol, WithExportConfig};
use opentelemetry_sdk::metrics::SdkMeterProvider;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::path::Path;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

// ============================================================================
// Configuration
// ============================================================================

/// Telemetry configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct TelemetryConfig {
    /// Master switch for telemetry. Default is `true` (opt-out).
    /// Users disable with `jarvy telemetry disable` (persistent),
    /// `JARVY_TELEMETRY=0` (per-invocation), or by setting
    /// `[telemetry] enabled = false` in `~/.jarvy/config.toml`.
    /// The first-run notice in `src/init.rs` makes the choice visible.
    pub enabled: bool,
    /// OTLP endpoint URL. Default is the project's hardened public
    /// forwarder — only reached if the user actually opts in. See
    /// `docs/operations/telemetry-forwarder.md` for how the forwarder
    /// is provisioned, the security model (TLS, rate limits, PII
    /// scrubbing), and the fan-out to Grafana Cloud.
    pub endpoint: String,
    /// Protocol: "http" or "grpc" (default: "http")
    pub protocol: String,
    /// Enable log export (default: true when telemetry enabled)
    pub logs: bool,
    /// Enable metrics export (default: true when telemetry enabled)
    pub metrics: bool,
    /// Enable trace export (default: false)
    pub traces: bool,
    /// Trace sampling rate 0.0-1.0 (default: 1.0)
    pub sample_rate: f64,
    /// Custom resource attributes
    #[serde(default)]
    pub resource: HashMap<String, String>,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            // Opt-out: telemetry is on by default. Documented in
            // CLAUDE.md and surfaced as a loud first-run notice in
            // src/init.rs. Users opt out with `jarvy telemetry disable`,
            // `JARVY_TELEMETRY=0`, or `[telemetry] enabled = false` in
            // `~/.jarvy/config.toml`.
            enabled: true,
            // Default endpoint is the project's hardened public OTLP
            // forwarder. Only reached if the user opts in.
            // Provisioning, security model, and data-handling policy
            // live in docs/operations/telemetry-forwarder.md.
            // Override per-install with `JARVY_OTLP_ENDPOINT`.
            endpoint: "https://telemetry.jarvy.dev".to_string(),
            protocol: "http".to_string(),
            logs: true,
            metrics: true,
            traces: false,
            sample_rate: 1.0,
            resource: HashMap::new(),
        }
    }
}

impl TelemetryConfig {
    /// Apply a project-level `[telemetry]` table on top of an existing
    /// (typically user-global + env) config, enforcing the trust-boundary
    /// rule: a project `jarvy.toml` arriving via `git clone <untrusted>`
    /// may **narrow** the user's telemetry posture but never **broaden** it.
    ///
    /// Narrowing is allowed (and applied):
    /// - `enabled = false` disables telemetry for this run
    /// - per-signal flags (`logs` / `metrics` / `traces`) AND with the
    ///   target — project can turn a signal off, never on
    /// - `sample_rate` takes `.min()` — project can reduce, never raise
    ///
    /// Broadening is refused (and reported back):
    /// - the project's `enabled = true` does NOT enable when the target
    ///   is disabled; only `JARVY_TELEMETRY=1` (explicit env consent)
    ///   or a global config change can do that
    /// - an endpoint override that differs from both the system default
    ///   AND the current target returns a sanitized warning string;
    ///   callers print it to stderr so the user knows the project's
    ///   intent was visible but ignored
    ///
    /// Returns `Some(message)` when a warning should be surfaced,
    /// otherwise `None`. The message is pre-sanitized for safe display
    /// (control bytes stripped) — the project's endpoint string is
    /// attacker-controlled when the project came from an untrusted
    /// repo, so embedding it raw would let a malicious project clear
    /// the terminal and forge fake Jarvy output.
    pub fn narrow_with_project(&mut self, project: &TelemetryConfig) -> Option<String> {
        // Allow narrowing: project may disable.
        if !project.enabled {
            self.enabled = false;
        }
        // Allow narrowing per-signal: AND with the target so any "off"
        // wins. Project cannot turn a signal on when the target has it
        // off.
        self.logs = self.logs && project.logs;
        self.metrics = self.metrics && project.metrics;
        self.traces = self.traces && project.traces;
        // Sample rate: take the minimum so the project can only reduce.
        // NaN-safe: `f64::min` returns the non-NaN argument when one is
        // NaN, so a malformed config can't poison the rate.
        self.sample_rate = self.sample_rate.min(project.sample_rate);

        // Refuse endpoint overrides — return a warning so callers can
        // print it. Compare against both the system default and the
        // current target so a project that happens to echo the user's
        // already-set endpoint doesn't trigger a spurious message.
        let default_endpoint = TelemetryConfig::default().endpoint;
        if project.endpoint != default_endpoint && project.endpoint != self.endpoint {
            // Sanitize the project's endpoint before embedding —
            // attacker-controlled input must not reach stderr raw.
            // Defer to the tool-name sanitizer; the rules (strip C0/C1
            // and Unicode tricks, length-cap) apply equally well to a
            // URL display value.
            let safe = crate::tools::unsupported::sanitize_for_display(&project.endpoint);
            return Some(format!(
                "[jarvy] project jarvy.toml requests non-default telemetry endpoint ({}). \
                 Refusing without explicit consent. Inspect jarvy.toml, then re-run with \
                 JARVY_OTLP_ENDPOINT set to your chosen endpoint (do not copy the value \
                 above blindly).",
                safe
            ));
        }
        None
    }

    /// Load config from environment variables, overriding defaults
    pub fn from_env() -> Self {
        let mut config = Self::default();

        // Master switch
        if let Ok(v) = env::var("JARVY_TELEMETRY") {
            config.enabled = matches!(v.as_str(), "1" | "true" | "yes");
        }

        // Endpoint
        if let Ok(v) = env::var("JARVY_OTLP_ENDPOINT")
            && !v.trim().is_empty()
        {
            config.endpoint = v;
        }

        // Protocol
        if let Ok(v) = env::var("JARVY_OTLP_PROTOCOL") {
            config.protocol = v;
        }

        // Signal toggles
        if let Ok(v) = env::var("JARVY_OTLP_LOGS") {
            config.logs = matches!(v.as_str(), "1" | "true" | "yes");
        }
        if let Ok(v) = env::var("JARVY_OTLP_METRICS") {
            config.metrics = matches!(v.as_str(), "1" | "true" | "yes");
        }
        if let Ok(v) = env::var("JARVY_OTLP_TRACES") {
            config.traces = matches!(v.as_str(), "1" | "true" | "yes");
        }

        // Sample rate
        if let Ok(v) = env::var("JARVY_OTLP_SAMPLE_RATE")
            && let Ok(rate) = v.parse::<f64>()
        {
            config.sample_rate = rate.clamp(0.0, 1.0);
        }

        // Unattended-mode auto-disable: covers CI runners AND modern
        // AI agent sandboxes (Codespaces, Claude Code, Cursor, e2b,
        // etc.). See `crate::sandbox::is_seamless_auto` and PRD-053.
        //
        // Uses the `_auto` variant: a hostile dotfile or compromised
        // devcontainer image that sets `JARVY_SANDBOX=1` should not
        // silently silence telemetry on a victim's machine. Forced
        // sandbox requires explicit `JARVY_TELEMETRY=0`. If
        // `JARVY_TELEMETRY` is set, the user's choice wins either way.
        if crate::sandbox::is_seamless_auto() && env::var("JARVY_TELEMETRY").is_err() {
            config.enabled = false;
        }

        config
    }

    /// Check if telemetry is effectively enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled && (self.logs || self.metrics || self.traces)
    }

    /// Returns `Ok(())` if the configured endpoint is acceptable, otherwise
    /// an explanatory error. Plain `http://` is rejected outside loopback so
    /// telemetry payloads (tool inventory, fingerprint, error stderr) do not
    /// leak to a passive listener on the network.
    pub fn validate_endpoint(&self) -> Result<(), String> {
        let url = self.endpoint.trim();
        if url.is_empty() {
            return Err("telemetry endpoint is empty".to_string());
        }
        if url.starts_with("https://") {
            return Ok(());
        }
        if url.starts_with("http://") {
            // Allow loopback only. Strip scheme + path and any IPv4 port,
            // and unwrap bracketed IPv6 literals (`http://[::1]:4318`).
            let host_with_port = url
                .trim_start_matches("http://")
                .split('/')
                .next()
                .unwrap_or("");
            let host_only = if let Some(rest) = host_with_port.strip_prefix('[') {
                // IPv6 literal: take chars up to closing bracket.
                rest.split(']').next().unwrap_or("")
            } else {
                host_with_port.split(':').next().unwrap_or("")
            };
            const LOOPBACK: &[&str] = &["localhost", "127.0.0.1", "::1"];
            if LOOPBACK.contains(&host_only) {
                return Ok(());
            }
            return Err(format!(
                "telemetry endpoint `{url}` uses plain HTTP to a non-loopback host; \
                 use https:// or set the endpoint to localhost"
            ));
        }
        Err(format!(
            "telemetry endpoint `{url}` must use http:// (loopback only) or https://"
        ))
    }
}

// ============================================================================
// Source enum for event tracking
// ============================================================================

/// Source of a tool request
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// From jarvy.toml config file
    Config,
    /// From MCP server request
    Mcp,
    /// From CLI argument
    Cli,
    /// User explicitly invoked `jarvy tools --request <name>`. Treated
    /// as direct consent — the telemetry consent gate is bypassed because this
    /// command's whole purpose is to file a request. The GitHub issue
    /// URL printed alongside remains the canonical channel.
    Request,
    /// User ran `jarvy diagnose <name>` for an unregistered tool.
    Diagnose,
    /// User ran `jarvy doctor --tools <name>` for an unregistered tool.
    Doctor,
    /// User ran `jarvy search <name>` and no results were returned.
    Search,
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::Config => write!(f, "config"),
            Source::Mcp => write!(f, "mcp"),
            Source::Cli => write!(f, "cli"),
            Source::Request => write!(f, "request"),
            Source::Diagnose => write!(f, "diagnose"),
            Source::Doctor => write!(f, "doctor"),
            Source::Search => write!(f, "search"),
        }
    }
}

// ============================================================================
// Global State
// ============================================================================

static TELEMETRY: OnceLock<TelemetryState> = OnceLock::new();

struct TelemetryState {
    config: TelemetryConfig,
    meter_provider: Option<SdkMeterProvider>,
    metrics: Option<Metrics>,
}

struct Metrics {
    tool_requests: Counter<u64>,
    tool_installs: Counter<u64>,
    tool_not_supported: Counter<u64>,
    errors: Counter<u64>,
    hooks_executions: Counter<u64>,
    commands: Counter<u64>,
    install_duration: Histogram<f64>,
    setup_duration: Histogram<f64>,
    hooks_duration: Histogram<f64>,
    commands_duration: Histogram<f64>,
    setup_inventory_size: Gauge<u64>,
    maintenance_stale_tools: Gauge<u64>,
    /// Per-backend failure counter (PRD-057 observability F1). Labels:
    /// `backend` ∈ 14 known backends, `error_kind` from the bounded
    /// [`BackendError::kind`] set. Enables "which backend is
    /// breaking?" on-call graphs without pivoting on report stdout.
    maintenance_backend_failures: Counter<u64>,
    /// Per-probe duration histogram (PRD-057 observability F2). Labels:
    /// `backend`, `outcome` ∈ {ok, not_found, manager_missing,
    /// timeout, parse_failed, permission_denied, other}. Gives on-call
    /// the p99 latency contribution of each backend when setup summary
    /// or refresh takes longer than expected.
    maintenance_probe_duration: Histogram<f64>,
}

// ============================================================================
// Initialization
// ============================================================================

/// Initialize telemetry with the given configuration
pub fn init(config: TelemetryConfig) {
    // Mirror the consent state into the lib-visible gate so modules
    // declared by `lib.rs` (`src/packages/*`, etc.) can check the
    // consent flag without reaching `crate::telemetry::is_enabled`,
    // which is bin-only. See `observability::telemetry_gate` for the
    // visibility-wall rationale.
    let enabled = config.is_enabled();
    let _ = TELEMETRY.set(build_telemetry_state(config));
    crate::observability::telemetry_gate::set_enabled(enabled);
    // Install the lib-visible emitters that route to the bin-side
    // OTel handles (PRD-057 obs F1 / F2). See
    // `observability::maintenance_metrics` for the seam.
    crate::observability::maintenance_metrics::set_backend_failed_emitter(
        maintenance_backend_failed,
    );
    crate::observability::maintenance_metrics::set_backend_ok_emitter(maintenance_backend_probed);
}

/// Initialize telemetry from environment variables
pub fn init_from_env() {
    init(TelemetryConfig::from_env());
}

fn build_telemetry_state(config: TelemetryConfig) -> TelemetryState {
    if !config.is_enabled() {
        return TelemetryState {
            config,
            meter_provider: None,
            metrics: None,
        };
    }

    // Validate the OTLP endpoint before we wire any exporters. Refuse plain
    // HTTP to non-loopback hosts so a stale config / `direnv` override
    // pointing at `http://attacker.tld` cannot silently leak the payloads
    // (full tool inventory, fingerprint, error stderr) onto the wire.
    if let Err(why) = config.validate_endpoint() {
        // Promoted to error! (round-2 obs F20): endpoint refusal is a
        // security event (operator pointed telemetry at plain HTTP /
        // unknown scheme), and the only reachable sink is the local
        // console — OTLP itself is what got refused.
        tracing::error!(
            event = "telemetry.endpoint.refused",
            reason = %why,
            "disabling telemetry: configured endpoint rejected"
        );
        let mut disabled = config;
        disabled.enabled = false;
        return TelemetryState {
            config: disabled,
            meter_provider: None,
            metrics: None,
        };
    }

    // Build meter provider for metrics
    let (meter_provider, metrics) = if config.metrics {
        match build_meter_provider(&config) {
            Ok(provider) => {
                let meter = provider.meter("jarvy");
                let metrics = Metrics {
                    tool_requests: meter
                        .u64_counter("jarvy.tool.requests")
                        .with_description("Number of tool installation requests")
                        .build(),
                    tool_installs: meter
                        .u64_counter("jarvy.tool.installs")
                        .with_description("Number of tool installations by status")
                        .build(),
                    tool_not_supported: meter
                        .u64_counter("jarvy.tool.unsupported")
                        .with_description(
                            "Number of unsupported tool requests \
                             (one per `tool.unsupported` event, sourced from \
                             config / mcp / cli / request)",
                        )
                        .build(),
                    errors: meter
                        .u64_counter("jarvy.errors")
                        .with_description("Number of errors by type")
                        .build(),
                    hooks_executions: meter
                        .u64_counter("jarvy.hooks.executions")
                        .with_description("Number of hook executions by type and status")
                        .build(),
                    commands: meter
                        .u64_counter("jarvy.commands")
                        .with_description("Number of command executions")
                        .build(),
                    install_duration: meter
                        .f64_histogram("jarvy.install.duration")
                        .with_description("Tool installation duration in seconds")
                        .with_unit("s")
                        .build(),
                    setup_duration: meter
                        .f64_histogram("jarvy.setup.duration")
                        .with_description("Total setup duration in seconds")
                        .with_unit("s")
                        .build(),
                    hooks_duration: meter
                        .f64_histogram("jarvy.hooks.duration")
                        .with_description("Hook execution duration in seconds")
                        .with_unit("s")
                        .build(),
                    commands_duration: meter
                        .f64_histogram("jarvy.commands.duration")
                        .with_description("Command execution duration in seconds")
                        .with_unit("s")
                        .build(),
                    setup_inventory_size: meter
                        .u64_gauge("jarvy.setup.inventory_size")
                        .with_description(
                            "Number of tools in the provisioning inventory (security audit)",
                        )
                        .build(),
                    maintenance_stale_tools: meter
                        .u64_gauge("jarvy.maintenance.stale_tools")
                        .with_description(
                            "Number of provisioner-tracked tools with newer upstream versions available (PRD-057)",
                        )
                        .build(),
                    maintenance_backend_failures: meter
                        .u64_counter("jarvy.maintenance.backend_failures")
                        .with_description(
                            "Backend probe failures by backend and error_kind (PRD-057 obs F1)",
                        )
                        .build(),
                    maintenance_probe_duration: meter
                        .f64_histogram("jarvy.maintenance.probe_duration")
                        .with_description(
                            "Backend probe duration in seconds, by backend and outcome (PRD-057 obs F2)",
                        )
                        .with_unit("s")
                        .build(),
                };
                (Some(provider), Some(metrics))
            }
            Err(e) => {
                tracing::warn!("Failed to initialize metrics: {}", e);
                (None, None)
            }
        }
    } else {
        (None, None)
    };

    TelemetryState {
        config,
        meter_provider,
        metrics,
    }
}

fn build_meter_provider(config: &TelemetryConfig) -> Result<SdkMeterProvider, String> {
    // Compose `/v1/metrics` onto the base endpoint. `opentelemetry-otlp`
    // 0.31's `with_endpoint()` is the FULL URL — a bare base produces
    // `POST /` and the collector 404s every export.
    let endpoint = crate::analytics::resolve_otlp_endpoint(&config.endpoint, "metrics");
    let exporter = opentelemetry_otlp::MetricExporter::builder()
        .with_http()
        .with_protocol(Protocol::HttpBinary)
        .with_endpoint(endpoint.as_str())
        .build()
        .map_err(|e| format!("Failed to build metric exporter: {}", e))?;

    let reader = opentelemetry_sdk::metrics::PeriodicReader::builder(exporter)
        .with_interval(Duration::from_secs(60))
        .build();

    // Reuse the same resource as the logger provider so logs and metrics
    // carry matching identity (service.name, host.name, …) in the
    // backend. Without `with_resource` the SDK falls back to
    // `service.name=unknown_service` which (a) breaks Grafana stack
    // filtering and (b) bypasses the forwarder's `host.name` hash
    // statement entirely.
    Ok(SdkMeterProvider::builder()
        .with_reader(reader)
        .with_resource(crate::analytics::build_resource())
        .build())
}

/// Shutdown telemetry, flushing any pending data
pub fn shutdown() {
    if let Some(state) = TELEMETRY.get()
        && let Some(ref provider) = state.meter_provider
    {
        let _ = provider.shutdown();
    }
}

/// Check if telemetry is enabled
pub fn is_enabled() -> bool {
    TELEMETRY
        .get()
        .map(|s| s.config.is_enabled())
        .unwrap_or(false)
}

/// Get the current telemetry configuration
pub fn config() -> Option<&'static TelemetryConfig> {
    TELEMETRY.get().map(|s| &s.config)
}

/// Bounded sibling to the `platform` field: distinguishes a WSL distro from
/// bare-metal Linux, which `env::consts::OS` alone cannot.
pub fn env_kind() -> &'static str {
    if crate::tools::common::is_wsl() {
        "wsl"
    } else {
        "native"
    }
}

// ============================================================================
// Event Functions - Tool Operations
// ============================================================================

/// Record a tool installation request
pub fn tool_requested(tool: &str, version: &str, source: Source) {
    if !is_enabled() {
        return;
    }

    tracing::info!(
        event = "tool.requested",
        tool = %tool,
        version = %version,
        source = %source,
        platform = %env::consts::OS,
        env_kind = %env_kind(),
    );

    if let Some(state) = TELEMETRY.get()
        && let Some(ref metrics) = state.metrics
    {
        metrics.tool_requests.add(
            1,
            &[
                KeyValue::new("tool", tool.to_string()),
                KeyValue::new("source", source.to_string()),
                KeyValue::new("platform", env::consts::OS.to_string()),
                KeyValue::new("env_kind", env_kind().to_string()),
            ],
        );
    }
}

/// Record a successful tool installation
pub fn tool_installed(tool: &str, version: &str, package_manager: &str, duration: Duration) {
    if !is_enabled() {
        return;
    }

    let duration_ms = duration.as_millis() as u64;
    let category = crate::tools::spec::get_tool_category(tool).unwrap_or("uncategorized");
    // PRD-060: attribute ecosystem-fallback installs without changing
    // call-site signatures — the fallback runtime records the route it
    // took per tool; everything else is a platform install.
    let (install_route, toolchain_bootstrapped) = crate::tools::fallback::route_for(tool)
        .map_or(("platform", false), |(label, boot)| (label, boot));
    tracing::info!(
        event = "tool.installed",
        tool = %tool,
        version = %version,
        category = %category,
        package_manager = %package_manager,
        install_route = %install_route,
        toolchain_bootstrapped = %toolchain_bootstrapped,
        duration_ms = %duration_ms,
        platform = %env::consts::OS,
        env_kind = %env_kind(),
    );

    if let Some(state) = TELEMETRY.get()
        && let Some(ref metrics) = state.metrics
    {
        let attrs = [
            KeyValue::new("tool", tool.to_string()),
            KeyValue::new("pm", package_manager.to_string()),
            KeyValue::new("platform", env::consts::OS.to_string()),
            KeyValue::new("env_kind", env_kind().to_string()),
            KeyValue::new("status", "success"),
            KeyValue::new("category", category.to_string()),
            KeyValue::new("install_route", install_route),
        ];
        metrics.tool_installs.add(1, &attrs);
        // Histogram label set excludes `status` and `category` to
        // keep cardinality bounded; duration distribution by
        // tool+pm+platform is what dashboards want.
        metrics
            .install_duration
            .record(duration.as_secs_f64(), &attrs[..3]);
    }
}

/// Record that a tool was already installed (install skipped).
///
/// Emitted when `jarvy setup` discovers a tool is already present and
/// skips the install path. Used to measure how often interactive
/// prompts (e.g. "Do you want to install Oh My Zsh?") lead to no-op
/// outcomes so the flow can be optimized.
///
/// Fields carry enough context for an automated remediation system to
/// locate the wasteful check and propose a fix:
/// - `install_path`: where the tool was detected (e.g. `~/.oh-my-zsh`)
/// - `detection_method`: how presence was confirmed (`path_exists`,
///   `command_check`, `version_query`)
/// - `source`: call site identifier so fixes can be routed to the right
///   module (e.g. `check_zsh`, `install_homebrew`)
/// - `prompted_user`: whether the user was asked before the skip — the
///   primary signal for "stop nagging" remediation
///
/// Bumps the `jarvy.tool.installs` counter with `status="already_installed"`
/// rather than introducing a new counter — keeps tool-install volume
/// queryable from one metric.
pub fn tool_already_installed(
    tool: &str,
    install_path: &str,
    detection_method: &str,
    source: &str,
    prompted_user: bool,
) {
    if !is_enabled() {
        return;
    }

    // `install_path` arrives as `/Users/<name>/.oh-my-zsh` (or the
    // platform equivalent) — the OS username is identifying. Redact
    // home-dir prefix to `~` before emitting; the forwarder's
    // server-side scrub is defense in depth, not the contract.
    let redacted_path = redact_path(install_path);
    tracing::info!(
        event = "tool.already_installed",
        tool = %tool,
        install_path = %redacted_path,
        detection_method = %detection_method,
        source = %source,
        prompted_user = %prompted_user,
        platform = %env::consts::OS,
        env_kind = %env_kind(),
    );

    if let Some(state) = TELEMETRY.get()
        && let Some(ref metrics) = state.metrics
    {
        metrics.tool_installs.add(
            1,
            &[
                KeyValue::new("tool", tool.to_string()),
                KeyValue::new("platform", env::consts::OS.to_string()),
                KeyValue::new("env_kind", env_kind().to_string()),
                KeyValue::new("status", "already_installed"),
                KeyValue::new("detection_method", detection_method.to_string()),
                KeyValue::new("source", source.to_string()),
                KeyValue::new("prompted_user", prompted_user),
            ],
        );
    }
}

/// Record a failed tool installation
pub fn tool_failed(tool: &str, version: &str, error: &str) {
    tool_failed_with_kind(tool, version, "install_command_failed", error);
}

/// Record a failed tool installation with a stable `error_kind`
/// discriminant. Use this from call sites that have an `InstallError`
/// in scope (`InstallError::kind()`) so dashboards can group "tap
/// fetch failed" vs "permission required" vs the generic
/// "install_command_failed" without parsing free-text error strings.
pub fn tool_failed_with_kind(tool: &str, version: &str, error_kind: &str, error: &str) {
    if !is_enabled() {
        return;
    }

    let redacted_error = redact_sensitive(error);
    let category = crate::tools::spec::get_tool_category(tool).unwrap_or("uncategorized");
    // Same dimension as `tool.installed` so per-route failure rates are
    // computable; "platform" = the native package-manager path failed.
    let install_route = crate::tools::fallback::failed_route_for(tool).unwrap_or("platform");

    tracing::error!(
        event = "tool.failed",
        tool = %tool,
        version = %version,
        category = %category,
        error_kind = %error_kind,
        install_route = %install_route,
        error = %redacted_error,
        platform = %env::consts::OS,
        env_kind = %env_kind(),
    );

    if let Some(state) = TELEMETRY.get()
        && let Some(ref metrics) = state.metrics
    {
        metrics.tool_installs.add(
            1,
            &[
                KeyValue::new("tool", tool.to_string()),
                KeyValue::new("platform", env::consts::OS.to_string()),
                KeyValue::new("env_kind", env_kind().to_string()),
                KeyValue::new("status", "failed"),
                KeyValue::new("error_kind", error_kind.to_string()),
                KeyValue::new("category", category.to_string()),
            ],
        );
        metrics
            .errors
            .add(1, &[KeyValue::new("error_type", "tool_install")]);
    }
}

/// Record an unsupported tool request (counter only).
///
/// The structured `tool.unsupported` event is emitted by the caller
/// directly (`setup_cmd` and `tools_cmd`) so the event field set lives
/// in one place — see [Event Taxonomy in `CLAUDE.md`]. This function
/// just bumps the OTEL counter so dashboards can graph request volume.
///
/// Sanitizes the `tool` label via [`crate::tools::unsupported::sanitize_for_display`]
/// to strip ANSI/control bytes/bidi overrides and cap length. Without this,
/// the `[maintenance.refused_unsafe_name]` cardinality-bomb precedent would apply
/// — user-typed queries from `search`/`diagnose`/`doctor --tools` are unbounded
/// text on an OTLP counter label.
///
/// Respects the consent guard: no-op if telemetry is disabled.
pub fn tool_not_supported(tool: &str, _version: Option<&str>, source: Source) {
    if !is_enabled() {
        return;
    }

    let safe = crate::tools::unsupported::sanitize_for_display(tool);

    if let Some(state) = TELEMETRY.get()
        && let Some(ref metrics) = state.metrics
    {
        metrics.tool_not_supported.add(
            1,
            &[
                KeyValue::new("tool", safe.into_owned()),
                KeyValue::new("source", source.to_string()),
                KeyValue::new("platform", env::consts::OS.to_string()),
                KeyValue::new("env_kind", env_kind().to_string()),
            ],
        );
    }
}

/// Emit the `tool.unsupported` event + counter for a discovery-surface
/// miss (search/diagnose/doctor). Sanitizes the raw user input, gates
/// the tracing emission behind the telemetry consent, and uses the
/// narrow field shape documented in CLAUDE.md's Event Taxonomy for
/// probe sources (setup/`--request` use the wider `build_report`
/// shape).
///
/// Level is `info!`, not `warn!`: an unsupported-tool hit from an
/// interactive discovery surface is an ordinary user outcome, not a
/// system fault.
pub fn emit_tool_unsupported_probe(raw: &str, source: Source) {
    let safe = crate::tools::unsupported::sanitize_for_display(raw);
    if crate::observability::telemetry_gate::is_enabled() {
        tracing::info!(
            event = "tool.unsupported",
            tool = %safe,
            source = %source,
            platform = %env::consts::OS,
            env_kind = %env_kind(),
        );
    }
    tool_not_supported(&safe, None, source);
    #[cfg(test)]
    probe_test_sentinel::record(source);
}

/// Test-only fire-counter for `emit_tool_unsupported_probe`. Standing
/// up a full tracing subscriber harness for a 3-line wrapper is
/// disproportionate; a lock-free counter per source is enough to catch
/// the regression the QA review flagged (someone deletes the emit
/// block and the criterion-only test doesn't notice).
#[cfg(test)]
pub mod probe_test_sentinel {
    use super::Source;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static DIAGNOSE: AtomicUsize = AtomicUsize::new(0);
    static DOCTOR: AtomicUsize = AtomicUsize::new(0);
    static SEARCH: AtomicUsize = AtomicUsize::new(0);

    pub(super) fn record(source: Source) {
        let slot = match source {
            Source::Diagnose => &DIAGNOSE,
            Source::Doctor => &DOCTOR,
            Source::Search => &SEARCH,
            _ => return,
        };
        slot.fetch_add(1, Ordering::Relaxed);
    }

    pub fn take(source: Source) -> usize {
        let slot = match source {
            Source::Diagnose => &DIAGNOSE,
            Source::Doctor => &DOCTOR,
            Source::Search => &SEARCH,
            _ => return 0,
        };
        slot.swap(0, Ordering::Relaxed)
    }
}

/// Record an explicit user request via `jarvy tools --request <name>`.
///
/// Bypasses the [`is_enabled`] consent guard because the user typed the
/// command — consent is implicit. The metric increment is still gated
/// on `TELEMETRY` having been initialized with a metrics provider, so
/// users who explicitly disabled telemetry for the run don't get
/// network traffic; the request is still recorded locally because the
/// caller emits the `tool.unsupported` tracing event regardless.
///
/// Returns `true` when the counter actually fired, `false` when the
/// metric was dropped (telemetry never initialized). Callers can use
/// the result to decide whether the request needs the GitHub-URL
/// fallback — though the structured event always lands in the local
/// log file.
pub fn tool_request_explicit(tool: &str, _suggestions: &[String]) -> bool {
    if let Some(state) = TELEMETRY.get()
        && let Some(ref metrics) = state.metrics
    {
        metrics.tool_not_supported.add(
            1,
            &[
                KeyValue::new("tool", tool.to_string()),
                KeyValue::new("source", Source::Request.to_string()),
                KeyValue::new("platform", env::consts::OS.to_string()),
                KeyValue::new("env_kind", env_kind().to_string()),
            ],
        );
        return true;
    }
    false
}

// ============================================================================
// Event Functions - Setup Flow
// ============================================================================

/// Summary of a setup operation
#[derive(Debug, Clone, Default)]
pub struct SetupSummary {
    pub tools_requested: usize,
    pub tools_installed: usize,
    pub tools_skipped: usize,
    pub tools_failed: usize,
    pub hooks_run: usize,
    pub duration: Duration,
    /// Bounded `MaintenanceState::as_str()` label (PRD-057
    /// analytics F2). `None` means "setup didn't evaluate the
    /// freshness phase" (legacy `src/setup.rs` path). Emitted on
    /// `setup.completed` when present so PMs can compute
    /// `maintenance_ran / setup_completed` from one event stream.
    pub maintenance_state: Option<&'static str>,
}

/// Record setup started
pub fn setup_started(tools_count: usize) {
    if !is_enabled() {
        return;
    }

    tracing::info!(
        event = "setup.started",
        tools_count = %tools_count,
        platform = %env::consts::OS,
        env_kind = %env_kind(),
    );
}

/// Record setup completed
pub fn setup_completed(summary: &SetupSummary) {
    if !is_enabled() {
        return;
    }

    let duration_ms = summary.duration.as_millis() as u64;
    tracing::info!(
        event = "setup.completed",
        tools_requested = %summary.tools_requested,
        tools_installed = %summary.tools_installed,
        tools_skipped = %summary.tools_skipped,
        tools_failed = %summary.tools_failed,
        hooks_run = %summary.hooks_run,
        duration_ms = %duration_ms,
        maintenance_state = summary.maintenance_state.unwrap_or("not_evaluated"),
    );

    if let Some(state) = TELEMETRY.get()
        && let Some(ref metrics) = state.metrics
    {
        metrics.setup_duration.record(
            summary.duration.as_secs_f64(),
            &[KeyValue::new("tools_count", summary.tools_requested as i64)],
        );
    }
}

/// Record the complete tool inventory being provisioned (for security audit).
///
/// Emits a single structured log event with the full manifest of tools, versions,
/// machine hardware ID, hostname, and platform so security teams can audit
/// provisioning across the fleet via their OTEL-connected observability platform.
pub fn setup_inventory(
    tools: &[(String, String)],
    role: Option<&str>,
    config_source: &str,
    machine_id: Option<&str>,
) {
    if !is_enabled() {
        return;
    }

    let tools_str = tools
        .iter()
        .map(|(name, version)| format!("{}={}", name, version))
        .collect::<Vec<_>>()
        .join(",");

    // `hostname` is now attached as a `host.name` resource attribute via
    // `analytics::build_resource()` so the forwarder's anonymize stage
    // (which only operates on `context: resource`) catches it. Emitting
    // it as a per-event log-record field as well would bypass the hash
    // — Grafana would receive plaintext `hostname=Zacs-MacBook-Pro.local`
    // alongside the hashed `host.name`. Dropped here for that reason.
    tracing::info!(
        event = "setup.inventory",
        tools = %tools_str,
        tools_count = %tools.len(),
        role = %role.unwrap_or("none"),
        config_source = %redact_path(config_source),
        machine_id = %machine_id.unwrap_or("unknown"),
        platform = %env::consts::OS,
        env_kind = %env_kind(),
    );

    if let Some(state) = TELEMETRY.get()
        && let Some(ref metrics) = state.metrics
    {
        metrics.setup_inventory_size.record(
            tools.len() as u64,
            &[
                KeyValue::new("machine_id", machine_id.unwrap_or("unknown").to_string()),
                KeyValue::new("platform", env::consts::OS.to_string()),
                KeyValue::new("env_kind", env_kind().to_string()),
            ],
        );
    }
}

// ============================================================================
// Event Functions - Maintenance (PRD-057)
// ============================================================================

/// Record the freshness advisory's stale-tool gauge on
/// `maintenance.phase_completed`. Attributed by machine + platform
/// so a fleet dashboard can chart "how far behind is my
/// organization?" without needing per-project cardinality.
pub fn maintenance_stale_tools(count: u64, machine_id: Option<&str>) {
    if !is_enabled() {
        return;
    }
    if let Some(state) = TELEMETRY.get()
        && let Some(ref metrics) = state.metrics
    {
        metrics.maintenance_stale_tools.record(
            count,
            &[
                KeyValue::new("machine_id", machine_id.unwrap_or("unknown").to_string()),
                KeyValue::new("platform", env::consts::OS.to_string()),
                KeyValue::new("env_kind", env_kind().to_string()),
            ],
        );
    }
}

/// Bump the per-backend failure counter and record probe duration
/// with `outcome = <error_kind>`. Called from the checker's fan-out
/// on every backend `Err(_)`. `backend` and `error_kind` are both
/// bounded &'static strs by contract — cardinality safe.
pub fn maintenance_backend_failed(
    backend: &'static str,
    error_kind: &'static str,
    duration: Duration,
) {
    if !is_enabled() {
        return;
    }
    if let Some(state) = TELEMETRY.get()
        && let Some(ref metrics) = state.metrics
    {
        metrics.maintenance_backend_failures.add(
            1,
            &[
                KeyValue::new("backend", backend),
                KeyValue::new("error_kind", error_kind),
                KeyValue::new("platform", env::consts::OS.to_string()),
                KeyValue::new("env_kind", env_kind().to_string()),
            ],
        );
        metrics.maintenance_probe_duration.record(
            duration.as_secs_f64(),
            &[
                KeyValue::new("backend", backend),
                KeyValue::new("outcome", error_kind),
                KeyValue::new("platform", env::consts::OS.to_string()),
                KeyValue::new("env_kind", env_kind().to_string()),
            ],
        );
    }
}

/// Record a successful probe's duration with `outcome = "ok"`.
/// Companion to [`maintenance_backend_failed`] so on-call can chart
/// per-backend p99 across both success and failure paths.
pub fn maintenance_backend_probed(backend: &'static str, duration: Duration) {
    if !is_enabled() {
        return;
    }
    if let Some(state) = TELEMETRY.get()
        && let Some(ref metrics) = state.metrics
    {
        metrics.maintenance_probe_duration.record(
            duration.as_secs_f64(),
            &[
                KeyValue::new("backend", backend),
                KeyValue::new("outcome", "ok"),
                KeyValue::new("platform", env::consts::OS.to_string()),
                KeyValue::new("env_kind", env_kind().to_string()),
            ],
        );
    }
}

// ============================================================================
// Event Functions - Hooks
// ============================================================================

/// Record hook started
pub fn hook_started(hook_name: &str, hook_type: &str, tool: Option<&str>) {
    if !is_enabled() {
        return;
    }

    tracing::info!(
        event = "hook.started",
        hook_name = %hook_name,
        hook_type = %hook_type,
        tool = %tool.unwrap_or("global"),
    );
}

/// Record hook completed successfully
pub fn hook_completed(hook_name: &str, hook_type: &str, duration: Duration, exit_code: i32) {
    if !is_enabled() {
        return;
    }

    let duration_ms = duration.as_millis() as u64;
    tracing::info!(
        event = "hook.completed",
        hook_name = %hook_name,
        hook_type = %hook_type,
        duration_ms = %duration_ms,
        exit_code = %exit_code,
    );

    if let Some(state) = TELEMETRY.get()
        && let Some(ref metrics) = state.metrics
    {
        let attrs = [
            KeyValue::new("hook_type", hook_type.to_string()),
            KeyValue::new("status", "success"),
        ];
        metrics.hooks_executions.add(1, &attrs);
        metrics
            .hooks_duration
            .record(duration.as_secs_f64(), &[attrs[0].clone()]);
    }
}

/// Record hook failed
pub fn hook_failed(hook_name: &str, hook_type: &str, error: &str, error_type: &str) {
    if !is_enabled() {
        return;
    }

    let redacted_error = redact_sensitive(error);
    tracing::error!(
        event = "hook.failed",
        hook_name = %hook_name,
        hook_type = %hook_type,
        error = %redacted_error,
        error_type = %error_type,
    );

    if let Some(state) = TELEMETRY.get()
        && let Some(ref metrics) = state.metrics
    {
        metrics.hooks_executions.add(
            1,
            &[
                KeyValue::new("hook_type", hook_type.to_string()),
                KeyValue::new("status", "failed"),
            ],
        );
        metrics
            .errors
            .add(1, &[KeyValue::new("error_type", "hook")]);
    }
}

/// Record hook timeout
pub fn hook_timeout(hook_name: &str, hook_type: &str, timeout_secs: u64) {
    if !is_enabled() {
        return;
    }

    tracing::error!(
        event = "hook.timeout",
        hook_name = %hook_name,
        hook_type = %hook_type,
        timeout_seconds = %timeout_secs,
    );

    if let Some(state) = TELEMETRY.get()
        && let Some(ref metrics) = state.metrics
    {
        metrics.hooks_executions.add(
            1,
            &[
                KeyValue::new("hook_type", hook_type.to_string()),
                KeyValue::new("status", "timeout"),
            ],
        );
    }
}

// ============================================================================
// Event Functions - Commands
// ============================================================================

/// Record command execution
pub fn command_executed(command: &str, duration: Duration, success: bool) {
    if !is_enabled() {
        return;
    }

    let duration_ms = duration.as_millis() as u64;
    let status = if success { "success" } else { "failed" };

    tracing::info!(
        event = "command.executed",
        command = %command,
        duration_ms = %duration_ms,
        status = %status,
    );

    if let Some(state) = TELEMETRY.get()
        && let Some(ref metrics) = state.metrics
    {
        let attrs = [
            KeyValue::new("command", command.to_string()),
            KeyValue::new("status", status),
        ];
        metrics.commands.add(1, &attrs);
        metrics
            .commands_duration
            .record(duration.as_secs_f64(), &[attrs[0].clone()]);
    }
}

/// Record doctor issue found
pub fn doctor_issue_found(category: &str, severity: &str, message: &str) {
    if !is_enabled() {
        return;
    }

    tracing::info!(
        event = "doctor.issue_found",
        category = %category,
        severity = %severity,
        message = %message,
    );
}

/// Record search execution.
///
/// The user's raw query is NEVER emitted — enterprise users routinely
/// `jarvy search` for project-internal tool names that would otherwise
/// leak as fragments of their internal tooling vocabulary. Only the
/// hit-rate signal (whether the query had results) and the bucketed
/// length are sent so dashboards can graph "what fraction of searches
/// return zero hits" without storing the query text itself.
pub fn search_executed(query: &str, results_count: usize) {
    if !is_enabled() {
        return;
    }

    let had_results = results_count > 0;
    let query_len_bucket = match query.chars().count() {
        0 => "0",
        1..=4 => "1-4",
        5..=15 => "5-15",
        16..=40 => "16-40",
        _ => "40+",
    };

    tracing::info!(
        event = "search.executed",
        had_results = %had_results,
        results_count = %results_count,
        query_len_bucket = %query_len_bucket,
    );
}

/// Record validate result
pub fn validate_result(errors_count: usize, warnings_count: usize) {
    if !is_enabled() {
        return;
    }

    tracing::info!(
        event = "validate.result",
        errors_count = %errors_count,
        warnings_count = %warnings_count,
    );
}

/// Record export completed
pub fn export_completed(tools_count: usize, format: &str) {
    if !is_enabled() {
        return;
    }

    tracing::info!(
        event = "export.completed",
        tools_count = %tools_count,
        format = %format,
    );
}

/// Record diff executed
pub fn diff_executed(to_install: usize, to_update: usize, satisfied: usize, unknown: usize) {
    if !is_enabled() {
        return;
    }

    tracing::info!(
        event = "diff.executed",
        to_install = %to_install,
        to_update = %to_update,
        satisfied = %satisfied,
        unknown = %unknown,
    );
}

/// Record upgrade result
pub fn upgrade_result(upgraded: usize, failed: usize, skipped: usize) {
    if !is_enabled() {
        return;
    }

    tracing::info!(
        event = "upgrade.result",
        upgraded = %upgraded,
        failed = %failed,
        skipped = %skipped,
    );
}

/// Record doctor result
pub fn doctor_completed(issues_count: usize, tools_count: usize, exit_code: i32) {
    if !is_enabled() {
        return;
    }

    tracing::info!(
        event = "doctor.completed",
        issues_count = %issues_count,
        tools_count = %tools_count,
        exit_code = %exit_code,
    );
}

// ============================================================================
// Event Functions - Config
// ============================================================================

/// Record config loaded
pub fn config_loaded(
    source: &str,
    tools_count: usize,
    has_hooks: bool,
    has_env: bool,
    has_services: bool,
) {
    if !is_enabled() {
        return;
    }

    tracing::info!(
        event = "config.loaded",
        source = %source,
        tools_count = %tools_count,
        has_hooks = %has_hooks,
        has_env = %has_env,
        has_services = %has_services,
    );
}

/// Record config parse error
pub fn config_parse_error(file: &str, error: &str) {
    if !is_enabled() {
        return;
    }

    let redacted_file = redact_path(file);
    let redacted_error = redact_sensitive(error);

    tracing::error!(
        event = "config.parse_error",
        file = %redacted_file,
        error = %redacted_error,
    );

    if let Some(state) = TELEMETRY.get()
        && let Some(ref metrics) = state.metrics
    {
        metrics
            .errors
            .add(1, &[KeyValue::new("error_type", "config_parse")]);
    }
}

/// Record a successful upward `jarvy.toml` parent-directory search
/// (`resolve_project_root_from_cli`, mirrors `git`'s walk to find
/// `.git`). `command` is a bounded subcommand label; `levels_climbed`
/// is the directory-depth distance between the original cwd and the
/// discovered root.
pub fn config_parent_search_succeeded(command: &str, levels_climbed: u32) {
    if !is_enabled() {
        return;
    }

    tracing::info!(
        event = "config.parent_search_succeeded",
        command = %command,
        levels_climbed = %levels_climbed,
    );
}

/// Record an upward `jarvy.toml` search that reached `$HOME` without
/// finding a config. `command` is a bounded subcommand label.
pub fn config_parent_search_failed(command: &str) {
    if !is_enabled() {
        return;
    }

    tracing::warn!(
        event = "config.parent_search_failed",
        command = %command,
    );
}

/// Emit the telemetry for a `config::resolve_project_root_from_cli`
/// outcome. That resolver runs immediately after `Cli::parse()`, before
/// `telemetry::init` populates the state `is_enabled()` reads, so it
/// cannot call telemetry directly; it returns a `ParentSearchOutcome`
/// instead, which the caller replays through here once `init` has run.
pub fn emit_parent_search_outcome(outcome: crate::config::ParentSearchOutcome) {
    use crate::config::ParentSearchOutcome;
    match outcome {
        ParentSearchOutcome::Found {
            command,
            levels_climbed,
        } => config_parent_search_succeeded(command, levels_climbed),
        ParentSearchOutcome::ChdirFailed { command } => {
            if is_enabled() {
                tracing::warn!(
                    event = "config.parent_search_chdir_failed",
                    command = %command,
                );
            }
        }
        ParentSearchOutcome::NotFound {
            command,
            hint_shown,
        } => {
            if hint_shown {
                config_parent_search_failed(command);
            }
        }
        ParentSearchOutcome::Skipped => {}
    }
}

// ============================================================================
// Event Functions - Services
// ============================================================================

/// Record service operation
pub fn service_operation(backend: &str, action: &str, success: bool) {
    if !is_enabled() {
        return;
    }

    let status = if success { "success" } else { "failed" };
    tracing::info!(
        event = "service.operation",
        backend = %backend,
        action = %action,
        status = %status,
    );
}

// ============================================================================
// Event Functions - Package Manager
// ============================================================================

/// Record package manager batch install
pub fn package_manager_batch_install(
    pm: &str,
    packages_count: usize,
    succeeded: usize,
    failed: usize,
    duration: Duration,
) {
    if !is_enabled() {
        return;
    }

    let duration_ms = duration.as_millis() as u64;
    tracing::info!(
        event = "package_manager.batch_install",
        pm = %pm,
        packages_count = %packages_count,
        succeeded = %succeeded,
        failed = %failed,
        duration_ms = %duration_ms,
    );
}

// ============================================================================
// Event Functions - CI
// ============================================================================

/// Record CI environment detected
pub fn ci_detected(provider: &str, build_id: Option<&str>, branch: Option<&str>) {
    if !is_enabled() {
        return;
    }

    tracing::info!(
        event = "ci.detected",
        provider = %provider,
        build_id = %build_id.unwrap_or("unknown"),
        branch = %branch.unwrap_or("unknown"),
    );
}

// ============================================================================
// Event Functions - Environment
// ============================================================================

/// Record .env file generated
pub fn env_dotenv_generated(vars_count: usize, secrets_count: usize) {
    if !is_enabled() {
        return;
    }

    tracing::info!(
        event = "env.dotenv_generated",
        vars_count = %vars_count,
        secrets_count = %secrets_count,
    );
}

/// Record shell rc updated
pub fn env_shell_rc_updated(shell: &str, vars_count: usize) {
    if !is_enabled() {
        return;
    }

    tracing::info!(
        event = "env.shell_rc_updated",
        shell = %shell,
        vars_count = %vars_count,
    );
}

// ============================================================================
// Tracing Spans (T8)
// ============================================================================

/// Create a span for setup operations
#[macro_export]
macro_rules! telemetry_span {
    ($name:expr) => {
        tracing::info_span!($name)
    };
    ($name:expr, $($field:tt)*) => {
        tracing::info_span!($name, $($field)*)
    };
}

/// Create a setup span
pub fn span_setup(tools_count: usize) -> tracing::Span {
    tracing::info_span!(
        "jarvy.setup",
        tools_count = tools_count,
        platform = %env::consts::OS,
        env_kind = %env_kind()
    )
}

/// Create a version check span
pub fn span_version_check(tool: &str) -> tracing::Span {
    tracing::info_span!("jarvy.version_check", tool = %tool)
}

/// Create an install span
pub fn span_install(tool: &str, version: &str) -> tracing::Span {
    tracing::info_span!("jarvy.install", tool = %tool, version = %version)
}

/// Create a hook span
pub fn span_hook(hook_name: &str, hook_type: &str) -> tracing::Span {
    tracing::info_span!("jarvy.hook", hook_name = %hook_name, hook_type = %hook_type)
}

/// Create a command span
pub fn span_command(command: &str) -> tracing::Span {
    tracing::info_span!("jarvy.command", command = %command)
}

/// Create a service span
pub fn span_service(backend: &str, action: &str) -> tracing::Span {
    tracing::info_span!("jarvy.service", backend = %backend, action = %action)
}

// ============================================================================
// Privacy Helpers
// ============================================================================

// Compile redaction regexes once. Previously each call to `redact_sensitive`
// rebuilt both regexes (~50–200 µs each). `tool_failed` and `hook_failed`
// fire on every install/hook failure — that's the failure path, exactly
// when users are already waiting on retries.
static REDACT_HOME_PATH_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"(/home/[^/\s]+|/Users/[^/\s]+|C:\\Users\\[^/\\\s]+)")
        .expect("static home-path regex must compile")
});
static REDACT_ENV_VALUE_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"(API_KEY|TOKEN|SECRET|PASSWORD|CREDENTIAL)=\S+")
        .expect("static env-value regex must compile")
});

// Cache `dirs::home_dir()` result. Each call queries env / sysconf and
// allocates; with `redact_path` on the telemetry hot path that adds up.
static HOME_DIR_STRING: std::sync::LazyLock<Option<String>> =
    std::sync::LazyLock::new(|| dirs::home_dir().map(|p| p.to_string_lossy().into_owned()));

/// Redact potentially sensitive information from strings.
///
/// Returns a `Cow::Borrowed` when no replacement happened so the common
/// case (no match) avoids allocation entirely.
///
/// Round-2 perf F8: the previous impl forced an extra `.into_owned()`
/// clone on the home-only-match path. Now we match on the second
/// `replace_all` and hand the existing Cow back when the second pass
/// was a no-op — saving one heap copy per `tool_failed`/`hook_failed`
/// log line that contains a path.
fn redact_sensitive(s: &str) -> std::borrow::Cow<'_, str> {
    let after_home = REDACT_HOME_PATH_RE.replace_all(s, "[HOME]");
    match REDACT_ENV_VALUE_RE.replace_all(&after_home, "$1=[REDACTED]") {
        // Env pass changed something — its owned String already
        // includes the home redaction, so return it directly.
        std::borrow::Cow::Owned(owned) => std::borrow::Cow::Owned(owned),
        // Env pass was a no-op. Return whichever Cow `after_home`
        // already is — Borrowed if neither pass matched, Owned if
        // only the home pass did. No extra clone either way.
        std::borrow::Cow::Borrowed(_) => match after_home {
            std::borrow::Cow::Borrowed(_) => std::borrow::Cow::Borrowed(s),
            std::borrow::Cow::Owned(owned) => std::borrow::Cow::Owned(owned),
        },
    }
}

/// Redact file paths to remove user-identifying information.
pub fn redact_path(path: &str) -> String {
    if let Some(home) = HOME_DIR_STRING.as_deref()
        && !home.is_empty()
        && path.starts_with(home)
    {
        return path.replacen(home, "~", 1);
    }
    path.to_string()
}

/// Record that the first-run boxed telemetry disclosure was rendered.
///
/// The banner itself is a bare `eprintln!` so the user always sees it
/// regardless of telemetry state. This event is the audit trail: when
/// a user files "I didn't know telemetry was on," on-call can confirm
/// the disclosure actually fired on their host. Counter-only; carries
/// no PII (just `platform`).
///
/// Respects the consent guard: no-op if telemetry is disabled (a
/// disabled user shouldn't generate telemetry just because they saw
/// the disclosure — the disclosure itself fired regardless).
pub fn disclosure_shown(trigger: &str) {
    if !is_enabled() {
        return;
    }
    tracing::info!(
        event = "telemetry.disclosure_shown",
        trigger = %trigger,
        platform = %env::consts::OS,
        env_kind = %env_kind(),
    );
}

/// Record that the end-of-`setup` "opt-out, currently on" nudge fired.
///
/// Counterpart to [`disclosure_shown`]: lets on-call graph what
/// fraction of the fleet is still in the "undecided" state, which is
/// the signal for "is it safe to retire the nudge yet?". The nudge
/// only emits when telemetry is on, so this is also gated.
pub fn undecided_nudge_shown() {
    if !is_enabled() {
        return;
    }
    tracing::info!(
        event = "telemetry.undecided_nudge_shown",
        platform = %env::consts::OS,
        env_kind = %env_kind(),
    );
}

/// Returns `true` when the user has persisted an explicit telemetry
/// decision in `~/.jarvy/config.toml` — i.e., the `[telemetry]` table
/// contains a literal `enabled` key, regardless of value.
///
/// `content` is the raw TOML text. An empty string, missing file, or
/// missing `[telemetry]` table all parse as "undecided" so the caller
/// can fire a disclosure surface. Parsing is done via `toml::Value` so
/// the check is section-aware — a sibling table that happens to have
/// its own `enabled` key (`[mcp_register] enabled = true`) does NOT
/// suppress the disclosure, fixing a bug in the prior line-by-line
/// string match that was not section-aware and was fragile to
/// whitespace.
///
/// This is the source-of-truth check for the disclosure surfaces in
/// `src/init.rs` (boxed banner on first-run + legacy-config upgrade)
/// and `src/commands/setup_cmd.rs` (end-of-setup nudge).
pub fn user_decided(content: &str) -> bool {
    // Use `toml::from_str` rather than `Value::from_str` — in toml
    // 0.8, the `FromStr` impl on `Value` is the older "value-only"
    // parser that refuses content beyond a single TOML value, so
    // `"[telemetry]\nenabled = true"` errors with "expected nothing"
    // after the section header. `from_str::<Value>` is the document
    // parser and accepts a full TOML document.
    let Ok(value) = toml::from_str::<toml::Value>(content) else {
        return false;
    };
    let Some(telemetry) = value.get("telemetry").and_then(|t| t.as_table()) else {
        return false;
    };
    telemetry.contains_key("enabled")
}

// ============================================================================
// Timing Helpers
// ============================================================================

/// Create a timestamp for timing operations
pub fn now() -> Instant {
    Instant::now()
}

/// Convert duration to milliseconds
pub fn ms(d: Duration) -> u128 {
    d.as_millis()
}

// ============================================================================
// AI Hooks events
//
// Event taxonomy: domain `ai_hook` (singular, matches `hook.completed`),
// action snake_case. Every event passes structured fields — never user
// hook names or raw error messages — so OTLP receivers don't store
// user-controlled strings that may carry secrets.
// ============================================================================

/// Record the start of the AI hooks provisioning phase. Pairs with
/// `ai_hook.phase_completed` so an SRE can compute "what fraction of
/// setups ran AI hooks" and "p95 phase duration".
pub fn ai_hook_phase_started(agents: usize, hooks_count: usize, scope: &str, dry_run: bool) {
    if !is_enabled() {
        return;
    }
    tracing::info!(
        event = "ai_hook.phase_started",
        agents = %agents,
        hooks_count = %hooks_count,
        scope = %scope,
        dry_run = %dry_run,
    );
}

/// Record the end of the AI hooks provisioning phase across every
/// targeted agent.
pub fn ai_hook_phase_completed(
    applied: usize,
    agents_touched: usize,
    refused_local: usize,
    refused_remote: usize,
    failures: usize,
    duration: Duration,
) {
    if !is_enabled() {
        return;
    }
    let duration_ms = duration.as_millis() as u64;
    tracing::info!(
        event = "ai_hook.phase_completed",
        applied = %applied,
        agents_touched = %agents_touched,
        refused_local = %refused_local,
        refused_remote = %refused_remote,
        failures = %failures,
        duration_ms = %duration_ms,
    );
}

/// Record a single agent's successful provisioning. Carries the agent
/// slug so dashboards can split by target.
pub fn ai_hook_agent_applied(agent: &str, applied: usize, warnings: usize, settings_path: &Path) {
    if !is_enabled() {
        return;
    }
    let redacted_path = redact_path(&settings_path.to_string_lossy());
    tracing::info!(
        event = "ai_hook.agent_applied",
        agent = %agent,
        applied = %applied,
        warnings = %warnings,
        settings_path = %redacted_path,
    );
}

/// Record a single agent's provisioning failure. `error_type` is the
/// stable `AiHookError::kind()` tag — the formatted message is NOT
/// emitted so user-controlled hook names can't leak to OTLP.
pub fn ai_hook_agent_failed(agent: &str, error_type: &str) {
    if !is_enabled() {
        return;
    }
    tracing::warn!(
        event = "ai_hook.agent_failed",
        agent = %agent,
        error_type = %error_type,
    );
}

/// Record a single Jarvy-managed entry landing on disk. Lets auditors
/// answer "did this developer have `audit-log` provisioned on
/// 2026-05-01?" without recomputing from setup logs.
pub fn ai_hook_provisioned(agent: &str, hook_name: &str, library_source: Option<&str>) {
    if !is_enabled() {
        return;
    }
    tracing::info!(
        event = "ai_hook.provisioned",
        agent = %agent,
        hook_name = %hook_name,
        library_source = %library_source.unwrap_or("custom"),
    );
}

/// Roll-up event for custom-command refusals. Single INFO line per
/// phase instead of one WARN per refused entry — refusal is configured
/// behavior, not an incident, so don't page on it.
pub fn ai_hook_custom_refused_summary(local_count: usize, remote_count: usize) {
    if !is_enabled() {
        return;
    }
    tracing::info!(
        event = "ai_hook.custom_refused_summary",
        local_count = %local_count,
        remote_count = %remote_count,
    );
}

/// Record a `jarvy ai-hooks check` invocation's outcome. Lets CI compute
/// drift rate over time.
pub fn ai_hook_check_completed(agents_checked: usize, drifted_agents: usize) {
    if !is_enabled() {
        return;
    }
    tracing::info!(
        event = "ai_hook.check_completed",
        agents_checked = %agents_checked,
        drifted_agents = %drifted_agents,
    );
}

/// Record a Windows auto-translation fallback for a custom hook.
/// Bounded cardinality (library size + custom name set) so safe to
/// emit per-fire.
pub fn ai_hook_windows_auto_translated(agent: &str, hook_name: &str) {
    if !is_enabled() {
        return;
    }
    tracing::info!(
        event = "ai_hook.windows_auto_translated",
        agent = %agent,
        hook_name = %hook_name,
    );
}

// ============================================================================
// MCP server registration events
//
// Same taxonomy shape as `ai_hook.*`: structured fields only, no
// user-controlled strings in OTLP payloads, per-agent attribution.
// ============================================================================

pub fn mcp_register_phase_started(agents: usize, servers_count: usize, scope: &str) {
    if !is_enabled() {
        return;
    }
    tracing::info!(
        event = "mcp_register.phase_started",
        agents = %agents,
        servers_count = %servers_count,
        scope = %scope,
    );
}

/// Record that `jarvy setup` synthesized a default `[mcp_register]`
/// because the project's `jarvy.toml` had no block AND the user had
/// at least one AI agent installed on disk. Emitted once per setup
/// invocation that auto-registered. Counts + agent slugs let on-call
/// graph "how many users are on the default opt-out path vs. an
/// explicit project block" and which agents are most commonly
/// auto-detected.
pub fn mcp_register_auto_detected(detected: &[crate::mcp_register::McpAgentTarget]) {
    if !is_enabled() {
        return;
    }
    let slugs: Vec<&str> = detected.iter().map(|a| a.slug()).collect();
    tracing::info!(
        event = "mcp_register.auto_detected",
        count = %detected.len(),
        agents = %slugs.join(","),
        platform = %env::consts::OS,
        env_kind = %env_kind(),
    );
}

pub fn mcp_register_phase_completed(
    applied: usize,
    agents_touched: usize,
    refused_local: usize,
    refused_remote: usize,
    failures: usize,
    duration: Duration,
) {
    if !is_enabled() {
        return;
    }
    tracing::info!(
        event = "mcp_register.phase_completed",
        applied = %applied,
        agents_touched = %agents_touched,
        refused_local = %refused_local,
        refused_remote = %refused_remote,
        failures = %failures,
        duration_ms = %duration.as_millis(),
    );
}

pub fn mcp_register_agent_applied(agent: &str, applied: usize, settings_path: &Path) {
    if !is_enabled() {
        return;
    }
    let redacted_path = redact_path(&settings_path.to_string_lossy());
    tracing::info!(
        event = "mcp_register.agent_applied",
        agent = %agent,
        applied = %applied,
        settings_path = %redacted_path,
    );
}

pub fn mcp_register_agent_failed(agent: &str, error_type: &str) {
    if !is_enabled() {
        return;
    }
    tracing::warn!(
        event = "mcp_register.agent_failed",
        agent = %agent,
        error_type = %error_type,
    );
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_telemetry_config_default() {
        // Pins the opt-out default. Flipping back to `enabled = false`
        // would silently undo the documented opt-out posture in
        // CLAUDE.md and docs/telemetry.md.
        let config = TelemetryConfig::default();
        assert!(config.enabled);
        // Pin the *effective* posture, not just the bit — survives
        // an accidental change that makes `enabled` true while every
        // signal is off.
        assert!(
            config.is_enabled(),
            "default must be effectively on (enabled + at least one signal)"
        );
        assert_eq!(config.endpoint, "https://telemetry.jarvy.dev");
        assert_eq!(config.protocol, "http");
        assert!(config.logs);
        assert!(config.metrics);
        assert!(!config.traces);
        assert_eq!(config.sample_rate, 1.0);
    }

    #[test]
    fn test_telemetry_config_is_enabled() {
        let mut config = TelemetryConfig::default();
        // Default is opt-out → enabled.
        assert!(config.is_enabled());

        config.enabled = false;
        assert!(!config.is_enabled());

        config.enabled = true;
        assert!(config.is_enabled());

        config.logs = false;
        config.metrics = false;
        config.traces = false;
        assert!(!config.is_enabled()); // No signals enabled
    }

    #[test]
    fn validate_endpoint_accepts_https_and_loopback() {
        let mut config = TelemetryConfig {
            endpoint: "https://otel.corp.com:4318".to_string(),
            ..TelemetryConfig::default()
        };
        assert!(config.validate_endpoint().is_ok());
        config.endpoint = "http://localhost:4318".to_string();
        assert!(config.validate_endpoint().is_ok());
        config.endpoint = "http://127.0.0.1:4318".to_string();
        assert!(config.validate_endpoint().is_ok());
        config.endpoint = "http://[::1]:4318".to_string();
        assert!(config.validate_endpoint().is_ok());
    }

    #[test]
    fn validate_endpoint_rejects_plain_http_remote() {
        let config = TelemetryConfig {
            endpoint: "http://attacker.tld:4318".to_string(),
            ..TelemetryConfig::default()
        };
        let err = config.validate_endpoint().unwrap_err();
        assert!(err.contains("plain HTTP"), "got {err:?}");
    }

    #[test]
    fn validate_endpoint_rejects_unknown_scheme() {
        let mut config = TelemetryConfig {
            endpoint: "ftp://otel.corp.com".to_string(),
            ..TelemetryConfig::default()
        };
        assert!(config.validate_endpoint().is_err());
        config.endpoint = "".to_string();
        assert!(config.validate_endpoint().is_err());
    }

    #[test]
    fn test_source_display() {
        assert_eq!(Source::Config.to_string(), "config");
        assert_eq!(Source::Mcp.to_string(), "mcp");
        assert_eq!(Source::Cli.to_string(), "cli");
        assert_eq!(Source::Request.to_string(), "request");
        assert_eq!(Source::Diagnose.to_string(), "diagnose");
        assert_eq!(Source::Doctor.to_string(), "doctor");
        assert_eq!(Source::Search.to_string(), "search");
    }

    // ----- narrow_with_project trust-boundary tests -----------------
    //
    // These pin the security claim that a project `jarvy.toml` (arriving
    // via untrusted `git clone`) may narrow telemetry but never broaden
    // it. Flipping any boolean direction in the helper must fail at
    // least one of these tests.

    fn user_telemetry_on() -> TelemetryConfig {
        TelemetryConfig {
            enabled: true,
            endpoint: "https://telemetry.jarvy.dev".to_string(),
            protocol: "http".to_string(),
            logs: true,
            metrics: true,
            traces: true,
            sample_rate: 1.0,
            resource: HashMap::new(),
        }
    }

    fn project_telemetry_default() -> TelemetryConfig {
        // Mirrors the implicit shape when a project jarvy.toml has only
        // a partial `[telemetry]` table.
        TelemetryConfig::default()
    }

    #[test]
    fn narrow_with_project_can_disable_when_user_enabled() {
        let mut user = user_telemetry_on();
        let project = TelemetryConfig {
            enabled: false,
            ..project_telemetry_default()
        };
        let warning = user.narrow_with_project(&project);
        assert!(warning.is_none(), "no warning expected on plain disable");
        assert!(!user.enabled, "project must be able to disable");
    }

    #[test]
    fn narrow_with_project_cannot_enable_when_user_disabled() {
        // User has explicitly opted out via `jarvy telemetry disable`
        // or `JARVY_TELEMETRY=0`. A project `jarvy.toml` setting
        // `enabled = true` must not flip them back on.
        let mut user = TelemetryConfig {
            enabled: false,
            ..TelemetryConfig::default()
        };
        let project = TelemetryConfig {
            enabled: true,
            ..TelemetryConfig::default()
        };
        let _ = user.narrow_with_project(&project);
        assert!(
            !user.enabled,
            "project enabling must not broaden user opt-out"
        );
    }

    #[test]
    fn narrow_with_project_ands_signal_flags() {
        let mut user = user_telemetry_on();
        let project = TelemetryConfig {
            logs: false,
            metrics: false,
            traces: true, // user has true; AND must keep true
            ..project_telemetry_default()
        };
        let _ = user.narrow_with_project(&project);
        assert!(!user.logs, "project can turn logs off");
        assert!(!user.metrics, "project can turn metrics off");
        assert!(user.traces, "project cannot turn traces on (AND-narrowing)");
    }

    #[test]
    fn narrow_with_project_ands_signal_cannot_broaden() {
        // User has metrics=false; project metrics=true must NOT enable.
        let mut user = TelemetryConfig {
            metrics: false,
            ..user_telemetry_on()
        };
        let project = TelemetryConfig {
            metrics: true,
            ..project_telemetry_default()
        };
        let _ = user.narrow_with_project(&project);
        assert!(!user.metrics, "AND-narrowing must not flip false to true");
    }

    #[test]
    fn narrow_with_project_sample_rate_takes_min() {
        let mut user = TelemetryConfig {
            sample_rate: 0.5,
            ..user_telemetry_on()
        };
        let project = TelemetryConfig {
            sample_rate: 0.1,
            ..project_telemetry_default()
        };
        let _ = user.narrow_with_project(&project);
        assert!(
            (user.sample_rate - 0.1).abs() < 1e-9,
            "min must apply: {}",
            user.sample_rate
        );

        // Inverse: project asks for higher rate — must not broaden.
        let mut user2 = TelemetryConfig {
            sample_rate: 0.1,
            ..user_telemetry_on()
        };
        let project2 = TelemetryConfig {
            sample_rate: 0.9,
            ..project_telemetry_default()
        };
        let _ = user2.narrow_with_project(&project2);
        assert!(
            (user2.sample_rate - 0.1).abs() < 1e-9,
            "project cannot raise sample_rate"
        );
    }

    #[test]
    fn narrow_with_project_refuses_endpoint_override_with_warning() {
        let mut user = user_telemetry_on();
        let project = TelemetryConfig {
            endpoint: "https://attacker.tld/collect".to_string(),
            ..project_telemetry_default()
        };
        let warning = user
            .narrow_with_project(&project)
            .expect("endpoint mismatch must produce a warning");
        assert!(
            user.endpoint.starts_with("https://telemetry.jarvy.dev"),
            "user endpoint must be untouched: {}",
            user.endpoint
        );
        assert!(
            warning.contains("non-default telemetry endpoint"),
            "warning text contract: {}",
            warning
        );
        assert!(
            warning.contains("JARVY_OTLP_ENDPOINT"),
            "warning must point at the env-var escape hatch"
        );
    }

    #[test]
    fn narrow_with_project_endpoint_warning_sanitizes_attacker_bytes() {
        // The whole point of refusing: don't let attacker-controlled
        // bytes reach stderr verbatim. Control bytes that would clear
        // the terminal or forge output must be stripped.
        let mut user = user_telemetry_on();
        let project = TelemetryConfig {
            endpoint: "https://evil.tld\x1b[2J\x1b[Hfake".to_string(),
            ..project_telemetry_default()
        };
        let warning = user.narrow_with_project(&project).unwrap();
        assert!(
            !warning.contains('\x1b'),
            "ANSI escape must not survive: {:?}",
            warning
        );
    }

    #[test]
    fn narrow_with_project_matching_endpoint_no_warning() {
        // If the project echoes the user's already-set endpoint, no
        // warning — that's just confirming the user's choice.
        let mut user = TelemetryConfig {
            endpoint: "https://example.com/otlp".to_string(),
            ..user_telemetry_on()
        };
        let project = TelemetryConfig {
            endpoint: "https://example.com/otlp".to_string(),
            ..project_telemetry_default()
        };
        let warning = user.narrow_with_project(&project);
        assert!(warning.is_none(), "matching endpoint must not warn");
    }

    #[test]
    fn tool_request_explicit_returns_false_when_telemetry_uninit() {
        // The test binary doesn't call `telemetry::init`, so TELEMETRY
        // is `None` and the counter is dropped. Item 1's regression
        // guard for the `--request` channel-lie bug relies on this
        // branch returning `false` so the caller routes to Manual.
        //
        // The TELEMETRY OnceLock prevents us from also asserting the
        // `true` branch in the same binary — the initialized branch
        // is covered by the integration tests
        // `tools_request_pretty_confirms_telemetry_send_when_enabled`
        // and `tools_request_json_telemetry_on_reports_telemetry_channel`
        // which spawn fresh processes with JARVY_TELEMETRY=1.
        let fired = tool_request_explicit("nonexistent-tool", &[]);
        assert!(
            !fired,
            "uninitialized telemetry must drop the metric and return false"
        );
    }

    #[test]
    fn narrow_with_project_default_endpoint_no_warning() {
        // Project endpoint equals the system default — no override,
        // no warning.
        let mut user = TelemetryConfig {
            endpoint: "https://example.com/otlp".to_string(),
            ..user_telemetry_on()
        };
        let project = project_telemetry_default(); // .endpoint is the default
        let warning = user.narrow_with_project(&project);
        assert!(warning.is_none(), "default endpoint must not warn");
    }

    #[test]
    fn test_redact_sensitive() {
        let input = "Error at /home/user/project: API_KEY=secret123";
        let result = redact_sensitive(input);
        assert!(result.contains("[HOME]"));
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("user"));
        assert!(!result.contains("secret123"));
    }

    #[test]
    fn test_redact_path() {
        // This test is platform-dependent
        let home = dirs::home_dir().map(|p| p.to_string_lossy().to_string());
        if let Some(home) = home {
            let path = format!("{}/some/path", home);
            let result = redact_path(&path);
            assert!(result.starts_with("~"));
            assert!(!result.contains(&home));
        }
    }

    #[test]
    fn user_decided_recognizes_explicit_enabled() {
        assert!(user_decided("[telemetry]\nenabled = true\n"));
        assert!(user_decided("[telemetry]\nenabled = false\n"));
        // Whitespace-agnostic — the prior string-match required exactly
        // `enabled = true` and missed `enabled=true`.
        assert!(user_decided("[telemetry]\nenabled=true\n"));
        // Order-independent: enabled may appear after other keys.
        assert!(user_decided(
            "[telemetry]\nendpoint = \"https://example\"\nenabled = false\n"
        ));
    }

    #[test]
    fn user_decided_undecided_when_telemetry_missing() {
        assert!(!user_decided(""));
        assert!(!user_decided("[settings]\nfingerprint = \"abc\"\n"));
        // Telemetry section present but no `enabled` key — undecided.
        assert!(!user_decided(
            "[telemetry]\nendpoint = \"https://example\"\n"
        ));
    }

    #[test]
    fn user_decided_section_aware_not_string_match() {
        // The prior implementation scanned lines and would treat a
        // sibling section's `enabled` as a decision. This must NOT.
        assert!(!user_decided(
            "[mcp_register]\nenabled = true\n[telemetry]\nendpoint = \"https://example\"\n"
        ));
        assert!(!user_decided("[drift]\nenabled = true\n"));
    }

    #[test]
    fn user_decided_returns_false_on_malformed_toml() {
        // Brand-new corrupt config — treat as undecided so the next run
        // surfaces the disclosure rather than silently enrolling.
        assert!(!user_decided("not [valid toml"));
    }

    #[test]
    fn test_setup_summary_default() {
        let summary = SetupSummary::default();
        assert_eq!(summary.tools_requested, 0);
        assert_eq!(summary.tools_installed, 0);
        assert_eq!(summary.tools_skipped, 0);
        assert_eq!(summary.tools_failed, 0);
        assert_eq!(summary.hooks_run, 0);
    }

    #[test]
    fn env_kind_returns_bounded_value() {
        let kind = env_kind();
        assert!(
            kind == "wsl" || kind == "native",
            "unexpected env_kind: {kind}"
        );
    }
}
