# Changelog

All notable changes to Jarvy will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Policy

- **Stable releases (`vX.Y.Z`)** get a curated entry below **before the tag is
  pushed**. The release workflow's `Build release notes` step awk-extracts the
  matching `## [vX.Y.Z]` section into the GitHub release body, then appends a
  `**Full Changelog**` compare link plus Jarvy's standing install/security
  footer. Forgetting this entry causes the workflow to fall through to a raw
  `git log` listing — technically valid, but reads like a commit dump rather
  than a curated narrative. Update CHANGELOG before tagging.
- **Pre-releases (`vX.Y.Z-rc.N`, `-beta.N`, `-alpha.N`)** do **not** get a
  CHANGELOG entry. The awk extraction returns empty, the workflow falls
  through to `git log <prev-tag>..<tag>` notes, and that fallback is the
  intended pre-release path. The curated stable entry below is written once
  when the corresponding stable cuts.
- Entry headers must match the awk pattern: `## [vX.Y.Z]` or
  `## [vX.Y.Z] — Title` (em-dash optional). Other shapes won't be matched.

See [`docs/release-testing.md`](https://github.com/Cliftonz/jarvy/blob/main/docs/release-testing.md)
for the full release process and
[`docs/release-quirks-jarvy.md`](https://github.com/Cliftonz/jarvy/blob/main/docs/release-quirks-jarvy.md)
for divergences from generic release skills.

## [v0.8.3] — In-house update swap, WSL-aware diagnostics (2026-09-06)

**Changed:**

- Drop the `self_update` crate; binary replacement is now an in-house
  two-rename swap. `jarvy update` first copies the downloaded binary to
  `<target>.new` beside the install target, so the rename that follows
  never crosses a filesystem boundary; the old path renamed straight out
  of the staging directory, which on a Linux box with a tmpfs `/tmp`
  failed with EXDEV and aborted the update outright. From there it renames
  the running binary to `<target>.old`, renames `<target>.new` into place,
  and renames the old one back if that second step fails (if even the
  rename-back fails, the error names both paths so the binary can be
  recovered by hand). Unix deletes `<target>.old` once the swap lands;
  Windows keeps it, because a running executable can't be unlinked there,
  and the next update replaces the stale sibling. Rollback runs the same
  swap through the same `<target>.new` staging copy, so restoring a backup
  from `~/.jarvy/backup` no longer requires the backup directory and the
  install directory to sit on one filesystem; the backup file is copied
  rather than moved, so it survives the restore. Removing the dependency
  also took quick-xml out of the tree, so the two RUSTSEC quick-xml
  advisory ignores are gone from `deny.toml`.
- The Windows Defender retry loop keeps its 5 attempts and 500ms-to-4s
  backoff, but now gives up the moment an attempt strands the install:
  when the new binary failed to land and the old one couldn't be renamed
  back, retrying can only bury the real diagnostic. The error naming both
  paths is what the user sees, instead of a generic not-found reported by
  a later attempt.

**Features:**

- Detect WSL at runtime by reading `/proc/version` once and memoizing the
  result. Non-Linux targets always report not-WSL, and an unreadable file
  counts as not-WSL rather than failing.
- `jarvy services` preflight now prints a WSL-specific hint when the Docker
  daemon is unreachable inside a distro: enable Docker Desktop's WSL
  integration, or start a native daemon with `sudo systemctl start docker`.
  Reported as `hint_kind = "wsl_docker_desktop"`.
- Telemetry carries a bounded `env_kind = "wsl" | "native"` next to every
  `platform` field and OTLP attribute (`tool.*`, `setup.started`,
  `setup.inventory`, the maintenance probe metrics, the `jarvy.setup` span,
  the telemetry disclosure events, and `mcp_register.auto_detected`), so a
  Linux binary running inside a distro is distinguishable from one on bare
  metal. Purely additive; `platform` is unchanged.

**Docs:**

- Add a local-LLM example (ollama + graphify) under `examples/`.

**Internal:**

- Pass `GITHUB_TOKEN` to the `installer-e2e` unix CI job.
- Bump the VS Code extension's transitive `fast-uri` to 3.1.7 for the open
  Dependabot advisories.

## [v0.8.2] — Windows dev-env fixes, config parent-dir search, 17 new tools (2026-09-02)

**Features — tools:**

- Add PowerToys, AutoHotkey, and VCRedist (Windows).
- Add 6 Rust ecosystem tools (cargo-generate, cargo-seek, dioxus-cli, sqlx-cli,
  tauri-cli, trunk) plus a bacon dev-loop config template.
- Add Ory, LastPass, Dashlane, 1Password, Bitwarden, and Doppler CLIs
  (secrets/auth). Ory installs on Windows via a newly wired Scoop tier,
  used after winget/choco for tools that ship no first-party package there.
- Add llm-checker, a hardware scanner and local-LLM recommendation CLI
  (npm-only distribution).
- Add graphify-dotnet's `graphify` knowledge-graph tool via a NuGet global
  install; refuses when no installed .NET SDK is major version 10 or newer.
- Give ollama a `default_hook` that backgrounds pulls of pinned default
  models (llama3.1:8b for chat, bge-m3 for embeddings) after install, logged
  to `~/.jarvy/logs/ollama-pull.log`.

**Features — config:**

- Every command now searches parent directories for `jarvy.toml` when it
  isn't found in the current directory (bounded by `$HOME`, mirroring git's
  search for `.git`), then switches into the discovered project root before
  running. Fixes `jarvy setup` and other subcommands failing with "Failed to
  read config file" when run from a project subdirectory.

**Features — windows:**

- Auto-install MSVC Build Tools via a `pre_setup` hook so a fresh Windows
  contributor building jarvy itself doesn't hit a missing `link.exe` with no
  guidance.
- Add `[windows.wsl] browser_launcher = "off" | "wslu" | "cmd_shim"`: fixes
  CLIs inside a WSL distro (e.g. `gh auth login`) that can't open the
  Windows browser. `wslu` installs the wslu apt package and sets
  `BROWSER=wslview`; `cmd_shim` points `BROWSER` at a `cmd.exe /c start`
  shim with no package install. `wslu` is refused at config-load time for
  the Debian base distro, since it's Ubuntu-universe-only.

**Features — dependency declarations:**

- New `depends_on_by_os` macro slot: a tool can declare a prerequisite that
  only applies on a specific OS (e.g. VCRedist is Windows-only), merged into
  dependency checking and topo-sort so it doesn't false-fire on other
  platforms.
- Node declared as `depends_on_one_of` for npm-distributed CLIs, so at least
  one JS runtime is ensured before those installs run.

**Fixes:**

- Default Windows hooks to git-bash instead of PowerShell (every built-in
  `default_hook` script is POSIX syntax), auto-bootstrap Chocolatey when a
  tool only ships a `windows.choco` entry, and refresh PATH mid-setup-run so
  a hook depending on a tool installed earlier in the same run sees it
  immediately.
- Scope pyenv/rbenv/nvm's declared dependencies to the OSes they actually
  support, fixing false "missing required dependency" warnings on
  unsupported OSes.
- Declare missing runtime dependencies for git-lfs (→ git), cargo-tarpaulin,
  and yadm so topo-sort installs them in the correct order.
- Add icu as a Linux-only dependency of dotnet; .NET on Linux dynamically
  loads ICU at runtime and fails hard without it, which bit `dotnet run` on
  a fresh WSL or Linux box.
- Use the maintained `ezwinports.make` winget id for `make` instead of
  `GnuWin32.Make`, a roughly 2006-era 3.81 build; the new id ships 4.4.1.
- Gate the legacy `install_docker()` call to macOS only; it was running
  unconditionally on every OS and logging a spurious warning from an
  unnecessary `brew --version` subprocess on Linux and Windows.
- Fix opentofu's Linux install: it isn't in Debian/Ubuntu's or Fedora/RHEL's
  default repos, and the apt/dnf package is named `tofu`, not `opentofu`.
  Routes apt/dnf through OpenTofu's official vendor repository instead;
  pacman already carries `opentofu` directly in its default repo.
- Use real shell detection for hooks instead of a hardcoded `powershell` on
  Windows and raw `$SHELL` on Unix; every built-in hook script is bash
  syntax, so a machine with git-bash on PATH but no explicit
  `[hooks.config]` override was silently failing hooks with parse errors.
  Installer failures (winget and others) also now fall back to stdout when
  stderr is empty, since some tools print their real diagnostic there.
- Strip CRLF line endings from hook scripts before POSIX-shell execution; a
  `jarvy.toml` saved with Windows line endings broke bash parsing on
  embedded `\r` bytes. `jarvy validate` now also warns when a hook script
  contains CRLF.

## [v0.8.1] — WSL2 bridge, 16 new tools, per-OS example templates (2026-08-22)

**Features — WSL2 bridge (`[windows.wsl]`):**

- Ship a `wsl` tool that installs Store-WSL2 plus a named distro
  instance on Windows, optionally curl-pipes jarvy inside the distro,
  and (when `run_setup = true`) delegates the outer `jarvy setup` into
  the distro on the translated project path (`C:\...` → `/mnt/c/...`).
  Every POSIX tool that already works on Linux (tmux, htop, zsh, yadm,
  goaccess, tilt, ...) becomes installable on Windows without a new
  install backend — the Linux-side jarvy is the install backend.
- Opt-in via `[windows.wsl] enabled = true`. Jarvy never triggers UAC;
  when elevation is required jarvy prints the exact
  `wsl --install -d <distro> --name <name>` command for the user to
  run themselves. Refuses to touch DISM on legacy pre-Store WSL.
- v1 supports base distros `Ubuntu` and `Debian`. Instance name
  defaults to `distro` verbatim; refuses `docker-desktop` /
  `docker-desktop-data` at config load. Config inherits
  `[windows] allow_remote` for remote-origin trust.
- `jarvy tools --remove wsl` refuses (destructive-action policy) and
  prints the exact `wsl --unregister <name>` command instead of
  running it.
- New `wsl.*` telemetry domain covering probe, bootstrap, in-distro
  jarvy install, delegation, path refusal, and remove refusal — all
  gated, all bounded labels only (instance name never emitted).

**Features — tools:**

- Add 10 web browsers: Chrome, Firefox, Brave, Edge, Arc, Opera,
  Vivaldi, Zen, Blisk, Helium. Plus a `browser_repos` helper that
  bootstraps vendor apt/dnf repos on Linux where publishers ship them.
- Add 5 file managers: Files (Windows), OneCommander, Directory Opus,
  Total Commander, Double Commander.
- Add `lens` — Mirantis Kubernetes IDE (cask on macOS, winget on
  Windows).

**Docs:**

- Add per-OS `personal-workstation-{macos,linux,windows}` example
  templates with a realistic `jarvy.toml` per platform.
- Document `[windows.wsl]` in `docs/windows.md` with the full
  bootstrap-and-delegate flow, refusal messages, and telemetry rows.

## [v0.8.0] — native git hooks, ecosystem fallback installers, multi-distribution Java (2026-08-19)

**Breaking:**

- `[git_hooks]` no longer shells out to the `pre-commit` framework; the
  native handler is the only execution path. Projects that relied on
  jarvy running `pre-commit install` should migrate their hooks to
  `[git_hooks.native]`. Husky and lefthook are still auto-detected but
  their handlers now return `UnsupportedFramework`.

**Features — native git hooks (`[git_hooks.native]`, PRD-059):**

- Hooks write directly into `.git/hooks/<stage>` and cover all 23 git
  hook stages (`pre-commit`, `pre-push`, `commit-msg`, `post-checkout`,
  `pre-rebase`, ...), with no third-party framework dependency.
- Four source shapes teams can mix, later shapes overriding earlier for
  the same stage: a shared remote hooks repo
  (`repo = "github:org/team-hooks"` with a required pinned `ref` and
  optional `subpath`), a scanned local folder (`dir = "scripts/hooks"`),
  a file reference, or an inline script body.
- Remote hook repos cache under `~/.jarvy/git_hooks_cache/` and refresh
  idempotently on rerun; branch refs warn as mutable; absolute paths,
  `..` traversal, and symlink escapes are refused.
- Every installed hook carries a managed-by-jarvy marker: reruns
  overwrite only jarvy's own output, hand-authored `.git/hooks/*` files
  are never clobbered, and `jarvy hooks uninstall` removes only
  marker-bearing files.

**Features — ecosystem fallback installers (PRD-060):**

- Tools without a first-party platform package can declare
  `fallback: { go|npm|cargo|uv: "<pkg>" }`; jarvy routes the install
  through that ecosystem's package manager and auto-bootstraps a missing
  toolchain through its own registry (never curl-pipe). 15 existing
  tools migrated to fallback routes; 11 new tool definitions added.
- Fallback installs write receipts to `~/.jarvy/install-receipts.json`:
  `jarvy check-updates` probes the receipt's ecosystem so freshness
  reflects how the tool was actually installed, and `jarvy upgrade`
  upgrades through the same route (`--native` reroutes to the platform
  manager and clears the receipt).
- `tool.installed` / `tool.failed` telemetry carry `install_route`, so
  "brew broke" and "npm fallback broke" are distinguishable.

**Features — multi-distribution Java:**

- `java = { version = "21", distribution = "temurin" }` in
  `[provisioner]` routes across openjdk, temurin, zulu, corretto,
  liberica, and microsoft-openjdk, with per-OS support matrices and a
  declared fallback chain; role inheritance propagates distribution and
  fallback to member configs.
- Linux installs bootstrap the vendor apt/dnf repositories (Temurin,
  Zulu, Corretto, Liberica, Microsoft) with GPG key fingerprint
  verification and a 30-day idempotency window.

**Features — Windows quality of life:**

- New `[windows]` block fixes ".sh opens in Notepad": `sh_pathext`
  appends `.SH` to HKCU PATHEXT and `sh_association = "open"` routes the
  Explorer "open" verb for `.sh` files to bash.exe while leaving "edit"
  alone. Per-user registry only (no admin); remote configs refused
  without `allow_remote = true`.

**Features — templates, tools, env:**

- Mobile template overhaul: Android, React Native, and Expo templates
  ship `[commands]` lifecycle wiring (`pre:android` hooks and friends)
  through an escape-safe TOML writer.
- New tools: git-town, kubeconform plus 6 more Kubernetes-ecosystem
  CLIs, kubent, vcluster, crossplane, git-cliff, zellij, difftastic,
  6 Grafana ecosystem CLIs, and a mobile-dev toolset (Android SDK
  tooling, Appium, EAS CLI, Watchman).
- `[env]` values support `append = true` with PATH-position handling
  end-to-end through the shell rc writer; `append` on a non-PATH-like
  key is refused at config load.
- Opt-in `block-unmanaged-installs` hook added to the AI-hook library.

**Performance:**

- `jarvy doctor` gains a disk-backed version-probe cache (~33s cold to
  ~2s warm); validity is stat-based, so upgrading a tool invalidates its
  entry instantly.
- Drift detection runs version probes in bounded parallel.

**Fixes:**

- Console output capped at WARN for every non-Setup command; routine
  commands no longer spray INFO chatter (`-v` restores it).
- Explicit `jarvy update check` bypasses the sandbox auto-disable.
- Java fallback preserves the requested version, and dry-run previews
  match the apply path.
- `jarvy search` demand signal uses substring matching instead of fuzzy
  score, so near-miss suggestions no longer mask unmet demand.

**Security:**

- Drift subsystem hardened: trust gates, path-traversal refusals,
  permission checks, and install-method rederivation.
- Remote-config trust boundary tightened across config loading; package
  validators hardened.
- `tool.unsupported` probe surfaces sanitized (ANSI, control bytes, bidi
  overrides stripped), telemetry-gated, and cardinality-bounded.
- Azul, Corretto, and BellSoft GPG key fingerprints verified against
  vendor docs and pinned.

**Internal:**

- Pre-release tags (`-rc`/`-beta`/`-alpha`) now publish to crates.io for
  the beta channel; RPM and MSI packaging skip on pre-release tags; docs
  build (mkdocs strict + link check) is a hard release gate.
- PTY-driven E2E coverage for the interactive menu, consent prompts, and
  refusal paths.

## [v0.7.0] — agent profile switcher (PRD-058) + tool freshness advisory (PRD-057) (2026-07-31)

**Features — agent profile switcher (PRD-058 v1):**

- **`jarvy agents profile {init,create,use,save,restore,list,status,delete,check-cwd}`** —
  named profiles that snapshot an AI agent's *whole* config dir
  (credentials, settings, skills, MCP registrations, memory) into
  `~/.jarvy/agent-profiles/<profile>/<agent-slug>/`, so a work
  account and a personal account no longer mean logging out and
  back in. Store is 0700 with a chmod read-back verify; file modes
  are preserved so credentials keep their original 0600.
  See `docs/agent-profiles.md`.
- **Two switching tiers**, declared per agent on the canonical
  `agents::Agent` enum so a new variant lands everywhere at once:
  *env tier* (claude-code → `CLAUDE_CONFIG_DIR`, codex →
  `CODEX_HOME`) switches per-terminal — two shells can run
  different profiles at once — and *symlink tier* (cursor)
  re-points `~/.cursor` atomically via sibling temp link plus
  rename-over, with a `mklink /J` junction fallback on Windows.
  `use --print-env` keeps stdout pure for `eval`; human text goes
  to stderr and paths are escaped.
- **`jp` shell shorthand** in the existing `jarvy shell-init`
  snippet — bare `jp` lists profiles, `jp <name>` evals the env
  exports. Same nushell materialization path as `jr`.
- **Refusals over clobbering**, matching the git-hooks installer's
  posture: a real directory where a managed symlink belongs is
  `UnmanagedDir` (checked with `symlink_metadata`, never
  followed — the agent reinstalled itself), a target canonicalizing
  outside `$HOME` is `EscapesHome`, and deleting a profile that a
  live symlink or registry entry still points into is
  `ActiveProfileDelete`.
- **Trust boundaries** — `[agents.profiles]` is stripped outright
  from remote-fetched configs (`Config::mark_remote`), since a
  hostile config suggesting a profile switch is a
  credential-redirection primitive. `jarvy ticket create` bundles
  exclude `agent-profiles/` entirely, and the ticket env allowlist
  omits `CLAUDE_CONFIG_DIR` / `CODEX_HOME`.
- **Identity-only snapshots** — a profile captures credentials,
  settings, skills, MCP registrations and plugin selections, and
  skips what an agent dir merely accumulates: conversation
  transcripts, re-fetchable package / extension / marketplace
  trees, log databases and scratch. On a real machine that is the
  difference between 2.3 GB and 2.9 MB per profile. The rule set is
  a denylist, so an unrecognized new file is kept rather than
  silently dropped. Whether it applies at all is carried by a
  `CopyPolicy`: `Snapshot` filters, `Relocate` cannot — the
  cross-device move fallback deletes its source, so anything skipped
  there would be destroyed rather than left behind. `create --from`
  clones through the same filter, resolving each top-level directory
  to its agent so one identity's transcripts never seed another's
  profile. Cursor's `extensions` tree is deliberately kept: cursor is
  symlink-tier, its snapshot *is* the live config dir, and nothing
  re-installs a dropped extension. Live IPC endpoints
  (`~/.codex/ipc/ipc.sock`) are skipped by file type; `fs::copy`
  fails on them and used to abort the whole snapshot.
- **`init --agents <slugs>`** narrows the snapshot, so the env tier
  can be captured while a symlink-tier editor is still open — the
  symlink tier *moves* the live config dir out from under it. A
  narrowed run only speaks for the agents it named, so it leaves an
  existing `default_profile` alone (it still claims one on a virgin
  store, or `use` would have no default to fall back on).
- **`[agents.profiles] prefer` setup hint** — `jarvy setup` compares
  each switchable agent's live profile against the repo's preference
  and prints a hint when they diverge; `strict = true` raises it to a
  stderr warning. It never auto-switches and never fails setup:
  switching a profile swaps the credentials an agent authenticates
  with. An agent that isn't profile-managed at all is counted as
  unmanaged rather than mismatched, so the hint doesn't fire at users
  who simply aren't using profiles. The env tier is read from
  `CLAUDE_CONFIG_DIR` / `CODEX_HOME`, not `profiles.json` — the
  registry records only the last *global* switch, so a shell that just
  switched would otherwise still be nagged.
- **`save [name]` refreshes snapshots from live**; symlink-tier agents
  report a no-op because their `~/.slug` IS the profile snapshot (the
  live dir is a jarvy-managed symlink into the store, so edits already
  land in the profile). Without `name`, each agent saves to whichever
  profile it is currently on — env tier reads the config-dir env var,
  symlink tier reads the live link.
- **`restore <name>` copies snapshots back over live dirs** (env tier)
  or re-points the symlink (symlink tier). Env-tier restores target
  the DEFAULT config dir (`~/.claude`, `~/.codex`) so a fresh shell
  with no env vars picks up the restored profile. Refuses cleanly when
  the profile has no snapshot for a targeted agent.
- **All six agents are now switchable.** windsurf, cline, and
  continue graduated from storage-only — their `~/.slug` dirs snapshot
  the same way cursor's do (per-agent denylist stays empty for now, so
  everything is kept). A `use --global` for one of them re-points its
  live symlink identically. A running-IDE probe (`pgrep -x` on unix,
  `tasklist /FI` on windows, 1s cap, fail-safe false) refuses the
  swap for cursor / windsurf when the editor is running unless
  `--force` is passed; cline / continue skip the probe (in-editor
  extensions have no separate binary to look for). The probe
  short-circuits to `false` under `cargo test` so the developer's own
  editor state can't leak into unrelated tests.
- **`check-cwd` + shell `cd` hook** — the `jarvy shell-init` snippet
  (bash `PROMPT_COMMAND` / zsh `chpwd_functions` / fish
  `--on-variable PWD`) runs `jarvy agents profile check-cwd` on every
  directory change. When the entered directory contains a `jarvy.toml`
  with `[agents.profiles] prefer` that doesn't match the live profile,
  one stderr line points at `jp <name>`. Debounced per (session, repo)
  for 4 h via a 0600 state file at
  `~/.jarvy/agent-profiles/.cwd-hint-state.json` (keyed by
  `sha256(session_id + "|" + repo_root)`) — a switch in *this*
  terminal silences the hint for THIS repo in THIS session without
  silencing sibling shells. Bash uses a `$PWD` compare so most prompts
  skip the jarvy spawn entirely. Opt out with `JARVY_NO_CWD_HINT=1`.
- **Telemetry** — new gated `agent_profile.*` event family. Profile
  *names* never reach telemetry; events carry counts and bounded
  agent slugs only. Per-path skips are `debug!`, so each snapshot
  also emits an info-level `agent_profile.snapshot_completed`
  carrying the file / byte / excluded / skipped counts — a denylist
  that stops matching shows up as `subtrees_excluded` collapsing
  while `bytes` jumps, without shipping a row per file. A failed
  snapshot emits `agent_profile.snapshot_failed` naming the agent
  that aborted the run, which `op_failed` (subcommand-scoped) cannot
  say; `op_failed` itself now carries a `stage`, separating a failed
  copy from a failed registry write — only the latter leaves the
  store half-applied. `create --from` reports its own
  `agent_profile.clone_completed` roll-up rather than borrowing the
  per-agent event with a sentinel slug.

Known v1 limits: no drift detection and no git-backed sync between
machines (both v1.1). `jarvy skills install` and `jarvy mcp
register` still write to an agent's default config path rather than
the env-redirected profile of the calling terminal — run them with
the target profile active globally, or re-run after switching.

**Features — tool freshness advisory (PRD-057):**

- **`jarvy check-updates`** and a companion `[maintenance]` block —
  compares installed provisioner / `[cargo]` / `[npm]` / `[pip]` /
  `[gem]` / `[go]` / `[nuget]` tools against upstream latest via
  package-manager-native shell-outs. Fourteen backends ship in v1:
  brew, apt, dnf, pacman, apk, winget, choco, scoop, cargo, npm,
  pip, gem, go, nuget. Advisory only — never blocks setup, never
  auto-upgrades. Results land in `~/.jarvy/update-cache.json` and
  `jarvy setup` reads that cache (sub-millisecond, no network) to
  print a one-line summary, then spawns a detached background
  refresher via `process_group(0)` (Unix) / `DETACHED_PROCESS`
  (Windows) so the cache warms for the NEXT invocation. First
  setup on a fresh box shows no summary (empty cache); run #2
  onward shows the previous run's data. See `docs/maintenance.md`.
- **Parallel backend probes** — rayon fan-out with
  `max(2, num_cpus/2)` workers over the cache-miss set. Cache
  hits skip the pool entirely so a fully warm run is O(N)
  file-reads rather than O(N) network waits. Report ordering is
  deterministic (sorted by tool name) so JSON output stays
  diff-stable across runs.
- **Lockfile guard** at `~/.jarvy/update-cache.lock` prevents
  concurrent refreshers from stomping on each other's cache
  writes. RAII drop cleans up on exit; stale locks (>1h) get
  reclaimed automatically for the crashed-refresher case.
- **`WindowsInstall.scoop` field** on `ToolSpec` — scoop bucket
  names diverge from winget IDs in enough cases (`gh` vs
  `GitHub.cli`) that reusing the winget id was misclassifying.
  Tool defs can now declare a real scoop bucket via
  `windows: { scoop: "..." }`; existing winget-only defs are
  unaffected.
- **Trust boundaries** mirror the rest of Jarvy's `allow_remote`
  pattern: `[maintenance] check_updates = false` opts out,
  `JARVY_CHECK_UPDATES=0` is the env kill-switch,
  sandbox/CI/non-TTY skip the *spawn* (cache reads still fire),
  and remote-fetched configs cannot enable `check_updates`
  without `allow_remote = true`.
- **Telemetry** — new `maintenance.*` event family (all gated on
  `[telemetry] enabled`) with per-tool `stale_tool` events plus
  a `jarvy.maintenance.stale_tools` gauge metric tagged
  `machine_id + platform` for fleet dashboards.

**Security:**

- `~/.jarvy/logs/` is now created 0700 and every `jarvy.log*` file in
  it is narrowed to 0600 at startup, including files written by earlier
  releases. The rotating appender opened with the process umask, so a
  default 022 left logs world-readable — and the sanitizer is a
  denylist, redacting the secret shapes it recognizes and writing
  everything else verbatim.
- `registry.fetch.*` events no longer emit the per-tool fetch URL
  unredacted. It is derived from the configured registry URL and so
  inherits any embedded `user:pass@`, which `registry_url` was already
  scrubbing on the same event.

## [v0.6.6] — quiet by default under non-TTY, dotfiles, personal overlay, service preflight (2026-07-22)

Bundles the material soaked as `v0.6.5-rc.1..rc.3` plus a new chatter
gate that lands the "why is jarvy shouting at me from my npm predev?"
fix reported after rc.3.

**Features:**

- **Silent by default when invoked non-interactively.** New `chatter`
  gate suppresses the setup narration (`Detecting Platform is: macos`,
  `Checking tool versions...`, `=== AI Hooks ===`, …) AND caps console
  tracing at WARN when `stderr` isn't a TTY — so `jarvy setup` from
  an npm `predev` script, `bun install` postscript, or CI runner no
  longer buries the caller's terminal in INFO-level output. File
  appender (`~/.jarvy/logs/jarvy.log`) and OTLP sinks still get the
  full INFO stream — the debug artifact and support tickets stay
  whole. Precedence for reopening the narration: env
  `JARVY_CHATTER=1|0` > `[logging] chatter = true|false` in
  `jarvy.toml` > `-v` on setup > `--quiet` > TTY auto-detect.
  Interactive terminals are unchanged.
- **`[dotfiles]` block** — cross-machine dotfile sync via `chezmoi`,
  `yadm`, or `stow`. `manager`, `repo` (URL or `github:owner/repo`
  shorthand), `apply` (auto-apply after clone, default true), and
  `allow_remote` (trust gate for remote-fetched configs, mirrors
  `[git_hooks]`). Setup runs `chezmoi init --apply <repo>` on first
  invocation and `chezmoi update` afterward (equivalent flow for
  `yadm`); `stow` is installed but not auto-applied — the per-package
  model doesn't fit a single verb. Advisory: setup never fails on
  dotfiles errors. See `docs/dotfiles.md`.
- **`~/.jarvy/jarvy.toml` personal overlay.** A user-scope overlay with
  the SAME schema as a project `jarvy.toml`, auto-merged into every
  project on load so personal tools, skills, hooks, MCP servers, and
  git identity follow you across machines. Merge semantics mirror
  workspace inheritance (project wins on collision, `[provisioner]`
  merges tool-by-tool). Overlay is tagged `Local` origin so personal
  `library_sources` and `allow_custom_*` are honored. Opt out with
  `JARVY_NO_PERSONAL_CONFIG=1`. Safety gates refuse symlinks,
  foreign-owner files, world-writable perms, and files over 1 MiB.
- **Container-runtime preflight for `services` commands** — Docker
  Compose and Podman Compose now probe `docker info` / `podman info`
  with a wall-clock cap before `compose up`, and surface targeted
  hints (Colima, Docker Desktop, `systemctl --user start docker`,
  `podman machine start`) when the daemon binary is present but the
  socket isn't answering. Podman Compose is auto-selected when Podman
  is installed and Docker isn't; docker-compose files with explicit
  `podman:` blocks override detection.
- **`jarvy diagnose <k8s-tool>` cluster liveness.** `kubectl`,
  `minikube`, `kind`, and `k3d` now route through a hard 2-second
  probe budget (`--request-timeout=2s` for kubectl; equivalent
  wall-clock caps for the others via `probe_with_timeout`) so a hung
  apiserver no longer stalls diagnose for 30 seconds. Advisory —
  diagnose remains read-only.

**Fixes:**

- **`jarvy setup --dry-run` prints the plan even under non-TTY.** The
  new chatter gate above silences the setup narration by default when
  `stderr` isn't a terminal — but a dry-run's whole point is to
  *preview* what would happen, so the `[DRY-RUN] Would install …`
  plan lines must always print. Dry-run now folds into the `verbose`
  axis of the chatter gate; `JARVY_CHATTER=0` still wins if you
  really want silence. Caught by `e2e_dry_run_mode` on the first
  v0.6.6 tag attempt (blocked pre-publish by the release-gate test
  workflow).
- **Dotfiles argv-injection guard.** The `[dotfiles] repo` field is
  now validated against leading-`-` values (CVE-2017-1000117-class
  option-injection into `git clone`) and NUL bytes; `chezmoi` and
  `yadm` invocations scrub `GIT_*` and `CHEZMOI_*` code-execution
  env vars before spawning.
- **Dotfiles telemetry never emits raw subprocess stderr.** `git` and
  `chezmoi` echo the repo URL (with any embedded token) on auth
  failure — the phase now emits a bounded `error_kind` label
  (`auth` / `network` / `not_found` / `conflict` / `binary_missing`
  / `permission_denied` / `spawn_failed` / `other`) and keeps the
  human message caller-only.
- **`jarvy diagnose <cmd>` no longer shells the fix command through
  `sh -c`.** The Tier 2 fix runner now argv-splits with a shared
  helper so command-substitution / redirection / metachars in a fix
  string can't reach a shell interpreter.
- **Personal overlay path field removed from telemetry.** The path
  contained `$HOME`, which on macOS and Windows is the account name
  — PII-adjacent. The overlay-applied event is also now debug-level
  (was info), because it fires on every jarvy invocation and would
  otherwise dominate log budgets.
- **Windows command-detection.** `command_on_path` now consults
  `PATHEXT` correctly — a Windows-only regression where `docker.exe`
  and friends registered as "missing" when only the base name was
  probed.

**Internal:**

- New `SkipReason` enum + `HasOrigin` trait consolidate the copy-paste
  trust-gate matching that had grown across `config.rs`,
  `dotfiles.rs`, `git.rs`, `git_hooks.rs`, and `packages/mod.rs`.
- `services::compose_command` memoized so parallel `compose up` /
  `compose down` invocations don't each re-probe `podman-compose` vs
  `podman compose`.
- Cookbook: `docs/cookbook/personal-workstation.md` walks through
  wiring the personal overlay + `[dotfiles]` + `[git]` into a
  one-command laptop setup.

## [v0.6.4] — task-runner lifecycle hooks + jr one-command setup (2026-07-17)

**Features:**

- **npm-style lifecycle hooks for `jarvy run`** — if `pre<name>` or
  `post<name>` keys exist in `[commands]`, `jarvy run <name>` runs them
  around the main command with npm's exact semantics: a failing pre
  aborts the run (its exit code propagates), post only runs after a
  successful main, a failing post fails the run, and extra `--` args go
  to the main command only. The more readable colon spelling works too
  (`"pre:build"` / `"post:build"`, quoted — TOML bare keys can't
  contain `:`); when both spellings are defined the colon form wins and
  a note is printed. Hooks are ordinary entries: listed, directly
  runnable, same safety guards.
- **`jarvy shell-init --apply`** — wires the `jr` (= `jarvy run`)
  shorthand into your shell rc file with one command, whatever way you
  installed jarvy (previously nothing did this automatically —
  `cargo install` users never got `jr` without hand-editing their rc).
  Idempotent; supports bash/zsh/sh/fish/PowerShell, and nushell via a
  materialized `~/.jarvy/init.nu` (re-run `--apply` after upgrading).
- Bare `jarvy run` with no `[commands]` table now prints a real
  getting-started guide (worked example, the three usage forms, cwd
  rule, `jr` tip) instead of a one-line hint.
- New docs page: [Task runner](https://github.com/Cliftonz/jarvy/blob/main/docs/run.md)
  — npm-run-style documentation including a "Coming from npm?" mapping
  table.

**Fixes:**

- Every jarvy invocation warned `plugin directory has insecure
  permissions` for users who never created `~/.jarvy/tools.d` — the
  permission probe conflated "directory doesn't exist" with "unsafe
  permissions". An absent plugin directory is now a silent no-op; the
  security gate still fires for directories that exist with loose
  permissions.

**Internal:**

- `verify-release-assets.sh` accepts the bare-basename `SHA256SUMS.txt`
  entries introduced in v0.6.3 (its listing regex previously required a
  path separator before the name).
- This repo's own `jarvy.toml` gained a `[commands]` table — jarvy's
  development tasks now dogfood `jarvy run`.

## [v0.6.3] — updater reliability + installer integrity (2026-07-16)

First release under the gate model (see `docs/release-testing.md`):
soaked as `v0.6.3-rc.1` for the 24h patch window because it changes the
update chain.

**Security:**

- **`install.sh` / `install.ps1` checksum verification actually fires
  now.** Two stacked bugs meant `curl | bash` and PowerShell installs
  were silently skipping integrity verification: `SHA256SUMS.txt`
  entries carry build paths that the lookup never matched, and under
  PowerShell 7 the sums file arrived as raw bytes that never split into
  lines. Both installers now match entries by basename and decode the
  manifest explicitly — a tampered download aborts the install on every
  platform. Caught by the new installer end-to-end suite on its first
  run. `SHA256SUMS.txt` entries are also generated as bare asset names
  going forward (the pathed entries likewise broke Chocolatey's publish
  step on v0.6.1).

**Fixes:**

- `jarvy update --rollback` no longer fails with `Text file busy` on
  Linux — the restore path wrote over the running executable; it now
  uses the same atomic temp-rename as the forward update. The consumed
  backup also no longer leaves an orphaned file in `~/.jarvy/backup/`.
- Windows binary updates retry the final swap (3× with backoff) when
  Defender or an indexer briefly holds the freshly extracted binary
  (`Access is denied` flake).
- Release binaries embed the exact release tag: an rc build reports
  `jarvy 0.6.3-rc.1` from `--version` instead of the bare crate
  version, and the update checker compares true prerelease versions —
  beta-channel users were previously never offered the next rc.
- Unknown subcommands (`jarvy rollback`, typos) exit `2` in
  non-interactive contexts instead of printing a menu prompt and
  exiting `0` — scripts and CI can no longer mistake a typo for
  success. Humans at a TTY still get the interactive menu.

**Changed:**

- Hooks run under `bash` (or `sh`) on Unix instead of the user's
  `$SHELL`. zsh's lack of word splitting silently broke POSIX-style
  hooks depending on who ran them — the same `jarvy.toml` now behaves
  identically for every developer. Opt back into a specific shell with
  `[hooks.config] shell = "zsh"`.
- A pinned `node` version installs through nvm when nvm is configured
  (`nvm install <ver> && nvm alias default <ver>`), so `node = "24"`
  is honored and a second package-manager node no longer shadows nvm's.
  Ranges and `latest` keep the platform installer.

**Known issues:**

- Upgrading **from v0.6.2 on Windows** with `jarvy update --method
  binary` can intermittently fail with `Access is denied` — the retry
  fix ships in this release but the binary performing that upgrade is
  v0.6.2's. Re-run the update or use the install script. Upgrades from
  v0.6.3 onward retry automatically.

**Internal:**

- Release pipeline hardening after the v0.6.1 incident: a
  draft-verification gate (checksums, cosign, SBOMs, and a binary
  `--version` smoke) must pass before any release publishes; the
  Cargo.toml↔tag guard refuses to build a mislabeled tag; and the
  installer/package e2e suites now actually run on every release.
  Soak-window automation is bump-kind aware and no longer labels
  force-dispatched validation runs (#53).

## [v0.6.2] — nvm detection fix (2026-07-14)

Corrected re-release of the withdrawn v0.6.1 — identical code plus the
version bump v0.6.1 was missing and a release-workflow guard that makes
the mistake impossible to repeat (the tag now refuses to build unless
Cargo.toml matches it).

**Fixes:**

- nvm is now detected via its filesystem marker
  (`${NVM_DIR:-$HOME/.nvm}/nvm.sh`) instead of shell probes. nvm is a
  shell function, not a binary, so the previous PATH lookup and
  `bash -lc` probes could never see it — every `jarvy setup` with `nvm`
  in the config re-downloaded and re-ran the installer, and the version
  check reported nvm as needing install forever. Detection now converges
  after one install, and `jarvy upgrade node` correctly suggests
  `nvm install` when node is nvm-managed. (Windows is unaffected:
  nvm-windows is a real binary.)

**Internal:**

- CI action bumps (Dependabot): `actions/checkout` 4→7,
  `actions/download-artifact` 7→8, `actions/setup-java` 4→5,
  `azure/setup-helm` 4.3.0→5.0.1, `docker/setup-buildx-action` 3→4.
  The release-critical ones were smoke-validated by dispatch runs
  before this release.

This is also the first release upgradable *to* via
`jarvy update --method binary` from v0.6.0 — the redirect fix shipped in
v0.6.0 makes the updater work going forward (upgrading *from* v0.5.x
still requires the install script or a package manager).

## [v0.6.1] — WITHDRAWN (2026-07-14)

Withdrawn ~40 minutes after publication: the version-bump commit was
missed before tagging, so every binary and package asset self-identified
as `0.6.0`, breaking `jarvy update` with a perpetual update loop. No
downstream channel consumed it (crates.io skipped, Chocolatey failed on
the name mismatch). Use [v0.6.2] — the corrected re-release of the same
code.

## [v0.6.0] — jarvy run, editor integrations, git config hardening (2026-07-13)

**Known issues:**

- Upgrading **from v0.5.x** with `jarvy update --method binary` fails with
  `Checksum verification failed`: the v0.5.x updater does not follow the
  HTTP redirects GitHub now serves on release-asset downloads (fixed in
  this release, but the fix cannot reach the old binary doing the
  upgrade). Workaround: reinstall via the install script
  (`curl -fsSL https://raw.githubusercontent.com/Cliftonz/jarvy/main/dist/scripts/install.sh | bash`)
  or your package manager. Upgrades *from* v0.6.0 onward work.

**Features:**

- `jarvy run [name] [-- args]` — npm-run-style command runner for a
  `[commands]` table in `jarvy.toml`. An explicitly named command is
  treated as consent (shell chaining allowed; the command is printed
  ANSI-stripped before exec), the child's exit code is propagated, and
  the working directory is the config file's directory. Bare `jarvy run`
  lists the available commands; there is no implicit `cargo run`/`cargo
  test` fallback, and NUL-bearing (on Windows, also `%`-bearing)
  `-- args` are refused. `jarvy shell-init` emits a snippet defining `jr`
  as shorthand, and shell-init/completions gain **nushell** support
  alongside bash/zsh/fish/sh/powershell. Telemetry: `run.command.*`,
  `shell_init.generated`, `completions.generated`.
- `jarvy doctor --check <path,tools,hooks>` — limit the health run to
  specific categories (system info is always shown as context). Skips the
  unselected probes entirely; an unknown category exits `2` with the valid
  set. Composes with `--tools` and `--extended`. Closes PRD-027.
- `jarvy skills update [<name>]` — re-fetch skills from `library_sources`,
  compare the advertised version/sha against the installed sidecar per
  agent, and reinstall only on divergence (no-op when unchanged; a pinned
  `[skills.install]` version that no longer matches the library refuses
  rather than silently drifting). PRD-049 phase 2.
- `jarvy skills remove <name>` — uninstall `SKILL.md` + the
  `.jarvy-skill.json` sidecar from every targeted agent. Idempotent:
  removing an absent skill is a clean no-op. User-added companion files
  next to `SKILL.md` are preserved.
- Ad-hoc `jarvy skills install <name>` — a name absent from
  `[skills.install]` now resolves from the configured `library_sources`
  at `latest` instead of erroring (the `--name` flag is replaced by a
  positional argument).
- All three subcommands accept `--format json` (PRD-051), and emit
  `skills.updated` / `skills.removed` telemetry events (gated behind the
  standard telemetry opt-in).
- `jarvy setup` observability flags are now wired into the tracing
  subscriber: `-v/-vv/-vvv` raise console verbosity, `-q` suppresses all
  but errors (without starving `jarvy.log` / OTLP), `--log-format json`,
  `--log-file <path>`, `--debug-filter <module>`, and `--profile` /
  `--profile-output` (phase-level timing report). These flags previously
  parsed but were silently dropped.
- 15 new tools: `allure`, `aws-sam-cli`, `cfn-lint`, `cypress`,
  `goaccess`, `k3s`, `linkerd`, `locust`, `microk8s`, `playwright`,
  `putty`, `structurizr`, `task`, `tmux`, `todoist`. `structurizr` is
  the 2026 consolidated "software architecture models as code" tool
  (C4 model) — installs via Homebrew / Linuxbrew (the old,
  now-deprecated `structurizr-cli` formula is not used); no first-party
  winget/choco, so Windows uses the `.war` or Docker image. `k3s` and
  `microk8s` cover single-node local Kubernetes: k3s is Linux-only and
  installs from a commit-pinned, sha256-verified copy of the upstream
  installer (no floating `get.k3s.io` fetch); microk8s installs via snap
  on Linux or Canonical's Multipass-backed Homebrew tap on macOS.
- `[git]` gains three capabilities (see `docs/git-config.md`):
  - `[git.extra]` — a free-form escape hatch for git config keys Jarvy
    doesn't model as typed fields (`core.fsmonitor`, `feature.manyFiles`,
    …), applied last so they override modeled fields. Each entry runs a
    layered guard: dotted-grammar/flag-injection key validation, a
    leading-`-` value refusal (argv option-injection), an **outright
    refusal of keys whose value git executes** (`core.pager`/`editor`/
    `sshCommand`/`askPass`/`hooksPath`, `pager.<cmd>`, `filter.*.clean`,
    `*.textconv`, `merge.*.driver`, `remote.*.uploadpack`,
    `init.templateDir`, … — RCE the `!`-filter alone missed;
    `core.fsmonitor=true|false` stays allowed) overridable with
    `JARVY_ALLOW_GIT_EXEC_KEYS=1`, a security-guardrail-downgrade check,
    and a `!`-shell-value refusal (leading whitespace included). The
    typed `editor`/`credential_helper` fields (which legitimately set
    such keys) are value-guarded instead: a bare command/helper + flags
    is allowed, a shell metacharacter / `!` / program-path value is
    refused. The dry-run preview runs the same gauntlet, so it shows
    refusals rather than claiming a blocked key "would be set."
  - `os_defaults` (default on; `os_defaults = false` opts out) — writes
    host-aware defaults for **unset** keys: `core.autocrlf`
    (Windows `true` / else `input`), Windows `core.longpaths`, macOS
    `core.precomposeunicode`, plus a cross-platform recommended set
    (`fetch.prune`, `rerere.enabled`, `merge.conflictStyle = zdiff3`).
    Explicit fields and `[git.extra]` always win; keys already matching
    are skipped so re-runs don't re-write.
  - Security guardrail — `[git.extra]` values that weaken a git defense
    are refused (`core.protectNTFS`/`protectHFS = false`,
    `safe.directory = *` (CVE-2022-24765), `safe.bareRepository = all`,
    `fsck.*`/`fetch.fsck.*`/`receive.fsck.* = ignore`,
    `*.fsckObjects = false`, `http[.<url>].sslVerify = false`) unless
    `JARVY_ALLOW_GIT_PROTECT_DOWNGRADE=1`.
  - Trust boundary — a remote-origin config (`--from <url>`) cannot apply
    `[git]` unless it sets `allow_remote = true`, and even then writes are
    forced to `--local` scope (never `~/.gitconfig`). `jarvy setup
    --dry-run` previews every OS-default and `[git.extra]` write. Full
    `git_config.*` telemetry — lifecycle, adoption (`extra_applied`,
    `os_defaults_applied`), and security refusals/overrides — all gated
    behind the telemetry opt-out.
- Default post-install hooks for `rust` (clippy/rustfmt + cargo env),
  `tmux` (TPM plugin manager), `kubectx` (kctx/kns aliases), and `nvim`
  (starter `init.lua`). Unix-only where the tool also ships on Windows.

**Added:**

- Library manifests can now reference item bodies by URL instead of
  inlining them (PRD-054 companion-fetch phase): `ai_hook` items honor
  `bash_url` / `powershell_url` (each requiring a matching `*_sha256`
  pin), and `skill` items install their `companion_files` (templates,
  helper scripts) alongside `SKILL.md`. Every companion fetch is
  HTTPS-bounded, sha256-verified against the manifest pin (no
  unverified fetch path), cached content-addressed under
  `~/.jarvy/library.d/companions/`, and skill companion filenames are
  path-safety validated so a hostile manifest cannot escape the skill
  directory. New telemetry: `library.companion.{fetched,sha_mismatch,fetch_failed,refused_filename}`.
- **npm wrapper** (`packages/npm/`, PRD-021): `jarvy-mcp` npm package that
  downloads the platform-native jarvy release binary on install
  (SHA256-verified against the release `SHA256SUMS.txt`, mirroring
  `install.sh`) and exposes `jarvy-mcp` (runs `jarvy mcp`) and `jarvy`
  bins. `npx -y jarvy-mcp` is now a zero-install MCP client command.
  Not yet published to npm — maintainer action.
- **Docker image** (`dist/docker/Dockerfile`, PRD-021): multi-stage
  Alpine image that builds jarvy from source, runs as a non-root user,
  pins `JARVY_MCP_WORKSPACE=/workspace`, and defaults to the MCP server
  entrypoint. `.github/workflows/docker-publish.yml` builds
  linux/amd64 + linux/arm64 to `ghcr.io/cliftonz/jarvy`, gated on
  release tags / manual dispatch only.
- **MCP registry submission prep** (`dist/mcp-registry/`, PRD-021):
  `server.json` for the official MCP registry
  (`io.github.cliftonz/jarvy`, validated against the `2025-12-11`
  schema) plus maintainer submission runbook. Ownership proofs are
  pre-wired: `mcpName` in the npm package.json and the
  `io.modelcontextprotocol.server.name` OCI label in the Dockerfile.
  Not submitted — requires the maintainer to publish npm + ghcr first.
- CI e2e coverage for Fedora, Arch, and Alpine via distro containers on
  the free ubuntu runner (PRD-014).
- **Published GitHub Action** (top-level `action.yml`, PRD-017 phase 2):
  installs jarvy (checksum-verified) and runs validate/setup/doctor, with
  caching, outputs, and a problem matcher. Self-tested on
  ubuntu/macos/windows. Referenceable as `Cliftonz/jarvy@v1` once tagged.
- **VS Code extension** (`editors/vscode/`, PRD-017 phase 3): live
  `jarvy.toml` diagnostics, setup/doctor/validate commands, status bar,
  and an unknown-tool quick fix. Compiles clean under strict `tsc`.
- **JetBrains/IntelliJ plugin** (`editors/jetbrains/`, PRD-017 phase 6):
  Tools ▸ Jarvy actions + a TOML ExternalAnnotator driven by
  `jarvy validate`. Builds via `./gradlew buildPlugin` (IntelliJ Platform
  Gradle Plugin 2.x, JDK 21).
- Consumable pre-commit hook (`.pre-commit-hooks.yaml`) and devcontainer
  feature (`dist/devcontainer/jarvy/`), PRD-017 phases 4–5.

**Changed:**

- `rust` migrated from a bespoke installer to the `define_tool!` macro
  (so it can carry a default hook); removed a duplicate `rust` entry from
  the tool index.
- Removed ~1.3k LOC of unwired diagnostic surface: the
  `observability::{network_trace, bundle}` modules (the latter duplicated
  the wired `jarvy ticket` bundler) and the per-tool/per-network arms of
  the profiler. `--profile-output` durations now serialize as integer
  `*_ms`, matching the `duration_ms` telemetry contract.

**Fixes:**

- `jarvy update --method binary` now follows HTTP redirects when
  downloading release assets, fixing `Checksum verification failed` on
  every binary self-update (GitHub serves release-asset downloads via
  redirect). This was the sev-1 that v0.6.0-rc.2 was cut for; see the
  known-issues note above for upgrading *from* v0.5.x.
- `cargo jarvy new-tool <name>` scaffolding no longer generates
  uncompilable module wiring.
- The ghcr.io Docker image build no longer fails with ``linker
  `musl-gcc` not found``: the x86_64-musl linker pin in
  `.cargo/config.toml` (needed by the Ubuntu cross-build) is overridden
  back to `cc` inside the Alpine builder stage.
- The JetBrains plugin ID was renamed `com.jarvy.intellij` → `com.jarvy`
  before first Marketplace publication — the Plugin Verifier hard-fails
  IDs containing template words like "intellij", which broke the weekly
  `verifyPlugin` CI job.
- `install.sh` no longer exits `1` after a successful install. The
  `EXIT`-trap cleanup referenced a `local tmp_dir` that was out of scope
  when the trap fired at script exit; under `set -u` that threw
  `tmp_dir: unbound variable` and failed every install right after it
  reported success. The trap now bakes the resolved path in at
  registration time. (Caught by the new GitHub Action self-test.)
- `install.sh` now retries transient GitHub failures (3× with backoff)
  and honors `GITHUB_TOKEN` / `GH_TOKEN` for the authenticated
  5000/hr API rate limit, so a single rate-limited request no longer
  hard-fails `get_latest_version` or silently skips the checksum
  fetch when `SHA256SUMS.txt` is actually present.
- The `curl … | bash` installer (`install.sh`) now verifies the downloaded
  archive against the release `SHA256SUMS.txt` before extracting it. The
  checksum routine existed but was never called, so installs ran with no
  integrity check at all; the PowerShell installer (`install.ps1`) had the
  same dead-code gap and is fixed too. A mismatch aborts the install; set
  `JARVY_SKIP_CHECKSUM=1` to opt out.
- Fix `curl … | bash` failing with a download 404 on 64-bit glibc Linux —
  the most common platform. `install.sh` requested an
  `x86_64-unknown-linux-gnu` asset, but Jarvy ships only the static
  `x86_64-unknown-linux-musl` build for x86_64, so nothing matched. The
  installer now requests the triple that actually ships (x86_64 → musl,
  aarch64 → gnu, armv7 → gnueabihf).

**Internal:**

- Log reading and stats (`jarvy logs view` / `logs stats`) no longer
  allocate per line on the no-sanitization fast path.
- New searchable tool directory in the docs site with per-OS install
  commands for every registered tool.
- Add end-to-end coverage for the download → install → upgrade path, which
  previously had none: unit tests for `install.sh` (checksum accept/reject,
  channel filter, platform-triple mapping), a `shellcheck` / `bash -n` /
  PowerShell-parse lint gate, an installer e2e workflow that runs
  `install.sh` and `install.ps1` against a real published release, and a
  package-install matrix (`.deb` → apt, `.rpm` → dnf, crate → cargo,
  `.msi` → Windows) that also asserts `jarvy update` auto-detects the
  install method.

## [v0.5.2] — Fix `jarvy update --method binary` (2026-07-06)

Patch release. `jarvy update --method binary` could not find a release
asset on any platform, so binary self-update failed for everyone; this
fixes it.

**Fixes:**

- Fix `jarvy update --method binary` failing with "No binary for this
  platform" (Windows) or a download error (macOS/Linux). The updater
  matched assets by an ad-hoc `<os>-<arch>` string (`linux-x86_64`,
  `darwin-aarch64`), but release assets are named by Rust target triple
  (`x86_64-unknown-linux-musl`, `aarch64-apple-darwin`,
  `x86_64-pc-windows-msvc`) since the binary-tarball change in v0.5.0,
  so nothing matched. The updater now requests the exact triple each
  platform ships. Other update methods (Homebrew, Cargo, apt/dnf/…)
  were unaffected.

**Internal:**

- Add a `package-verify` follow-through and make the telemetry
  disclosure tests hermetic against an ambient `JARVY_TEST_MODE`, so the
  Coverage and cross-platform E2E jobs are green again. crates.io
  publishing moved to Trusted Publishing (OIDC) with the API token kept
  as a fallback.

## [v0.5.1] — Fix `cargo install jarvy` + auto-publish to package managers (2026-07-05)

Patch release. The v0.5.0 crate published to crates.io was incomplete
and would not compile, so `cargo install jarvy` and the cargo path of
`jarvy update` both failed. This fixes that and hardens the release
pipeline so package managers actually receive each release.

**Fixes:**

- Fix `cargo install jarvy` failing to compile with `couldn't read
  .../assets/wizard-skill/SKILL.md`. The wizard embeds that file via
  `include_str!`, but it fell outside the `Cargo.toml` `include`
  allowlist, so `cargo publish` stripped it from the tarball. Local and
  CI builds passed because the file exists in the working tree — only
  the published crate was broken. If you hit this on v0.5.0, install
  v0.5.1 (or a prebuilt binary / Homebrew / the install script).
- Add a `package-verify` CI job that compiles the *packaged* crate via
  `cargo publish --dry-run`, so a missing embedded asset now fails at
  pull-request time instead of after publishing an immutable release.

**Release automation:**

- Auto-publish to package managers on every stable release. crates.io,
  Homebrew, and Chocolatey were being skipped because the `release:
  published` event does not trigger downstream workflows when the
  release is cut by CI's built-in token; the release workflow now
  dispatches the publish workflow explicitly (stable tags only).
- Fix the release job's downstream dispatches, which had been failing
  with a permissions error and silently skipping the post-release
  validation and publish workflows.
- Raise the release test-gate poll budget from 15 to 40 minutes so a
  slow Windows test leg no longer times the release out before it goes
  green.

## [v0.5.0] — Agent-driven wizard + polyglot detection + two review-plan sweeps (2026-07-04)

The wizard graduates from "spawn an agent and hope" to a hardened trust
boundary — per-invocation session UUID, Drop-guarded marker file at
`~/.jarvy/state/wizard-session-<uuid>.active`, single-turn playbook
that actually completes end-to-end. Alongside: a big polyglot
detection sweep (Node + PHP + Rust + Go all first-class, 13 niche
languages, 3 build systems, ~20 new rules, 10 new tools) and the
back-to-back execution of two full parallel-code-review enhancement
plans (32 + 38 items) that hardened almost every corner of the
diff.

**Distribution — prebuilt binaries on macOS + Windows:**

- Release assets now ship prebuilt binary tarballs (macOS) and zips
  (Windows) alongside the existing Linux builds (closes #30). Install
  without a Rust toolchain via the install script or by downloading the
  target archive directly.

**Wizard runtime — from "hangs on MCP prompt" to production-ready:**

- **Single-turn agent execution.** Prompt rewritten for
  `claude -p` / `codex exec --` — no interactive follow-up gates, no
  "await user approval" loops that never resolve. The agent runs the
  playbook top-to-bottom and prints a summary.
- **MCP mutation-gate bypass** (`JARVY_WIZARD_SESSION_ID=<uuid>` +
  marker file). Previous behaviour: agents piping stdin would fail
  closed on `prompt_mutation_confirmation`'s TTY read. New behaviour:
  descendant `jarvy mcp` server processes verify both the env AND a
  paired Drop-guarded marker file AND freshness (≤10 min) via
  `wizard::session::is_active()`. Orphaned env carriers (language
  servers, `nohup`-detached daemons that outlive the wizard) fail
  the check and fall through to the normal confirmation gate.
- **Trust boundary hardening.** `~/.jarvy/state/` created 0700
  (was 0755 — leaked session UUIDs to local users). Marker chmod'd
  0600 with chmod-failed / chmod-ignored telemetry. Bypass now
  refuses unexpected MCP clients (falls through to confirmation gate
  for anything outside `{claude-code, codex}`) instead of just
  warn-logging. Future-mtime capped at 5s clock skew.
- **`--allowedTools "mcp__jarvy"`** pre-approval on `claude -p` so
  the wizard doesn't hang on the first MCP call. Full argv pinned by
  test.
- **Silent-success debuggability.** `wizard.headless_exit` now
  carries `jarvy_toml_before / _after / terminal_state`
  (`playbook_completed | noop_already_configured | early_exit |
  unknown`) so on-call can distinguish "agent honored step-2 no-op"
  from "agent crashed" from "agent misread plan" without reading
  source.

**Polyglot detection — from "Rust + Node + Docker + Make" to full
first-class support for four ecosystems + 13 niche languages:**

- **Full 4-lang polyglot.** New rules for **PHP** (`composer.json` /
  `composer.lock` / `artisan` / `symfony.lock`), **Go** companions
  wired (`golangci-lint`, `air`, `delve` — were registered but had
  empty suggests), Rust companions wired (`bacon`, `cargo-nextest` —
  registered, previously silently dropped by the unknown-tool filter),
  lockfile-precise Node PMs (`pnpm-lock.yaml → pnpm`, `yarn.lock →
  yarn`, `bun.lockb → bun` become **required**, not blanket
  suggestions).
- **13 niche languages** — `deno`, `elixir` (+ erlang companion),
  `erlang`, `haskell`, `crystal`, `gleam` (+ erlang), `lua`
  (+ luarocks), `luarocks`, `nim`, `ocaml`, `scala` (+ Mill via
  `build.sc`), `zig`, `julia` (careful to only match
  `Manifest.toml` / `JuliaProject.toml`, not the ambiguous bare
  `Project.toml`).
- **Build systems + infrastructure** — `cmake` (`CMakeLists.txt`),
  `skaffold` (`skaffold.yaml`), `bazelisk` (Bazel launcher, catches
  `MODULE.bazel` / `WORKSPACE*` / `BUILD.bazel` / `.bazelrc`),
  `release-plz` (`release-plz.toml` + `cargo install --locked`
  install path), `git` (`.git/`), `gh` (`.github/`), `vscode`
  (`.vscode/`), `infisical` (`.infisical.json` / `.env.infisical`).
- **10 new tools** total: `bacon`, `cargo-nextest`, `release-plz`
  (cargo-install via new `install_via_cargo_install` helper),
  `composer`, `pnpm`, `yarn`, `cmake`, `skaffold`, `bazelisk`,
  `infisical`. Publisher-pin comments on the cargo-install trio
  document canonical crates.io owners for supply-chain traceability.

**Discover trust + perf hardening:**

- **`atomic_write` sensitive-key policy.** Refuses to persist any
  `jarvy.toml` containing top-level `[secrets] / [credentials] /
  [tokens] / [api_keys] / [auth]` — case-insensitive. `jarvy.toml`
  is chmod'd 0644 (world-readable) on the invariant that discover
  writes only sanitized tool names + versions; the refusal is
  belt-and-braces against a future contributor adding a
  `discover-can-also-cache-X` codepath. Emits
  `discover.sensitive_key_refused` at ERROR level.
- **`atomic_write` perms telemetry.** `jarvy.toml` now explicitly
  chmod'd 0644 (was inheriting `NamedTempFile`'s 0600 default —
  fresh clones + CI couldn't read the file). `discover.jarvy_toml_
  perms_unsafe` fires on chmod failure OR chmod-ignored (NFS /
  drvfs / exFAT silently drop).
- **Duplicate `[provisioner]` key fix.** A tool that was BOTH
  required (own marker) AND recommended (companion of another
  detection) — concrete case `release-plz.toml` + `.github/` —
  would land twice in `[provisioner]`, producing a duplicate-key
  TOML parse error on the next `jarvy discover` / `jarvy validate`.
  Dedup lives in `analyze_with` post-loop; `recommended_dropped_dup`
  count surfaces on `discover.applied`.
- **Dash / underscore name normalization.** Detection rules
  conventionally use dash (`release-plz`) — matches TOML keys under
  `[provisioner]` — while handlers register underscore
  (`RELEASE_PLZ → release_plz`, since Rust identifiers can't hold
  dashes). `known_tool_set()` now populates both forms so
  `known_tools.contains(...)` in `analyze_with` resolves either
  spelling. Startup-panic on collision.
- **`RootIndex` batches root `read_dir` once per pass** — was `N`
  syscalls + `O(dir_size)` allocations across 11 glob rules;
  now one syscall + `HashMap<OsString, PathBuf>` for exact lookups
  + pre-sorted `Vec<PathBuf>` for globs. On a 200-file root with
  20+ rules, ~35× syscall reduction.
- **`LazyLock<Regex>`** for `rust-toolchain.toml` and `go.mod`
  version patterns. Was `Regex::new` per version extraction — a
  30-100 µs compile paid per discover pass. Custom-rule patterns
  cache in a `RwLock<HashMap>`. Fast path returns
  `Cow<'static, Regex>` (borrowed) so even the LazyLock arms skip
  atomic RMWs.
- **`DetectionRule` / `Detection` Cow refactor.** `name` and
  `suggests` land as `Cow::Borrowed(&'static str)` for built-in
  rules constructed via `.into()` from string literals — zero
  allocations per matched rule. Custom rules deserialize as
  `Cow::Owned` via serde with no source changes. Regression guard
  test asserts every default-rule name + suggest is Borrowed.
- **`known_tool_set` memoization.** OnceLock cache — was rebuilding
  ~300 Strings per discover pass (register_all + name clones +
  dash / underscore aliasing). Startup-panic on alias collision so
  two tools normalising to the same form fail loudly at boot, not
  silently mis-dispatch.

**Wizard e2e infrastructure:**

- **`wizard_e2e_repo_matrix.rs`** — data-driven 12-shape matrix
  covering polyglot Node+PHP+Rust+Go, Laravel PHP, Bazel monorepo,
  K8s + skaffold, Elixir/Gleam BEAM, niche-lang clusters, Python +
  Infisical + VSCode, empty-repo negative shape. Each row
  asserts: preview JSON → apply JSON → `jarvy.toml` round-trips
  → 0644 perms → `jarvy validate` accepts → byte-identical second
  apply. Adding a shape = one row.
- **`polyglot_node_php_rust_go` shape** pinned at `min_warnings: 1`
  (node-without-nvm advisory) so a refactor stopping the emission
  trips instead of silently regressing the operator signal.
- **New niche-language + dir-marker table tests** in
  `discover::mod::tests` — 13 language markers, 3 dir markers, plus
  a `discover_does_not_walk_into_dot_git` regression guard that
  plants hostile `Cargo.toml` / `mix.exs` / `package.json` inside
  `.git/` and asserts they DON'T trigger detection.

**Observability contract:**

- **9 new events** (all telemetry-gated): `mcp.mutation.wizard_bypass`,
  `mcp.mutation.wizard_bypass_unexpected_client`,
  `wizard.session_token_activate_failed`,
  `wizard.session_token_perms_unsafe`, `wizard.session.bypass_refused`,
  `discover.recommended_dropped`, `discover.rules_loaded`,
  `discover.jarvy_toml_perms_unsafe`, `discover.sensitive_key_refused`.
- **6+ new fields on existing events**: `wizard.headless_spawned` gets
  `mcp_preapproval`, `wizard_session_env`, `wizard_session_id`;
  `wizard.headless_exit` gets `jarvy_toml_before / _after /
  terminal_state / wizard_session_id`; `discover.applied` gets
  `recommended_dropped_dup / detections_by_rule`.
- **`terminal_state` pinned via `pub const`** — four constants
  (`TERMINAL_STATE_PLAYBOOK_COMPLETED / _NOOP_ALREADY_CONFIGURED /
  _EARLY_EXIT / _UNKNOWN`) so alerting has a stable contract.
- **`mcp.auto_approve.*` events** now honour `telemetry_gate::
  is_enabled()` — closes a documented opt-in contract hole.
- **CLAUDE.md event taxonomy** fully updated with every new event +
  field addition. Stable contract for on-call queries.

**MCP audit trail correctness:**

- **`AuditAction::McpMutationRequested`** — pre-flight audit is now
  a distinct action from `McpMutation` (which is emitted only on
  completed mutations). Pre-fix, `gate_mutation` wrote
  `mcp_mutation success=true` BEFORE rate-limit / confirmation /
  execution, meaning failed mutations appeared successful in
  audit-log queries. Success entries now land at the three actual
  completion sites (Yes / Always / wizard-bypass).
- **Wizard-bypass double-audit fixed.** Pre-fix, the bypass arm
  wrote `log_mcp_mutation` twice per grant (once pre-flight, once
  in the arm), producing indistinguishable duplicate rows. The
  arm's log removed; pre-flight covers the request; the tracing
  event on grant carries forensic detail.
- **`effect_summary` ANSI-sanitized** before flowing to audit + tracing.
  Workspace-root paths are attacker-controllable via hostile repo
  directory names on `git clone`; a path containing ESC or CR/LF
  could inject fake log entries or clear the terminal when an
  operator pages the audit log.

**Perf sweep (execution of both review plans):**

- `BufWriter` around `ChildStdin` in `send_prompt_to_child_stdin` —
  20 KiB prompts now go out in one vectored write instead of
  PIPE_BUF chunks.
- `RootIndex::find_first_match` returns `Option<&Path>` — index
  outlives the loop by construction. Pre-fix cloned `PathBuf` per
  hit.
- `read_bounded` uses `Read::take + read_to_end` — skips the
  `resize(cap, 0)` memset. ~16 KiB per polyglot pass.
- `sanitize_source` byte-level — skips UTF-8 decode for an
  ASCII-only check.
- `known_tool_set` collision check uses a byte-fold single-walk.
- Toml parsed exactly ONCE per `discover` pass — was two full
  `toml::Table` parses (`parse_provisioner_pins` +
  `parse_discover_block` each parsed the same source).
- `detections_by_rule` fold-into-String — no throwaway `Vec<&str>`
  between `collect` and `join`.
- Session-check span (`debug_span!("wizard.session.check")`) — makes
  slow home directories (network mounts, macOS synced drives)
  visible to on-call.

**Test hardening:**

- **Two full parallel-code-review sweeps** (5 personas each: Rust
  perf, security, QA, observability, maintainability/DRY) — 32
  items in the first, 38 in the second. Every P0 and P1 closed
  across both.
- **Wizard-bypass code path** — was untested (a security-relevant
  trust hole). Now 5 unit tests + one integration test cover
  env-set → bypass fires, non-exact env values ignored,
  rate-limit still enforced, single-audit-row invariant,
  session::is_active helper. Plus a `filetime`-driven test that
  actually forces old mtime so the 10-minute staleness window is
  covered (pre-fix the test admitted it couldn't and asserted the
  opposite).
- **`RootIndex` direct coverage** — 6 new tests: exact lookup, dir
  filter, glob determinism, empty dir, only-dirs, missing project
  dir.
- **Cow perf regression guard** —
  `default_rules_name_and_suggests_are_all_borrowed` refuses to let
  a future `String::from(...).into()` construction silently reintroduce
  the ~15-20 heap allocations per polyglot pass.
- **`cargo_install_argv_pins_locked_flag`** — supply-chain contract
  for bacon / cargo-nextest / release-plz.
- **`classify_headless_outcome`** — table-driven exhaustive
  coverage of the 4-way terminal_state match.

**Breaking-shape additions (all opt-in, no existing config breaks):**

- New `~/.jarvy/state/` directory created 0700 on first
  wizard invocation. Nothing at 0.4.0 wrote here; nothing else
  reads it. Users downgrading to 0.4.0 after using 0.5.0's wizard
  will leave harmless orphan `.active` marker files behind.
- New env vars: `JARVY_WIZARD_SESSION` (bool), `JARVY_WIZARD_SESSION_ID`
  (UUID) set on wizard-spawned agents. Both benign if inherited by
  unrelated processes.
- New optional dev dep: `filetime = "0.2"`.

**Files (~40 touched):**

- New: `src/wizard/session.rs`, `src/tools/{bacon,bazelisk,
  cargo_nextest,cmake,composer,infisical,pnpm,release_plz,skaffold,
  yarn}/{mod,definition}.rs`, `tests/wizard_e2e_repo_matrix.rs`.
- Enhanced significantly: `src/mcp/extended_tools.rs`,
  `src/wizard/{headless,prompt}.rs`, `src/commands/wizard_cmd.rs`,
  `src/discover/{commands,mod,rules,scanner,version}.rs`,
  `src/tools/{common,mod}.rs`, `src/paths.rs`, `src/mcp/audit.rs`,
  `tests/{tools_matrix,wizard_e2e_repo_matrix}.rs`, `CLAUDE.md`.

## [v0.4.0] — Library registry / monorepo / discover / git hooks / 25-item review sweep (2026-06-29)

The biggest single milestone since v0.2.0 — eleven PRDs closed
across a multi-day push, plus the full close-out of the parallel-
code-review enhancement plan that ran against the new surface.

**Highlights:**

- **PRD-044 auto-discovery (full)** — `jarvy discover` scans the
  project tree, suggests tools, optionally `--apply`s into
  `jarvy.toml`. `[discover]` config block supports custom rule files;
  `--rules <path>` CLI flag overrides per-run. `--watch` mode
  re-runs on filesystem events. `FileContaining` detection pattern.
  Version-range narrowing suppresses re-suggestions when the pinned
  range already covers the detected version. `uninstallable` bucket
  surfaces ecosystems jarvy can't install (maven/gradle/dotnet)
  instead of silently dropping them. Continuous discovery in
  `jarvy setup` emits an advisory when new markers appear.
- **PRD-047 monorepo (full)** — `[workspace] members = [...]` with
  glob expansion (`apps/*`) + `exclude = [...]`. `jarvy setup
  --project <name>` runs setup against one member; `name = "current"`
  auto-detects. Implicit auto-context when cwd sits inside a declared
  member. New `jarvy context` diagnostic command. `jarvy drift` and
  `jarvy doctor` honor the same auto-context.
- **PRD-051 universal `--format json`** — every command that prints
  to stdout accepts `--format json`. Per-subcommand placement on
  drift/logs/ticket/services/workspace; exit codes match human path.
- **PRD-054 library registry (phase 5 + phase 6)** — cosign signature
  enforcement (real, not scaffolded). `jarvy library list/show/clean/sync`
  CLI + matching MCP tools.
- **PRD-048 git hook frameworks (full)** — pre-commit, husky,
  lefthook, native all ship as first-class handlers behind the
  unified `HookFramework` enum. `docs/replace-husky.md` walks
  through three migration paths.
- **PRD-055 git skill sources** — `git+https://...@<ref>[#<subpath>]`
  + `github:owner/repo@<ref>` URL schemes for `[skills]
  library_sources`. Argv-injection refused, symlink walks refused,
  `file://` scoped to the cache root.
- **`jarvy diagnose --apply`** + **`jarvy migrate --apply`** —
  previously "(Fix application not yet implemented)" / "--apply is
  not yet implemented" placeholders now actually do the work.
- **25-item parallel-review hardening** — strict version allowlists,
  filename sanitizer, workspace path-traversal refusal, atomic-write
  for discover --apply, `MergeOutcome` enum, telemetry-gate
  consistency, perf wins (Arc<Manifest> snapshot, cmd_satisfies
  cache), QA + observability rounds.
- **Half-baked surface close-out** — every dead-coded
  `#[allow(dead_code)] // Reserved for ...` either shipped or got an
  explicit deferral note in CHANGELOG.

**Breaking-shape additions (all opt-in, no existing config breaks):**

- `[discover]` block in `jarvy.toml` — optional, drives custom
  detection rules.
- `[git_hooks.native]` block — only consumed when
  `framework = "native"`.
- `[[ai_hooks/mcp_register/skills.library_sources]]` entries now
  accept `manifest_sha256 = "<hex>"`.
- Cargo feature `test-bypass` — gates the
  `JARVY_{LIBRARY,REGISTRY}_ALLOW_INSECURE_FETCH` + `JARVY_TEST_HOME`
  test escape hatches out of release builds. Release builds are
  inert against those env vars.

**New events (CLAUDE.md taxonomy stable contract):**
- `discover.applied`, `discover.setup_advisory`
- `workspace.validate_completed`, `workspace.member_invalid`
- `library.sync.failed`, `library.signature.verified`
- `git_hooks.{install,update}_{started,completed}`

**Test counts:** 974 lib + 1442 bin + 54 integration suites green.

**Behavior changes:**
- `jarvy discover --watch` now exits **CONFIG_ERROR (2)** instead of
  `0` when the filesystem watcher backend dies (inotify exhausted,
  permission revoked, channel closed). CI wrappers and cargo-watch-
  style shells chaining on `$?` will now correctly see the failure
  instead of treating a silent watcher death as success.
- `jarvy setup --project <name>` now stages synthesized member
  configs under `~/.jarvy/cache/synthesized/<sha>.toml` (content-
  addressed, overwritten per run) instead of leaking persistent
  `jarvy-setup-project-*.toml` files into `/tmp`. CI bots that
  previously accumulated these in their tmpfs can clear them.
- `[discover] rules = "<path>"` now refuses absolute paths and `..`
  traversal components. Parse-error advisories also redact source
  bytes from the target file so a hostile `jarvy.toml` cannot
  exfiltrate `/etc/shadow` content via the error message.

**Migration notes:**
- `jarvy migrate --apply` will auto-rewrite legacy `[tools]` blocks
  to `[provisioner]` for users still on the pre-v0.2 schema.
- `setup --project current` is the natural one-liner for monorepo
  contributors who `cd` into their working subdir.
- Workspace members without a per-member `jarvy.toml` will see their
  synthesized config moved from `/tmp/jarvy-setup-project-*.toml` to
  `~/.jarvy/cache/synthesized/`. The contents are equivalent; only
  the storage path changed.

## PRD-056 agent-driven wizard — original 2026-06-30 note (folded into v0.5.0 above)

`jarvy wizard` — hands the current project to your local AI coding
agent (Claude Code, Codex, Cursor, Windsurf, Cline, Continue) to
analyze + configure. Uses *your* subscription (no vendor LLM gateway,
no API keys handled by Jarvy).

**Two modes, auto-picked:**
- **Headless CLI** for Claude Code (`claude -p`) and Codex
  (`codex exec --`) — Jarvy spawns the agent with a system prompt +
  project-context envelope on stdin; the agent calls Jarvy's MCP
  tools inline.
- **Skill drop** for the other four agents — writes
  `~/.{agent}/skills/jarvy-setup/SKILL.md`; user opens their agent
  and types "set up jarvy for this project".

**Greenfield supported.** With no `jarvy.toml`, the wizard prompt
explicitly instructs the agent to call `jarvy_discover_apply` with
`apply=true` to bootstrap a starter file.

**Quickstart fallback.** If no AI agent is installed, the wizard
delegates to the existing `jarvy quickstart` flow — users without an
agent aren't blocked.

**Trust boundaries.** Refuses in sandbox / CI / non-TTY (headless) /
against remote configs by default; `JARVY_WIZARD=1` overrides.

**New MCP tool:** `jarvy_wizard_plan` — read-only proposal endpoint
the agent calls before mutating. No new mutating MCP tools; existing
gates (`MutationCtx` rate limit + TTY confirm + audit log) cover the
change surface.

**New events:** `wizard.started`, `wizard.skill_dropped`,
`wizard.headless_spawned`, `wizard.headless_exit`, `wizard.refused`.
All gated on `telemetry_gate::is_enabled()`.

**Files:**
- New: `src/wizard/{mod,context,prompt,headless,skill_drop}.rs`,
  `src/commands/wizard_cmd.rs`, `assets/wizard-skill/SKILL.md`,
  `prd/056-agent-driven-wizard.md`, `docs/wizard.md`.
- Modified: `src/cli/args.rs` (adds `Wizard` subcommand),
  `src/commands/dispatch.rs` (routing), `src/mcp/extended_tools.rs`
  (`jarvy_wizard_plan` definition + handler),
  `src/mcp/server.rs` (dispatch arm), `mkdocs.yml`, `CLAUDE.md`.

## v0.4.0 detail expansion — original 2026-06-28 note (already summarized in v0.4.0)

A documentation + maintainability + ecosystem-breadth pass that closes
eleven long-open PRDs across five commits. The headliner is **PRD-054
library registry** + **PRD-055 git skill sources** — a shared
HTTPS-fetched manifest format that lets a team publish reusable AI
hooks, MCP servers, and AI agent skills at any URL, with skills
additionally supported via plain Git repos (no `manifest.json`
required). `[ai_hooks]`, `[mcp_register]`, and `[skills]` all consume
the same format. PRD-049 (skills) rides on it; PRD-048 / 052 (git
hooks, spinners) shipped earlier in the day. No user-visible behavior
changes for existing configs — all new surface is additive.

### Added — PRD-044 / 047 phase 2 (everything except interactive mode)

Closes the remaining deferred work on the two PRDs plus several
quality-of-life follow-ups that fell out of the dependency graph.

**PRD-044 — auto-discovery phase 2:**
- `[discover]` config block in `jarvy.toml` carrying `rules = "<path>"`
  for custom rule files + `ignore_dirs`. Custom rules append to (never
  replace) the built-in set so a user tree can't silence detection of
  a real ecosystem.
- `--rules <path>` CLI flag overrides the config-file path for one-off
  runs.
- `FileContaining { file, containing }` detection pattern — bounded
  4 KiB scan, used today by the kubectl rule to catch
  `kind: Deployment` / `apiVersion: apps/v1` in bare `*.yaml` files at
  the repo root.
- Per-language version-range narrowing: when the user has already
  pinned a semver range that COVERS the detected version (e.g.
  `node = "^20"` + `.nvmrc` says `20`), discover treats it as
  already-configured instead of re-suggesting an exact-version pin.
  Falls back to the prior "list as required" behavior when we can't
  parse either side.
- `uninstallable` bucket in `DiscoverReport` for tools jarvy detected
  but doesn't have a first-party installer for (maven, gradle, dotnet
  …). New CLI section + JSON field — users see what jarvy noticed and
  can't help with, instead of silent drops.
- `--watch` mode — `notify` filesystem watcher re-runs discover on
  every relevant event with a 750ms debounce so editor saves /
  bulk rewrites only re-emit once.
- Continuous discovery on `jarvy setup` — emits a stderr advisory
  + `discover.setup_advisory` event when project files imply tools
  not pinned in `[provisioner]`. Read-only; never mutates jarvy.toml.

**PRD-047 — monorepo phase 2:**
- `[workspace] members = ["apps/*"]` glob expansion. `*`-only
  patterns supported; subdirectory members under the parent are
  enumerated and skipped if they start with `.`.
- `[workspace] exclude = [...]` glob patterns applied AFTER
  expansion. Pairs with globs to handle "everything except
  apps/legacy."
- `jarvy setup --project <name>` runs setup against ONE workspace
  member. `name = "current"` auto-detects from cwd. Members without a
  per-member jarvy.toml get a synthesized merged config written to a
  tempfile so the existing single-project setup loop doesn't need to
  know about workspaces.
- Auto-context detection: when `jarvy setup` is invoked WITHOUT
  `--project` AND cwd sits inside a declared member, setup scopes to
  that member implicitly + prints an advisory line.
- `jarvy context` standalone command — read-only diagnostic showing
  cwd, detected workspace, members (current marked with `→`),
  auto-detected project, and resolved setup file. Supports
  `--format json`.
- `jarvy drift` and `jarvy doctor` honor the same auto-context. cwd
  inside a member → drift reads that member's merged config and
  `.jarvy/state.json` lives next to the member; doctor checks the
  merged inherited toolset. No new flags — the behavior emerges from
  the shared `setup_cmd::auto_detect_project` helper.

**Skipped on purpose:** `--interactive` confirm-each-suggestion mode
for discover — user signal said it wasn't worth the friction; the
existing dry-run-then-`--apply` two-step covers the same need.

**Verification:** cargo fmt + clippy --all-features --all-targets -D
warnings clean. 974 lib + 1442 bin tests green (+11 lib, +16 bin vs
the prior baseline). New tests cover version-narrowing, uninstallable
bucket, glob expansion, exclude patterns, glob_matches table,
workspace-project resolution (4 cases including current / synthesize
/ unknown / no-workspace), and context-cmd in a non-workspace.

### Added — close-out of every half-baked surface flagged in the audit

A single bundled commit retires the partial-feature list surfaced by
the codebase audit. Net: ~3,100 LOC added, ~210 removed, 7 commits
worth of work folded into one.

**PRD-054 phase 6 — `jarvy library` cache CLI (`src/commands/library_cmd.rs`).**
New top-level command with four subcommands:
- `library list` — every cached library (URL, publisher, item counts)
- `library show <url>` — items inside one cached library
- `library clean [--dry-run]` — wipe `~/.jarvy/library.d/` and the
  process cache (atomic via the existing `clear_disk_cache` helper)
- `library sync --file <jarvy.toml>` — force-refresh every declared
  `[<subsystem>.library_sources]` entry, honoring the
  remote-config trust gate

All four support `--format json`. New `library_registry::list_cached`,
`get_cached`, and `clear_disk_cache` helpers (the dead-coded
internals from earlier phases are now wired). 4 unit tests.

**Husky framework handler (`src/git_hooks/husky.rs`).** PRD-048's
declared-but-unwired Husky variant now ships a full handler:
`npm install --save-dev husky` + `npx husky install` for `install`,
re-run for `update`, walks `.husky/<name>` for `run` and `list`. 5
unit tests covering the missing-package.json / missing-`.husky/` /
unknown-hook-id error paths.

**Lefthook framework handler (`src/git_hooks/lefthook.rs`).** Single
Go-binary alternative to pre-commit / husky. Permissive YAML parse
(top-level `BTreeMap<String, Value>`) so non-stage keys like
`skip_output:` don't break the lister. `lefthook self-update` is
treated as advisory — non-zero exit doesn't fail the update path
(brew users update via `brew upgrade`). 5 unit tests.

**Native git hooks handler (`src/git_hooks/native.rs`).** No framework
process between git and your script: hook bodies live in
`[git_hooks.native.hooks.<stage>]` and get written straight into
`.git/hooks/<stage>` with a `# managed by jarvy` marker. Refuses to
overwrite hand-rolled hooks (any `.git/hooks/<name>` without the
marker is preserved). Atomic tmp+rename + `chmod +x` so a mid-write
crash leaves the prior hook intact. User-supplied shebangs are
honored; a missing `#!` line gets `#!/bin/sh` injected. 7 unit tests.

**Husky migration docs (`docs/replace-husky.md`).** Three paths laid
out — wrap (zero migration), switch to pre-commit, switch to
lefthook — with a decision tree, sample configs for each, and the
caveats per path. Linked from the git-hooks doc.

**`jarvy diagnose --apply` fixes the "not yet implemented" placeholder.**
Auto-applicable fixes now actually shell out via `sh -c` and surface
the exit status. Manual-fix items still print the suggestion — they
need human review. Trust posture: `Fix::command` comes from the
tool's own `ToolSpec` (first-party data, not remote-fetched), same
trust we already extend to the printed-for-copy variant.

**`jarvy migrate --apply` rewrites `[tools]` → `[provisioner]`.**
Atomic tmp+rename + post-write TOML round-trip check (refuses to
write garbage). Unknown-tool advisories still report-only — human
decision required. 4 unit tests pinning the rewrite, no-op, and
report-only paths.

**`jarvy_discover_*` / `jarvy_workspace_*` / `jarvy_library_*` MCP
tools.** Seven new first-class MCP tools so AI agents don't have to
shell out to the CLI for these surfaces:
`jarvy_discover_scan`, `jarvy_discover_apply` (mutating — runs
through the standard rate-limit + audit + confirmation gate),
`jarvy_workspace_list`, `jarvy_workspace_show`, `jarvy_workspace_validate`,
`jarvy_library_list`, `jarvy_library_show`.

**PRD-054 phase 5 — cosign signature enforcement
(`src/library_registry/signature.rs`).** `require_signature = true`
now actually verifies. We fetch `<url>.sig` + `<url>.pem`, feed
them to the existing `crate::update::signature::verify_sigstore_signature_with_identity`,
and refuse on failure with structured errors:
`SignatureRejected`, `SignatureCompanionsMissing`, or
`CosignMissing`. The previous "scaffolded but not enforced" warning
is gone — replaced by a per-fetch `library.signature.verified`
success event so operators can graph the verified fraction.
Cached hits don't re-verify (the cache only ever holds previously-
verified manifests). `require_signature = false` short-circuits as
documented — `library.signature_disabled` warning still fires.

CLAUDE.md event taxonomy table updated for the new
`library.signature.verified` event.

### Added — parallel-review hardening for PRD-044 / 047 / 051 (25 review items)

Follow-up sweep against the parallel-code-review enhancement plan that
ran against the three preceding PRDs. All 25 items addressed across
7 batched commits. Highlights:

- **Security (P0 #1, #2, #3; P2 #17, #18, #20, #21):** Strict allowlist
  on extracted versions (`[A-Za-z0-9._+~:\-]`, ≤64 chars, BOM stripped)
  closes TOML injection via `.python-version` / `rust-toolchain.toml` /
  `.ruby-version`. Strict allowlist on `*.tf`-glob filename source
  attribution closes the same threat via attacker-controllable
  filenames. `jarvy workspace` refuses traversal members
  (`members = ["../../etc"]`) — existence-oracle leak and external
  jarvy.toml reads no longer possible. `jarvy discover --apply` now
  uses atomic tmp+rename writes (no torn files on crash). Ticket-show
  TTY path runs zip entry names through the existing ANSI sanitizer.
- **UX / data safety (P0 #4; P1 #8, #14):** Empty `[workspace] inherit
  = []` is treated as `["provisioner"]` by BOTH the CLI display and
  the production setup resolver via a new
  `WorkspaceConfig::effective_inherit()` helper — the previous
  CLI-only widening was a "show advertises what setup won't do" bug.
  `jarvy discover --apply` refuses to overwrite an existing jarvy.toml
  that fails to parse as TOML (data-loss prevention). The
  `[provisioner]` section detector in the merger now uses a TOML
  parse instead of substring match, so a comment containing the
  literal `[provisioner]` no longer confuses the splicer.
- **Robustness (P1 #5):** The string-level TOML edit in
  `discover/generator.rs::merge_into_existing` now re-parses its
  output and falls back to a fresh `render_fresh` on any edge case
  (`[provisioner.linux]` subtables, indented headers, `[[provisioner]]`
  arrays-of-tables) — guarantee: NEVER write invalid TOML. Return
  type widened to `MergeOutcome { Merged, Noop, BailedToFresh,
  ExistingUnparseable }` so the CLI can produce a clear diagnostic.
- **Observability (P1 #6, #7, #15):** New events
  `workspace.validate_completed` (`info!`/`warn!` by error count) +
  `workspace.member_invalid` (`error_kind` only — no member-name
  leak), `discover.applied` (tools added, recommended, already
  configured, target = "merged" | "noop" | "bailed_to_fresh",
  duration_ms). All gated through `telemetry_gate::is_enabled()`.
  CLAUDE.md taxonomy table updated. New CLI dispatch test pins the
  stderr-pure contract when `--format json` + `JARVY_TELEMETRY=0`.
- **Performance (P1 #9, #10, #11; P2 #22, #23, #24, #25):** Workspace
  list/show parses root jarvy.toml once instead of per-member. Key
  membership now uses `HashSet<&str>` borrowed from the parse instead
  of cloning every value into a `BTreeMap`. drift_cmd's version
  regex cached in `OnceLock`. discover::default_rules() cached in
  `OnceLock` (no per-call rebuild of 12-rule vec). recommended_seen
  checks before clone. detections moved into report instead of
  cloned. Generator splicer uses `String::with_capacity` +
  `push_str` instead of `Vec<String>`.
- **QA (P1 #12, #13; P2 #19):** 13 new tests covering — extension-glob
  determinism, hostile filename refusal, version sanitizer, BOM
  strip, length cap, merge `[provisioner]`-in-comment edge case,
  merge bail-to-fresh on broken splice, merge refuse-when-unparseable,
  atomic-write completeness, traversal-member refusal,
  `effective_inherit()` round-trips, ci-info/ticket/logs/discover/
  workspace JSON-shape contracts, and stdout-pure JSON regression
  guard. Test totals: 945 lib + 1400 bin (up from 907 / 1353).

CHANGELOG note also includes `docs/discover.md` and `docs/workspace.md`
in the doc set that surfaces these PRDs to end users.

### Added — PRD-044 / 047 / 051 (auto-discovery + monorepo + JSON output)

Three PRDs closed in a single session, all additive (no existing
config changes).

**PRD-051: `--format json` on every command.** Subcommands that
previously only emitted human text now accept `--format json` and emit
a structured envelope: `jarvy ci-info`, `jarvy drift {status,accept,
fix}`, `jarvy logs {stats,clean,config}`, `jarvy ticket {create,show,
list,clean}`, `jarvy services {start,stop,status,restart}`. CLI exit
codes are identical between human and JSON paths so `$?` based
control flow keeps working. See `docs/cli-reference.md` and the new
"Structured output" section in `CLAUDE.md`.

**PRD-044: `jarvy discover` project tool auto-discovery.** New
top-level command that scans the project root for marker files
(Cargo.toml, package.json, go.mod, Dockerfile, k8s/, *.tf, Makefile,
Justfile, …), infers versions from `rust-toolchain.toml` / `.nvmrc`
/ `.python-version` / `go.mod`, and either prints suggestions or
merges them into `jarvy.toml` (`--apply`). The merge is append-only:
hand-pinned tools survive unchanged. New module `src/discover/` with
built-in `default_rules()` covering rust, node, python, go, ruby,
docker, kubectl, helm, terraform, pre-commit, make, just. Custom rule
files deferred. See `docs/discover.md`.

**PRD-047: `jarvy workspace` monorepo inspection.** New CLI surface
over the existing workspace foundation (`crate::workspace::
find_workspace_root` + `merge_configs`). Three read-only subcommands
— `list`, `show <member>`, `validate` — that resolve per-member
configs via inheritance and surface the result with `(inherited)` /
`(overridden)` provenance. Empty `[workspace] inherit = []` is
treated as `["provisioner"]` for display so the common case works
without explicit config. Workspace-aware `jarvy setup --project <name>`
orchestration deferred. See `docs/workspace.md`.

### Added — parallel-review enhancement plan (25 P0/P1 items + sweep)

A multi-batch sweep against the parallel-code-review enhancement plan
shipped as 12 commits (`6155056..HEAD`). Highlights:

- **Security** (items 1, 2, 3, 12, 13, 14, 15): argv-injection refusal on
  git refs, symlink-escape refusal on the clone walker, `file://` scoping
  to the library cache root, `manifest_sha256` pin on library_sources
  (refuses re-published manifests), loud `library.signature_unenforced`
  warning when `require_signature = true` (cosign not yet enforced in v1),
  `GitHooksConfig::default()` matches serde defaults instead of silently
  disabling hooks, and a new `test-bypass` Cargo feature that compiles
  `JARVY_{LIBRARY,REGISTRY}_ALLOW_INSECURE_FETCH` + `JARVY_TEST_HOME`
  out of release builds (env vars are inert in shipped binaries).
- **Trust gates** (items 4, 5): `[skills]` and `[git_hooks]` now propagate
  `ConfigOrigin::Remote` from `Config::mark_remote` and enforce the
  `allow_remote` opt-in on both subsystems.
- **Observability** (items 6, 7, 8, 22, 23, 24): every
  `library.*` / `library.git.*` / `skills.*` / `git_hooks.*` /
  `package.*` event reads `telemetry_gate::is_enabled()` so opt-out
  users don't ship breadcrumbs; `library.sync.failed` emit on every error
  path (was silent); `library.sync` tracing span wraps each per-source
  fetch; `git_hooks.{install,update}_{started,completed}` envelopes
  carry `status`, `applied`, `framework`, `auto_update`,
  `run_after_install`, `duration_ms` — same shape as
  `ai_hook.phase_*` / `mcp_register.phase_*`.
- **Maintainability** (items 16, 17, 18, 19): `library_registry::sync_all`
  consolidates the three identical `prepare_library_sources` copies;
  `packages::common::run_install_loop` consolidates the gem/go/cargo/nuget
  install + telemetry loops behind a closure-based helper;
  `net::bounded_fetch` collapses the two copies of HTTPS-only refusal +
  bounded read + loopback-bypass parser (with per-consumer env-var
  names preserved for test isolation); `agents::Agent` is the canonical
  enum shared by `ai_hooks`, `mcp_register`, and `skills` (the three
  former per-subsystem enums are now `pub use` aliases). Net 450 LOC
  removed.
- **Performance** (items 20, 21): library_registry caches `Arc<Manifest>`
  so resolvers snapshot the cache and drop the mutex before walking
  items; `cmd_satisfies` caches `<cmd> --version` stdout so per-tool
  version probes don't refork the package manager. `detect_linux_pm`
  / `detect_bsd_pm` dropped their local `has` closures that bypassed
  the cached `has()`.
- **QA** (items 9, 10, 11, 25): coverage tests for ai_hooks
  `library_sources` resolution, `mcp_register::use_library` overrides,
  `skills` sha-mismatch refusal, and `hooks_cmd` action exit-code
  contract. Plus `#[serial(jarvy_telemetry_disclosure)]` on the
  telemetry disclosure tests to prevent parallel-test flakes.

User-visible config additions: `[[<subsystem>.library_sources]]` accepts
an optional `manifest_sha256 = "<hex>"` pin. CLI exit codes and
existing event names are unchanged.

### Added — git-shorthand for skill sources (PRD-055)

- **`git+https://...@<ref>[#<subpath>]` URL scheme** on `[skills]
  library_sources`. Jarvy clones the repo at the pinned ref, walks
  the optional subpath for `SKILL.md` files, parses each file's YAML
  frontmatter, and synthesizes a manifest in-memory. Publishers don't
  need to maintain `manifest.json` — the SKILL.md files are
  self-describing.

  ```toml
  [[skills.library_sources]]
  url = "git+https://github.com/myorg/jarvy-skills.git@v1.2.0#skills/"
  ```

- **`github:owner/repo@<ref>` shorthand** for the common GitHub case:

  ```toml
  [[skills.library_sources]]
  url = "github:anthropics/skills@v1.0.0"
  ```

- **`@<ref>` pin is mandatory**. Unpinned URLs refused at parse time
  with a clear message — silent floating refs would let a publisher
  rev skills without a visible pin bump.

- **Trust hierarchy**: commit SHA (tamper-evident) > tag (mutable but
  conventional) > branch (freely mutable, emits
  `library.git.mutable_ref` warning every fetch). Documented in
  `docs/library-registry.md`.

- **SKILL.md frontmatter convention**: `name:` + `version:` required,
  `description:` + `supported_agents:` optional. Files missing
  required fields are skipped with a `library.git_skill.skipped`
  event citing the reason. No silent failures.

- **Subpath traversal refused** at parse time + at fetch time:
  `..` segments and absolute paths are rejected with a canonical-path
  check inside the clone root. Mirrors
  `safety::resolve_within_workspace` from `src/mcp/extended_tools.rs`.

- **No new dependencies**. Shells out to `git`; missing git refuses
  with a clear error pointing at `[provisioner] git = "latest"`.
  Cached `--depth 1` clone refreshes via `git fetch + git checkout
  <ref>`.

- **Why skills only**: SKILL.md carries its own frontmatter, so
  Jarvy has everything needed to build a manifest entry from one
  file. AI hooks (script bodies) and MCP servers (command/args/env
  tables) don't — those still ship via `manifest.json`. A publisher
  who wants both in one Git repo puts `manifest.json` at the root and
  uses the existing URL form.

- New modules: `src/library_registry/url_parser.rs` (scheme +
  `@<ref>` + `#<subpath>` parsing with safety refusals),
  `src/library_registry/git_fetch.rs` (clone + frontmatter walker
  + manifest synthesizer). `sync()` in `mod.rs` dispatches by
  scheme. `read_file_url()` extends the installer's fetch path to
  handle `file://` URLs that point into the git cache.

- Trust gate inherits unchanged from PRD-054 — remote-fetched configs
  CANNOT declare `library_sources` of any scheme.

### Added — library registry (PRD-054)

- **`src/library_registry/` shared module**: manifest schema (tagged
  by `kind`: `ai_hook` / `mcp_server` / `skill`), HTTPS-bounded fetch
  (`MAX_MANIFEST_BYTES = 16 MiB`, `MAX_ITEM_BYTES = 1 MiB`), on-disk
  cache at `~/.jarvy/library.d/<sha256-of-url>/manifest.json`,
  in-process resolver across all cached libraries. Atomic write
  pattern (`.new` → rename) for cache durability.

- **One manifest, three consumers**: a single `manifest.json` URL can
  publish AI hooks, MCP servers, and skills simultaneously — each
  consumer filters by `kind`. Publishers write one manifest; teams
  point `[ai_hooks] library_sources`, `[mcp_register] library_sources`,
  and `[skills] library_sources` at the same URL.

  ```toml
  [[ai_hooks.library_sources]]
  url = "https://cdn.myorg.com/jarvy/manifest.json"

  [[ai_hooks.hook]]
  use = "no-prod-deploys"
  ```

- **Trust model uniform across consumers**: remote-fetched configs
  (`jarvy setup --from <url>`) CANNOT declare `library_sources` —
  refused with `library.remote_refused` event. Mirrors
  `[packages] allow_remote` semantics. There is no override flag;
  adding one would defeat the purpose. Teams that want to ship
  `library_sources` to every developer copy them into each user's
  local `~/.jarvy/config.toml`.

- **Built-in items win over library items**: `crate::ai_hooks::LIBRARY`
  (the canonical Jarvy-shipped hooks) is checked BEFORE library
  fallbacks, so name collisions favor the audited built-in.

- **sha256 verification** for skill `SKILL.md` bodies (mandatory) and
  scaffolded for ai_hook `bash_url` (v1 only honors inline `bash:` for
  hooks). A publisher mutating a versioned artifact in place surfaces
  a clear `library.sha_mismatch` event and refuses to install.

- **Offline tolerance**: on network failure, the cached on-disk
  manifest is served with a `library.fetch.cached_hit
  reason="fetch_failed"` event so log scrapers can see staleness.

- **Cosign signature verification scaffolded but not enforced in v1**.
  `require_signature = true` (default) is honored once cosign wiring
  lands; `false` today emits a `library.signature_disabled` warning.
  Phase 5 of PRD-054.

### Added — AI agent skills installation (PRD-049 v1)

- **`[skills]` config block** with `library_sources`, `install` map,
  per-skill `agents = [...]` narrowing.
- **`jarvy skills` subcommand**: `install` (all or `--name <skill>`),
  `list` (per-agent status), `status` (drift summary), `agents`
  (detect installed AI agents).
- **Setup integration**: `jarvy setup` auto-installs every configured
  skill when `[skills] auto_install = true` (default).
- **Per-agent path layout**: `~/.{agent}/skills/<skill-name>/SKILL.md`
  across claude-code, cursor, codex, windsurf, cline, continue. Two
  narrowing layers (consumer `agents = [...]` + publisher
  `supported_agents = [...]`) both apply.
- **`.jarvy-skill.json` sidecar** records version + sha256 + install
  time per skill per agent. `jarvy skills status` uses it for drift
  detection without needing to re-fetch.
- **v1 explicitly skips** skills.sh API integration (search / info /
  popular), companion file fetching, `jarvy skills update` /
  `remove`, version-range pinning, project-scope skills. Tracked
  under PRD-049 phase 2.

### Added — library_sources consumers for AI hooks + MCP register

- **`[ai_hooks].library_sources`**: fetch + register library hook
  items. `use = "hook-name"` resolves built-in `LIBRARY` first, then
  cached library items. Hook bodies are taken inline from manifest
  `bash:` / `powershell:` fields. Per-source-failure-is-advisory:
  `apply` continues with cached + built-in hooks if a library URL is
  unreachable.

- **`[[mcp_register.server]] use = "library-name"`**: pulls
  `command` / `args` / `env` defaults from a previously synced
  library item. Locally-declared fields on the spec override the
  library defaults (e.g. spec `env = { ... }` wins over library env).
  Subject to the existing `allow_custom_servers` gate plus the new
  `library_sources` remote-refusal gate.

### Tracking

- Drafts + closes PRD-054 (Library Registry — v1 shipped, sig verify
  + `jarvy library` CLI tracked as follow-up)
- Drafts + closes PRD-055 (Git skill sources — full v1 shipped;
  `git+ssh://` and sparse-checkout tracked as follow-up)
- Closes PRD-049 phase 1 (Skills Registry Integration — library-based
  install ships; skills.sh API + remove/update commands tracked as
  PRD-049 phase 2)
- Continues PRD-048 (Pre-Commit Hook Installation) + PRD-052
  (Progress Indicators) from the prior commit
- Continues PRD-011 / 013 / 014 / 037 / 038 / 039 closures from the
  first commit

---

## [Unreleased — earlier: pre-commit hooks + progress spinners]

(Originally a separate `[Unreleased]` entry; merged into the section
above so the awk extractor sees a single curated block.)

A documentation + maintainability + ecosystem-breadth pass that closes
eight long-open PRDs across three commits: gem/go package handlers +
main.rs extraction + documentation gaps in the first, pre-commit hook
framework + spinner output in the second. No user-visible behavior
changes for existing configs — all new surface is additive (`[gem]` /
`[go]` / `[git_hooks]` sections, `jarvy hooks` subcommand,
opt-out-friendly progress spinners).

### Added — pre-commit hook framework integration (PRD-048)

- **`[git_hooks]` section** auto-installs and manages git pre-commit
  hooks from `jarvy.toml`. Today the `pre-commit` framework
  (<https://pre-commit.com>) is fully supported; `husky` and `lefthook`
  are recognized by auto-detection but their handlers are stubbed with
  a clear "framework configured but not yet supported" error so configs
  can declare intent without silent no-ops.

  ```toml
  [git_hooks]
  # block presence is the opt-in; auto-detects from .pre-commit-config.yaml

  [git_hooks.pre_commit]
  version = "3.6.0"                # pin the framework version
  install_hooks = true             # warm hook envs eagerly
  ```

- **`jarvy hooks` subcommand**: `install`, `update`, `status`, `list`,
  `run` (with `--all-files` / `--hook <id>`), `uninstall`. Status output
  parses `.pre-commit-config.yaml` directly, so hook counts work even
  when the `pre-commit` CLI itself isn't installed yet.

- **Setup integration**: `jarvy setup` auto-runs `jarvy hooks install`
  between the git-config and ai-hooks phases. Gated on
  `[git_hooks] auto_install = true` (default). New phase emits
  `git_hooks.phase_started` / `_completed` / `_skipped` /
  `_install_failed` telemetry events.

- **Trust boundary**: deliberately a new top-level block, NOT
  `[hooks].git_hooks` — `[hooks]` is already taken by setup-lifecycle
  shell scripts (PRD-003). Remote-config trust gate via
  `[git_hooks] allow_remote = true` (mirrors `[packages] allow_remote`):
  a friendly-looking remote config cannot silently land arbitrary git
  hooks on the consuming machine without explicit opt-in in the SOURCE
  config. Refusals log `git_hooks.remote_refused` for audit.

- New module: `src/git_hooks/{mod.rs, config.rs, detection.rs, precommit.rs}`.
  Husky / lefthook handler stubs return `HookError::UnsupportedFramework`
  with the framework name, so the surface is stable for follow-up work.

### Added — progress spinners (PRD-052)

- **`indicatif` dependency** plus `src/progress.rs` helper providing
  `Progress::start()` → `Spinner` with `finish_ok` / `finish_skipped` /
  `finish_failed`. All long-running commands route through this helper
  rather than constructing `ProgressBar` directly, keeping the muting
  decision in one place.

- **Auto-disable** when any of: stdout is not a TTY, `JARVY_QUIET=1` or
  `--quiet`/`-q` on argv, `--format json` / `--log-format json`,
  sandbox / CI detected by `sandbox::is_seamless_auto()`, or
  `JARVY_NO_PROGRESS=1` (explicit kill switch). In sandbox / CI mode,
  spinners fall through to plain `println!` lines so log scrapers still
  see start / finish events.

- **Wired into** `jarvy update check` (network spinner) and
  `jarvy hooks install` / `update`. Deeper integration in `setup_cmd`'s
  parallel-install loop is deferred — needs design to avoid clashing
  with subprocess streaming stdout.

- Uses stdlib `std::io::IsTerminal` rather than dragging in a direct
  `libc` dep.

### Changed

- `src/main.rs` + `src/lib.rs`: register `progress` and `git_hooks`
  modules. `CLAUDE.md` module map updated.
- `src/config.rs`: new `git_hooks: Option<GitHooksConfig>` field;
  `TOP_LEVEL_SECTIONS` extended; `top_level_sections_matches_config_fields`
  destructure test updated.
- `src/commands/dispatch.rs`: route `Commands::Hooks { action, file }`
  to `commands::hooks_cmd::run_hooks`.

### Docs

- `docs/git-hooks.md` covers configuration, commands, status output,
  trust boundary, CI considerations, troubleshooting.
- `mkdocs.yml` adds "Git hooks (pre-commit)" under Guides.
- `tasks/prd-048-pre-commit-hook-installation.json` and
  `tasks/prd-052-progress-indicators.json` created with completion
  notes and explicit follow-up scope.

### Tracking

- Closes PRD-048 (Pre-Commit Hook Installation — pre-commit framework
  only; husky / lefthook tracked as follow-up)
- Closes PRD-052 (Progress Indicators — helper module + `update check`
  + `hooks install/update`; deeper setup_cmd integration tracked as
  follow-up)

---

### Earlier in the day — gem/go handlers, main.rs extraction, documentation gaps

(Originally committed separately; merged into this unreleased block so
the awk extractor sees a single curated `[Unreleased]` section.)

### Added — language package ecosystems (PRD-039)

- **`[gem]` section** installs Ruby gems via `gem install --no-document
  <name> [-v <version>]` against the active ruby. `--no-document` is
  unconditional — provisioning runs don't need RDoc/RI, and skipping the
  build cuts install time from ~30s to ~3s on chatty gems like
  `rubocop`.

  ```toml
  [gem]
  bundler = "latest"
  rubocop = "1.60.0"
  ```

- **`[go]` section** installs Go binaries via `go install <module>@<version>`
  to the user's `GOBIN`. Module paths are full import paths (require
  quoting in TOML); version is mandatory outside a `go.mod` tree, use
  `"latest"` for floating installs.

  ```toml
  [go]
  "github.com/golangci/golangci-lint/cmd/golangci-lint" = "latest"
  "github.com/cosmtrek/air" = "v1.49.0"
  ```

- Both handlers wired into `PackagesConfigRef`, `install_packages`
  dispatcher, `Config` struct, `TOP_LEVEL_SECTIONS`,
  `validate_package_section`, and `run_packages_phase` telemetry.
  `GEM_KNOBS` / `GO_KNOBS` slices pinned by destructure tests so adding
  a future config knob without updating the slice fails compilation
  instead of silently making the validator reject the new knob as a
  hostile package name.

- `packages.phase_started` / `packages.phase_completed` events now
  carry `gem` and `go` booleans alongside the existing
  `npm`/`pip`/`cargo`/`nuget` flags. `packages.phase_previewed` carries
  matching `gem_count` / `go_count` for dry-run preview observability.

- Per-package name + version validation (control bytes, leading-`-`,
  URL schemes) inherits unchanged from `packages/common.rs`. The
  trust-gate refusal of remote-config installs without
  `[packages] allow_remote = true` now also covers `[gem]` / `[go]`.

### Changed — `src/main.rs` extraction (PRD-037)

- `src/main.rs` reduced 734 → 271 LOC (-63%). The original
  1500-line `main()` match block is fully eliminated.

- All CLI dispatch + 14 `handle_*` glue helpers moved to a new
  `src/commands/dispatch.rs` (486 LOC). `main` now retains only
  process init that genuinely belongs at the entry point: telemetry
  config merge precedence (`env > project > global`), sandbox banner
  muting, panic hook, OTLP flush at exit, and the
  `extract_config_path` helper for early telemetry config loading.

- Per-command modules already lived at `src/commands/*_cmd.rs` from
  earlier PRD-037 phases; this round finishes the extraction by
  taking the routing table out of `main` too.

- Zero behavior change: same exit codes, same output, same flag
  forwarding, same OTLP flush sequence. `cargo fmt`, `clippy
  --all-features -- -D warnings`, 814 lib tests, and the full
  integration test matrix are all green on the refactored layout.

### Added — documentation (PRD-011)

Closes the six remaining `docs/` gaps from the PRD-011 audit. All new
pages match the existing flat layout (no new subdirectories) and the
Material for MkDocs style (tabbed code blocks, admonitions, fenced
code with `title=`):

- `docs/installation.md` — full install guide for macOS, Linux,
  Windows, and from-source. Covers winget / scoop / choco / brew /
  cargo, verify steps, update channels, and clean-uninstall.
- `docs/services.md` — operational guide for `[services]`: Docker
  Compose, Tilt, inline service blocks, auto-start during `jarvy
  setup`, CI auto-disable, and `--wait-healthy` patterns.
- `docs/environment.md` — `[env]` guide: plain variables, tool-scoped
  overrides, secret resolvers (prompt / `from_env` / 1Password /
  Vault / AWS Secrets Manager), `.env` vs shell rc, trust boundaries.
- `docs/tools-by-category.md` — 235+ tools grouped by purpose so
  users browsing for "what's available" can scan instead of
  `jarvy search`-ing blindly.
- `docs/contributing-testing.md` — contributor testing guide: when to
  reach for unit / integration / E2E layers, `assert_cmd` patterns,
  `insta` snapshots, `JARVY_TEST_MODE` / `JARVY_FAST_TEST` /
  `JARVY_E2E` flags, common pitfalls.
- `docs/decisions.md` — architecture decisions index. Pointers to
  the canonical sources (`prd/*.md` + `CLAUDE.md`) with one-line
  summaries for the highest-leverage trust, architecture, and
  convention decisions.

- `docs/packages.md` updated with `[nuget]`, `[gem]`, `[go]`
  sections matching the existing `[npm]` / `[pip]` / `[cargo]`
  format; updated module-source line; expanded order-of-operations
  to list all six ecosystems.

- `mkdocs.yml` nav extended: Installation under Get Started;
  Environment variables + Services under Guides; Tools by category +
  Architecture decisions under Reference; Contributor testing guide
  under Community.

### Changed — PRD task tracker hygiene

- Updated `tasks/prd-*.json` for nine PRDs whose JSON status had
  drifted from on-disk reality: 002 (tool post-install hooks), 011
  (documentation), 013 (235 tool dirs vs the 150 target), 014
  (real-world testing — examples + smoke tests + e2e workflow ship),
  021 (MCP server — `src/mcp/` ships), 027 (observability —
  `src/observability/` ships), 037 (main.rs refactor), 038 (E2E
  harness — Phase 1 GitHub-hosted ships; Phase 2 AWS EC2 deferred),
  039 (language packages — gem/go added in this release).

- Each updated JSON carries a `completionNote` field with verification
  evidence: the on-disk files, the LOC delta, or the directory count
  that demonstrates the work is actually shipped (not just intended).

### Tracking

- Closes PRD-011 (Comprehensive Documentation System)
- Closes PRD-013 (Expand Tool Coverage)
- Closes PRD-014 (Real-World Testing and Example Configurations)
- Closes PRD-037 (Main.rs Code Maintainability Refactor)
- Closes PRD-038 (Hybrid Cross-Platform E2E Testing Harness — Phase 1)
- Closes PRD-039 (Language Package Dependencies)
- Stale-status sync for PRD-002, PRD-021, PRD-027 (code shipped earlier)

## [v0.3.0] — Repo relocation to Cliftonz + MCP auto-register default-on (2026-06-26)

First release under the new canonical home, `github.com/Cliftonz/Jarvy`.
The repository was transferred from the `bearbinary` org; the old URL
continues to auto-redirect for git and HTTP traffic, but signing and
package metadata now point at the new owner. Existing users on v0.2.x
do not need to re-clone — `git pull` will follow the redirect — but the
cosign cert-identity baked into v0.3.0 is anchored to `Cliftonz/jarvy`
and will reject artifacts signed under the old subject. There is no
backwards-compatible overlap window; this is a clean cut.

Bundled with the move: `[mcp_register]` now opts in by default when
`jarvy setup` runs against a project with no explicit block, the
`scripts/bootstrap.sh` one-command onboarding is now the canonical
entry point for contributors, and two CI bugs that surfaced during the
v0.2.2 publish (missing Linux tarballs for AUR/chocolatey, parent-vs-
templates version gating) are fixed.

### Changed — repository home

- **Repository relocated to `github.com/Cliftonz/Jarvy`.** All in-tree
  references — Cargo.toml `repository`, install scripts, package
  manifests (Homebrew, AUR, RPM, Debian, winget, chocolatey, Helm),
  documentation URLs, CODEOWNERS, FUNDING — rewritten in a single
  sweep. Cosign cert-identity regex anchored to the new owner; releases
  signed under `bearbinary/jarvy` will no longer verify. GitHub Pages
  (`jarvy.dev`), crates.io ownership (`jarvy`, `jarvy-templates`), and
  Actions secrets carried over via the transfer. CODEOWNERS team
  syntax (`@bearbinary/maintainers`) collapsed to `@Cliftonz` since the
  new home is a user account, not an org.

### Added

- **`[mcp_register]` default-on auto-register.** `jarvy setup` now
  synthesizes a default `[mcp_register]` block when the project has no
  explicit block and at least one supported AI agent (Claude Code,
  Cursor, Codex, Windsurf, Cline, Continue) is detected on disk. The
  built-in `jarvy` MCP server is registered against each detected agent
  with project scope (this repo only), not user scope. Fires the
  `mcp_register.auto_detected` telemetry event with `count`, `agents`,
  `platform`. Suppressed in dry-run, test mode, seamless / CI sandboxes,
  and when `JARVY_MCP_REGISTER=0`. Explicit blocks always win.

### Fixed

- **AUR and chocolatey downstream publish unblocked.** The v0.2.2
  release workflow built macOS+Windows artifacts but skipped the Linux
  tarballs that AUR `PKGBUILD-bin` and chocolatey's MSI bundler expect
  (CPMR0041). The release matrix now produces
  `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu` tarballs
  alongside the existing platforms, so the downstream publish step
  finds its inputs.
- **`jarvy-templates` publish gate now reads the templates crate's own
  version.** Before this fix, the publish workflow keyed off the parent
  `jarvy` crate's version, which meant a parent-only release would
  attempt to re-publish a `jarvy-templates` version that already
  existed on crates.io (and fail). The workflow now reads
  `crates/jarvy-templates/Cargo.toml::version` and only publishes when
  that specific version is new.

### Docs

- **`scripts/bootstrap.sh` is now the canonical one-command onboarding
  path.** End-user repos integrating Jarvy should copy it into their
  own `scripts/` so contributors run `./scripts/bootstrap.sh` to
  install Jarvy (via `dist/scripts/install.sh`) and execute `jarvy
  setup` against the repo-root `jarvy.toml`. Idempotent. Flags:
  `--no-setup`, `--channel <stable|beta|nightly>`, passthrough args to
  `jarvy setup`. Quickstart and contributor docs updated to surface
  this over hand-rolled curl-pipe + `cargo install` snippets.

## [v0.2.2] — Opt-out telemetry default + P0 seamless-gate fix (2026-06-25)

Patch release on the v0.2.x line, but a behavior-significant one: the
telemetry default flipped from opt-in to opt-out, and a P0 security
regression in the CI / sandbox auto-disable was caught and fixed
before any stable shipped with the flip. The two changes are bundled
because they were authored back-to-back in the same evening — the
opt-out flip introduced the regression, and the follow-up commit
closed it along with 15 review findings from a five-persona parallel
code review (security / Rust perf / QA / observability /
maintainability).

Users on a pre-`[telemetry]`-block legacy config also now see the
disclosure on first post-upgrade run, closing the silent-enrollment
loop the security reviewer found. Privacy-disclosure surfaces (`PRIVACY.md`,
`UPGRADING.md`, `data/faq.json`) were swept to match the new posture.

### Changed — privacy posture

- **Telemetry default flipped from opt-in to opt-out.** New installs and
  existing installs whose `~/.jarvy/config.toml` has no explicit
  `[telemetry] enabled = …` line now ship anonymized usage data to
  `https://telemetry.jarvy.dev` by default. The first-run boxed notice
  declares telemetry enabled and surfaces the disable path; the
  end-of-`setup` nudge fires when the user is still on the default and
  points at `jarvy telemetry disable`. Trust boundary unchanged: a
  remote `jarvy.toml` can still only narrow telemetry, never broaden
  it.

  Disable persistently with `jarvy telemetry disable`, per-invocation
  with `JARVY_TELEMETRY=0 jarvy <cmd>`, or via `[telemetry]
  enabled = false`. CI / unattended AI sandboxes still auto-disable —
  that guardrail was hardened (see Fixed below).

  Public docs and disclosure surfaces updated: `CLAUDE.md`, `PRIVACY.md`,
  `UPGRADING.md`, `docs/telemetry.md`,
  `docs/operations/telemetry-forwarder.md`, `docs/ai-hooks.md`,
  `docs/ai-sandboxes.md`, `docs/index.md`, `docs/release-testing.md`,
  `docs/for-ai-agents.md`, `data/faq.json`.

### Added

- **`telemetry.disclosure_shown` event.** Fires after the first-run
  boxed banner (or the legacy-upgrade banner for users whose config
  pre-dates the `[telemetry]` block) renders. Carries `trigger`
  (`first_run` / `legacy_upgrade`) and `platform`. Gives on-call an
  audit trail when users file privacy complaints.
- **`telemetry.undecided_nudge_shown` event.** Fires when the
  end-of-`setup` "Note: opt-out and currently on" line emits. Carries
  `platform`. Lets operators graph what fraction of the fleet is still
  in the undecided state and decide when to retire the nudge.
- **Legacy-upgrade disclosure.** Users with a `~/.jarvy/config.toml`
  that pre-dates the `[telemetry]` block now see the boxed disclosure
  on the next post-upgrade run, after which the block is persisted
  with `enabled = true` so the disclosure doesn't repeat. Closes a
  silent-enrollment loop that would otherwise leave the long tail of
  pre-`d039d9b` configs without ever seeing the banner.

### Fixed

- **`jarvy setup` no longer re-prompts "Do you want to install Oh My
  Zsh?" when `~/.oh-my-zsh` already exists.** The macOS hard-dep check
  asked first and *then* detected the existing install. Detection now
  runs before the prompt; the `tool.already_installed` telemetry event
  still fires (`prompted_user = false`). The decision logic moved into
  a pure `decide_omz_action` function with a table-driven regression
  test — including a `never_prompt` closure that panics if invoked,
  pinning the "AlreadyInstalled short-circuits before any prompt"
  invariant.
- **CI / sandbox telemetry auto-disable now actually fires under the
  opt-out default.** `from_env`'s seamless-detection branch correctly
  computed `enabled = false` when `JARVY_TELEMETRY` was unset, but
  `main.rs` only propagated `env_config.enabled` when the env var was
  *set* — discarding the disable in exactly the case it was supposed to
  fire. Under the prior opt-in default this was masked because the
  disk value was already false. The seamless gate now applies
  unconditionally after the config merge when `JARVY_TELEMETRY` is
  unset. Forced sandbox (`JARVY_SANDBOX=1` without real detection) is
  deliberately NOT in this gate — a hostile dotfile must not silence
  telemetry on the victim's machine.
- **`tool.already_installed` install_path is now home-prefix
  redacted.** Pre-flip the event only fired after a user prompt;
  post-flip it can fire automatically on every `jarvy setup` (the OMZ
  short-circuit). The raw `/Users/<name>/.oh-my-zsh` path is now
  routed through `redact_path` to `~/.oh-my-zsh` before emit. The
  forwarder's server-side scrub remains the defense-in-depth backstop,
  not the contract.
- **`search.executed` no longer emits the raw query string.** The
  user's free-text input previously shipped verbatim — invisible
  under opt-in, but a leak surface once telemetry was on by default.
  Replaced with `had_results` (bool) and `query_len_bucket`
  (`0`/`1-4`/`5-15`/`16-40`/`40+`) so hit-rate dashboards still work
  without storing the query text.
- **`emit_telemetry_hint_if_undecided` uses a section-aware TOML
  parse, not a line-by-line string match.** The prior predicate
  treated `[mcp_register]\nenabled = true` (or any sibling section's
  `enabled` key) as a telemetry decision, suppressing the nudge for
  users who never made one. Extracted as
  `telemetry::user_decided(content)` with five table-driven test
  cases pinning the section-aware behavior.

## [v0.2.1] — Registry pull QA suite + sync.rs supply-chain fixes (2026-06-25)

Patch release on the v0.2.x line. Dominantly defensive: a comprehensive
end-to-end QA suite for the `jarvy registry sync` feature shipped in
v0.2.0, plus the two real bugs that suite caught in the supply-chain
verification path. Also closes Windows test-isolation tech debt that had
been silently red on every tag-push CI run going back to v0.2.0-rc.1.
Soaked as `v0.2.1-rc.1` → `-rc.8` over 2026-06-24 → 2026-06-25; soak
record in [#39](https://github.com/Cliftonz/Jarvy/issues/39).

The two registry-sync bug fixes are the user-impacting items. Operators
running `jarvy registry sync` against a cosign-signed manifest in v0.2.0
were getting fail-CLOSED behavior that looked correct on the surface
(verification rejected) but happened for the wrong reason (the sig/pem
staging paths never matched what `cosign verify-blob` looked for), so
the actual signature was never checked. The second fix closes a window
where a malformed manifest body could be promoted to the canonical
`manifest.json` path before validation rejected it — a subsequent
`jarvy registry status` would then dump the invalid bytes verbatim.
Both shipped silently in v0.2.0 because the original PR only had
in-process tests of `run_sync_with_config`; the new e2e suite drives
the real binary against a programmable mock registry + cosign shim and
is what surfaced them.

### Known limitation — bootstrap-mode gates carry forward

Same status as v0.2.0: [#30](https://github.com/Cliftonz/Jarvy/issues/30)
is still open, so the Path 2/3/4 (upgrade / skip-version / rollback) CI
gate still runs in bootstrap mode. No regression vs v0.2.0; the gap
closes when tarballs ship.

### Added

- **Comprehensive registry-pull QA suite** (~1900 LOC across 4 new test
  files). End-to-end lifecycle (configure → sync → status → clear),
  cosign signature path with a FakeCosign shim, resilience (oversized
  manifest, truncated body, HTTP 500, parallel-fetch stress, recovery
  after prior failed sync, duplicate names, invalid UTF-8, unparseable
  TOML), and tracing-event regression guards that pin
  `registry.sync.{started,completed,sha_mismatch,signature_disabled,failed}`
  by name + level + field shape against the documented OTEL taxonomy.
  Replaces the prior in-process-only coverage that missed the
  staging-path bug.

### Fixed

- **Registry `cosign verify-blob` actually verifies now.** Prior to this
  release, `verify_sigstore_signature_with_identity` looked for
  `manifest.json.unverified.{sig,pem}` as siblings of the staged
  manifest, but the orchestrator wrote them at
  `manifest.json.{sig,pem}.unverified`. Cosign returned
  `SignatureFilesMissing` on every invocation, which `signature_outcome_is_acceptable`
  correctly rejected — so the failure mode was fail-CLOSED ("sync
  refused") rather than silent-bypass, but no signature was ever
  actually checked. Staging now uses the path shape cosign's extension
  derivation expects.
- **Malformed manifest bodies no longer poison the cache.** Previously,
  `sync.rs` wrote `manifest.json.unverified` to disk and then promoted
  to the canonical `manifest.json` BEFORE parsing the bytes. A non-UTF-8
  or syntactically invalid manifest would error out of sync but leave
  the canonical file populated with the bad bytes, which
  `jarvy registry status` then printed verbatim. Manifest is now parsed
  in-memory before any disk write; promotion happens only after a
  successful parse.
- **Windows test-isolation tech debt cleared across the suite.** Eight
  previously-silent Windows-only test failures (`paths::tests`,
  `network::propagate::tests`, `update::installer::tests`, plus 12
  `ai_hooks_integration` + 2 `mcp_register_integration` tests) had
  been red on every tag-push CI run since v0.2.0-rc.1 because (a) test
  helpers hard-coded `/tmp` paths that aren't absolute on Windows,
  (b) `Path::starts_with` is component-aware but string-prefix checks
  with `format!("{prefix}/")` weren't, (c) `dirs::home_dir()` on
  Windows is Win32-API-backed and ignores HOME/USERPROFILE env vars
  (so test sandboxes had no effect), and (d) `cosign` discovery only
  knew about `.exe`, not `.cmd`/`.bat`. All fixed; the Test workflow
  now runs Windows-green on every tag push. v0.2.0 stable shipped with
  these failures as inherited sev-2.
- **Test-mode bypass for `jarvy audit`.** `audit::run_one_scanner` now
  honors `JARVY_FAST_TEST=1` (the documented test-mode contract for
  "skip external command execution") and returns synthetic "not
  available" results. The test for this code path went from 683s to
  1.7s locally.

### Changed

- **Registry CLI + cache events now route through `telemetry_gate::emit`.**
  Closes the opt-in contract for the `registry.*` event family — v0.2.0
  leaked `registry.cli.sync_failed`, `registry.cache.swap_failed`, and
  the `registry.cache.index_*` events to OTLP even when the user had
  set `telemetry.enabled = false`. Matches the contract already
  documented for the `package.*` event family.
- **CI Test workflow on `cargo-nextest`.** Switched from `cargo test`
  to `cargo nextest run --all-features --no-fail-fast`. Process-level
  parallelism per test; the Windows lane went from ~14 min to ~3-4 min
  warm-cache. Also dropped `--show-output` (Windows terminal I/O sink)
  and `--verbose` from `cargo check`.
- **CI actions on Node 24.** Bumped `actions/checkout` v4→v7,
  `actions/upload-artifact` v4→v7.0.1, `actions/deploy-pages` v4→v5,
  `softprops/action-gh-release` v2.2.1→v3.0.0,
  `KSXGitHub/github-actions-deploy-aur` v2.7.2→v4.1.3. Clears the
  Node 20 deprecation warnings the runner had been forcing through.

### Tooling

- **Cursor + JetBrains Toolbox Linux install support** ([#35](https://github.com/Cliftonz/Jarvy/pull/35)).
  Both were macOS+Windows only in v0.2.0; Linux now lands via tarball
  fallback paths.
- **9 networking tools** ([#36](https://github.com/Cliftonz/Jarvy/pull/36)):
  `cloudflared`, `headscale`, `nebula`, `netbird`, `openvpn`,
  `tailscale`, `twingate`, `wireguard-tools`, `zerotier`. Covers VPN +
  overlay-mesh stacks for both home-lab and corp deployments.

## [v0.2.0] — Tooling breadth, MCP surface, AI hooks, release-soak hardening (2026-06-22)

First minor release in the v0.x line. Bigger than its predecessor — 32
commits adding two new tool ecosystems (NATS messaging, .NET / NuGet), a
significant MCP tool surface, AI-hooks distribution to six coding agents,
auto-registration of the Jarvy MCP server, and the release-soak CI gates
that catch regressions before promotion. Soaked as `v0.2.0-rc.1` →
`-rc.2` over 2026-06-16 → 2026-06-22; soak record in
[#25](https://github.com/Cliftonz/Jarvy/issues/25).

### Known limitation — binary self-update gate ships in bootstrap mode

The Path 2/3/4 (upgrade / skip-version / rollback) CI gate is live but
[#30](https://github.com/Cliftonz/Jarvy/issues/30) is open: `release.yml`
does not yet emit `.tar.gz` / `.zip` binary tarballs as release assets, so
the `BinaryInstaller` self-update path has nothing to consume. Users on a
package-manager path (Homebrew, cargo, apt, dnf, pacman, winget,
Chocolatey, scoop, AUR) update normally. Users on the binary fallback see
"No binary for this platform" — same documented gap as v0.1.x. Tracked for
v0.3.0.

### Added

- **NATS messaging toolchain (4 tools).** `nats-server`, `nats` CLI, `nsc`
  (account credentials), plus a `nats-services` built-in template that
  wires a working three-service mesh into a fresh `jarvy.toml`.
- **.NET / NuGet ecosystem.** New `[nuget]` package section + `NugetHandler`
  with end-to-end dry-run + install support. 5 .NET dev tools (full set
  validated against upstream channel docs), 5 .NET-flavored templates, 5
  example configs, and `grpcurl` for grpc service introspection.
- **12 queuing / messaging tools across two batches.** First batch: 6
  workflow + broker tools. Second batch: `pulsar`, `kaf`, `kafkactl`,
  `emqx`, `argo` (Workflows CLI), `kn` (Knative CLI). Tools without
  first-party Windows manifests omit the `winget` block entirely rather
  than ship placeholder IDs that could be hijacked under
  supply-chain attack (see Security).
- **Extended MCP tool surface.** AI hooks, MCP register, drift, roles,
  services, templates, validation — all exposed over MCP. Mutating tools
  (`services_start`, `templates_use`) gated by `gate_mutation` +
  `MutationCtx`: rate limit → stderr TTY confirm → audit log. Workspace
  containment enforced by `safety::resolve_within_workspace` (canonical-
  root check; refuses `..`, absolute escapes, endpoint symlinks).
- **`ai_hooks` distribution to six AI coding agents.** Curated guardrail
  hooks (the "don't `rm -rf` your homedir", "respect .gitignore",
  "stop-on-tests" class of safeguard) provisioned uniformly to Claude
  Code, Cursor, Codex, Windsurf, Cline, and Continue. Bash → PowerShell
  translation on Windows handled in-process so the same hook YAML works
  cross-platform.
- **`mcp_register` auto-registration to the same six agents.** One-shot
  setup that places the Jarvy MCP server entry in each agent's config
  with the correct stdio invocation, so users don't have to copy-paste
  per-agent boilerplate. Trust-gated: only the built-in `jarvy` server
  registers from a remote config unless `allow_custom_servers = true`.
- **Telemetry category plumbing.** `category` field travels through every
  `tool.requested` / `tool.installed` / `tool.failed` event, plus
  `template.materialized`. Operators can graph "what fraction of NATS
  rollouts succeeded?" without pivoting on tool name.
- **`tool.already_installed` event.** Surfaces the skip path with
  `install_path`, `detection_method`, `prompted_user` fields — previously
  invisible in telemetry, now visible.
- **Telemetry `error_kind` discrimination.** `tool.failed` carries an
  `error_kind` enum (`tap_fetch`, `command_failed`, `permission_denied`,
  …) so an operator can split "the brew tap was unreachable" from "the
  binary install actually broke".
- **Drift report category grouping.** Tools group by category in human
  output (`messaging`, `workflow`, `runtime`, …) instead of one flat
  list, making diff review tractable at scale.
- **CI: Path 8 asset download sweep workflow.** `.github/workflows/verify-release.yml`
  fetches every release asset, verifies HTTP 200, sha256 against
  `SHA256SUMS.txt`, cosign signature, SBOM well-formedness, and asserts
  the `.deb`-extracted binary's `--version` matches the tag's core version.
  Auto-fires on `release: published` and weekly to catch asset rot.
- **CI: Path 2/3/4 release-paths validation workflow.** `.github/workflows/release-paths.yml`
  exercises upgrade-from-N-1, skip-version-from-N-2, and rollback flows
  on macOS arm64 / Ubuntu 22.04 / Windows. Runs in bootstrap mode until
  #30 ships tarballs; auto-tightens to hard-fail after.
- **CI: one-shot winget submission helper.** For first-time Jarvy.Jarvy
  publisher onboarding.

### Changed

- **Dash ↔ underscore tool aliasing is now uniform.** `nats-server` and
  `nats_server` resolve to the same tool in three places that previously
  diverged: `registry::get_tool()`, `commands::validate::validate_tools`,
  and `tools::spec::get_tool_spec()`. The third site was the sev-2 found
  during rc.1 soak and fixed in rc.2 — `validate` accepted `nats-server`
  but `setup --dry-run` reported `tool.unsupported` for the same name.
- **Brew tap auto-tap.** When `macos.brew` (or `linux.brew` fallback) is
  `org/tap/formula` form (exactly two slashes), install path runs
  `brew tap org/tap` first so a fresh box doesn't surface an "untrusted
  tap" error. Soft-fail; already-tapped is not a blocker.
- **`jarvy validate` and `jarvy setup --dry-run` now surface `[nuget]`.**
  Previously the new section silently dropped from the validate report —
  users would think their NuGet packages were configured when they
  weren't.
- **`publish-packages.yml` decouples downstream channels from crates.io.**
  Previously a transient crates.io publish failure left winget / chocolatey
  / homebrew unsynced. Each channel now has independent secret gates and
  failure modes.
- **Release binary `--version` comparison uses core version, not full
  tag.** rc tags like `v0.2.0-rc.2` build binaries that report
  `jarvy 0.2.0` (no prerelease suffix); the verify-release step now
  matches on core only.

### Fixed

- **Drop placeholder Windows package IDs from tool definitions.**
  Six tools previously listed placeholder `winget` IDs like
  `Pivotal.RabbitMQ` for upstream namespaces that the publisher had not
  actually claimed. Any party who registered that publisher could ship
  a malicious installer pinned by `winget install -e --id`. Replaced
  with explicit `// No first-party winget manifest as of YYYY-MM` notes;
  `tool.unsupported` telemetry fires in place at runtime.
- **Telemetry gate respects `[telemetry] enabled = false`.** Every
  `package.*` / `packages.*` / `package_command.failed` event reads
  `observability::telemetry_gate::is_enabled()` before emitting. Prior
  implementation leaked package events to OTLP when telemetry was
  disabled but an endpoint was set for unrelated reasons. Broke the
  documented opt-in contract.
- **MCP safety boundary applies to extended mutating tools.** The new
  drift/roles/templates/services tools all run through
  `resolve_within_workspace` — a path containing `..` or an absolute
  escape that lands outside the workspace root canonicalizes to a
  refusal, not a silent file write.
- **De-flaked `telemetry_smoke` integration test.** Ephemeral port +
  `#[serial]` annotation + 30s timeout, replacing the prior flaky
  hardcoded port that intermittently lost to other tests' bound
  sockets.
- **Mass conversion of ~200 `_registered_returns_some` tautology tests
  to `_registration_shape` tests.** The old tests verified
  `Some(_).is_some()` after registration — a tautology that always
  passed even when the underlying `ToolSpec` was structurally broken.
  Replaced with shape-asserting tests that fail when a tool's platform
  matrix degrades.

### Security

- **Supply-chain: no more placeholder winget IDs.** See Fixed above.
- **Package-name validation.** `validate_package_name` /
  `validate_package_version` refuse leading-`-`, URL schemes, shell-meta,
  and control bytes (ESC/BEL/DEL/NUL — closes ANSI injection in dry-run
  preview). `jarvy validate` runs them on every `[npm]/[pip]/[cargo]/[nuget]`
  entry.
- **Remote-config trust narrowing only.** `ConfigOrigin::Remote` tags
  remote-fetched configs; `allow_custom_commands`, `allow_custom_servers`,
  `allow_remote` (packages), and telemetry endpoint override are all
  refused for remote configs. Library hooks and the built-in `jarvy` MCP
  server remain trustable; user-authored extensions do not.

### Impact on v0.1.x users

- **Cargo (`cargo install jarvy`)** — resolves to v0.2.0; no breaking API
  surface in command flags. Existing `jarvy.toml` parses unchanged.
- **`.deb` / `.rpm` / `.dmg` / `.msi` / `.AppImage`** — install normally
  from the GitHub release.
- **Homebrew, install.sh, install.ps1** — still broken pending #30,
  same as v0.1.x. No regression; no improvement.
- **`jarvy update`** — package-manager paths upgrade fine. Binary
  fallback returns the documented "No binary for this platform" — same
  state as v0.1.x, tracked in #30.



Patch release closing the crates.io gap that v0.1.0 left open. No
runtime code changes — release-pipeline metadata only.

### Fixed

- **`jarvy-templates` is now publishable.** The crate was marked
  `publish = false` and lacked the `repository` / `homepage` metadata
  crates.io requires. Both `jarvy` and `cargo-jarvy` depend on it via
  `{ version = "X", path = "..." }`; crates.io strips `path` on publish
  and resolves from the registry, so the dep must already be available
  there. With `publish = false` + no version spec on the parents, the
  v0.1.0 `cargo publish` failed at `error: failed to verify manifest
  ... 'jarvy-templates' does not specify a version` before either crate
  could upload.
- **Both `jarvy-templates` path dependency declarations now carry a
  `version = "0.1.1"` requirement.** Required by `cargo publish` —
  without it the parent crate cannot verify against the published
  registry form of the dep.
- **`publish-packages.yml::publish-crates-io` step is now ordered.**
  Previously one `cargo publish` call attempted to publish `jarvy` as
  the workspace root; `jarvy-templates` was never published, so the
  parent's resolve always 404'd. The job now publishes
  `jarvy-templates` first, polls the crates.io index for up to 150s
  until the dep surfaces, then publishes `jarvy` with `--no-verify`
  (the workspace verify already ran at tag-build time; the
  post-publish re-verify would race the index refresh).

### Impact on v0.1.0 users

- The GitHub Release for v0.1.0 (all 49 binary assets + Sigstore
  signatures) is unaffected. `.deb` / `.rpm` / `.dmg` / `.msi` /
  `.AppImage` install paths work exactly as documented.
- `cargo install jarvy` resolves to v0.1.1 (the first crates.io
  release in the v0.1.x line). Users who tried `cargo install jarvy`
  during the v0.1.0 → v0.1.1 window saw `error: could not find
  jarvy 0.1.0 in registry crates-io`.
- Other channels (Homebrew tap, AUR, winget, Chocolatey) were not
  affected by this gap.

## [helm-v0.6.1] — Defense-in-depth: anonymize record-level attrs (2026-05-25)

### Fixed — `jarvy-telemetry-forwarder` Helm chart

- `transform/anonymize` now runs at the record level in addition to
  resource context. The 0.6.0 chart's hash statements were scoped to
  `context: resource`, so any client SDK that emitted a PII-shaped
  attribute as a per-event field (e.g. `tracing::info!(hostname = %h)`)
  bypassed the SHA256 hash and reached the backend in plaintext. The
  companion SDK fix moves `host.name` to the resource where it belongs;
  this chart change makes the privacy contract hold even when a future
  SDK regression mis-slots an attribute.
- Adds a second OTTL statement context to each pipeline
  (`log` / `datapoint` / `span`). Same `pii.hashedAttributes` list is
  reused — single source of truth.
- `keep_keys` is intentionally NOT applied at record level: event-
  specific attributes (`event`, `tools`, `duration_ms`) are not PII and
  must pass through. Resource-context `keep_keys` remains the
  allowlist enforcement point for per-process identity attrs.

### Migration

No action needed. Patch bump; no values surface change. Consumers can
no-op-upgrade.

## [helm-v0.6.0] — Grafana Cloud OTLP region default fix (2026-05-25)

### Fixed — `jarvy-telemetry-forwarder` Helm chart

- `exporter.endpoint` default hardcoded `prod-us-east-0`, but
  Grafana Cloud API keys are region-bound — keys issued for any other
  region 401 at the gateway and silently drop every export. The home
  stack lives on `prod-us-east-3`; consumers relying on the chart
  default had 100% export failure for traces, metrics, and logs.

### Changed — `jarvy-telemetry-forwarder` Helm chart

- Region hoisted to a new top-level value `grafanaCloud.region`
  (default `prod-us-east-3`). When `exporter.endpoint` is empty the
  chart now composes
  `https://otlp-gateway-<region>.grafana.net/otlp` via a single
  `exporterEndpoint` helper. Explicit `exporter.endpoint` still wins
  unchanged — operators pointing at Honeycomb / Datadog / in-cluster
  Tempo keep their override path.
- Both the Deployment's `BACKEND_OTLP_ENDPOINT` env and the
  `CiliumNetworkPolicy`'s FQDN derivation flow through the same
  helper, so a region bump can't desync the egress allow-list from
  the actual gateway.

### Migration — BREAKING

`exporter.endpoint` default is now empty (was a hardcoded URL).
Consumers that depended on the chart-default us-east-0 URL must
either:

- Set `exporter.endpoint` explicitly to keep their previous URL, or
- Align `grafanaCloud.region` with their stack's region (default
  `prod-us-east-3` works for home-cluster installs).

## [helm-v0.5.3] — `helm test` smoke pod actually works now (2026-05-20)

The 0.5.2 ship landed the `helm test` smoke pod + supporting infra
but the pod itself never ran green in CI on the first push (or on
local kind clusters). Three fixes were needed; this release rolls
them into a clean cut.

### Fixed — `jarvy-telemetry-forwarder` Helm chart

- NetworkPolicy: explicit egress allow for in-namespace `helm test`
  pods (paired with the 0.5.2 ingress rule). Production CNIs
  (Cilium, Calico) are conntrack-aware and don't need this — it's
  defense-in-depth for CNIs that evaluate egress per-packet
  (kindnet).
- Test pod hook-delete-policy drops `hook-succeeded` so the pod
  sticks around after a green run. Without this, `helm test --logs`
  failed with `pods ... not found` because the pod was deleted
  before the log fetch ran.
- Test pod template is now nil-safe (nested `if .Values.tests`
  before `.enabled`). Fixes a render failure when the template
  file from a newer chart is checked out alongside an older
  `values.yaml` that doesn't carry the `tests:` block (CI
  upgrade-leg pattern).

### Fixed — `helm-chart-ci` workflow

- Live install + upgrade step deletes the NetworkPolicy before
  running `helm test`. kindnet's netpol enforcement isn't
  conntrack-aware, so the collector's `wide-except-rfc1918`
  egress filter drops reply SYN-ACKs to in-cluster test pods.
  The netpol structure itself is fully covered by the render +
  kubeconform matrix; this step covers the receiver only.
- Common-annotations fanout test now sees the test pod carrying
  the chart's common annotations.
- Diagnostics-on-failure step dumps pods, services, endpoints,
  netpol, collector logs, test-pod logs, and runs a netpol-free
  repro curl. Costs nothing on green runs.
- Three other pre-existing matrix failures fixed in the same
  iteration (kept here for the changelog reader's context):
  helm/kind-action SHA pin corrected, promtool input shape
  (extract `.spec` for RuleGroups), extraEnv reject assertion
  accepts both helm 3.18 and helm 4.x schema messages.

### Migration

No action needed. The chart now passes `helm test` cleanly on
production CNIs. On stock kindnet (only relevant for in-cluster
test runs, not production), drop the NetworkPolicy before
running `helm test` — see the workflow comment for the rationale.

## [helm-v0.5.2] — `helm test` smoke pod + live HTTPS smoke script (2026-05-20)

### Added — `jarvy-telemetry-forwarder` Helm chart

- `templates/tests/otlp-smoke.yaml` — `helm test` hook pod that POSTs
  minimal OTLP/HTTP payloads at `/v1/{logs,metrics,traces}` on the
  Collector Service and asserts 2xx. Validates the receiver pipeline
  end-to-end after `helm install` without depending on the public
  ingress. Image `curlimages/curl:8.10.1` pinned, restricted-PSS
  compliant.
- `tests.*` values + schema validation (`enabled`, `image`,
  `resources`, `securityContext`). Disable with
  `--set tests.enabled=false`.
- NetworkPolicy now whitelists pods carrying BOTH the chart-test
  component label AND the release instance label — required so the
  `helm test` pod can reach the Collector through the otherwise
  locked-down ingress.
- `scripts/smoke-live.sh` — bash script that smokes the public
  HTTPS endpoint with the same three OTLP payloads. A diff between
  this and the in-cluster `helm test` isolates ingress (TLS,
  gateway, middlewares) as the suspect.
- Makefile targets: `helm-smoke-live` (live HTTPS) and
  `helm-test-kind` (in-cluster).
- `helm-chart-ci` kind job now runs `helm test` after the fresh
  install — receiver-pipeline regressions fail CI alongside the
  rendering/lint suite.

### Migration

No action needed; new behavior is purely additive. `helm test`
becomes opt-in once you upgrade — run it whenever you want
in-cluster validation of the receiver path.

## [helm-v0.5.1] — HTTPRoute `filters: null` lint fix (2026-05-17)

### Fixed — `jarvy-telemetry-forwarder` Helm chart

- HTTPRoute template no longer emits an empty `filters:` key (which
  YAML-parses as `null`) when traefik middlewares are disabled and no
  `extraFilters` are supplied. Surfaced by the `helm-chart-ci`
  matrix's `gatewayclass-envoy-accepted` scenario, which has been
  failing kubeconform-strict since the field was added — the Gateway
  API HTTPRoute schema types `filters` as `array`, not
  `array | null`. The fix wraps the key in an `or` guard so it is
  omitted entirely when no filters apply, which is the
  spec-compliant equivalent and produces no Argo CD drift.

### Migration

No action needed. Behavior at runtime is unchanged — a missing
`filters` key and an empty `filters` list both mean "no filters
applied". The diff visible on `helm diff upgrade` is purely the
removal of an `null`-valued field from the rendered HTTPRoute when
running without traefik middlewares.

## [helm-v0.5.0] — ExternalSecret Argo CD drift fix (2026-05-17)

Rendered ExternalSecrets now emit the two server-side defaults the ESO
admission webhook fills in (`target.deletionPolicy: Retain`,
`data[].remoteRef.conversionStrategy: Default`). Without these in the
chart's desired manifest, Argo CD's compare saw the webhook-injected
values as drift on every reconcile, leaving every install of this
chart perpetually `sync=OutOfSync, health=Healthy`. Discovered while
diagnosing the `jarvy-telemetry` Argo app on the home cluster on
2026-05-17.

### Added — `jarvy-telemetry-forwarder` Helm chart

- `secrets.externalSecrets.deletionPolicy` (default `Retain`) and
  `secrets.externalSecrets.conversionStrategy` (default `Default`)
  values. Both default to the ESO server-side default so existing
  installs see no semantic change — only that Argo CD diffs now show
  zero drift after the next `helm upgrade`. Override either if your
  use case needs `Delete` / `Merge` (deletionPolicy) or `Unicode`
  (conversionStrategy).
- `values.schema.json` constraints for both new fields with enum
  validation.

### Fixed — `jarvy-telemetry-forwarder` Helm chart

- ExternalSecret resources no longer drift in Argo CD when the ESO
  admission webhook fills server-side defaults. Bump and `helm
  upgrade` to clear the perpetual OutOfSync state.

### Migration

No action needed beyond `helm upgrade`. Defaults match ESO
server-side, so rendered output is functionally identical — the diff
visible on `helm diff upgrade` is purely the two new explicit
field assignments.

## [helm-v0.4.0] — Chart enhancement plan v3 (2026-05-14)

Multi-perspective parallel review (perf, security, QA, observability,
maintainability) produced a 27-item enhancement plan; all 27 items
ship together. Probe semantics, graceful shutdown, queue-saturation
alert, dashboard, recording rules, image-digest default, FQDN egress
mode, DoS-protection gate, split Service, container security context
schema constraints, runbook anchors in the ops doc, and 5 new CI
guards (kind install/upgrade, helm 3.14/3.16/3.18 matrix, promtool,
README↔schema drift, runbook-anchor check). 13 render scenarios pass,
8 template-time guards fire, `helm lint --strict` clean. **Backward
compatible**: defaults harden but no required-field renames; legacy
`networkPolicy.cilium.enabled=true` still works (now a synonym for
`egressMode: fqdn`).

### Added — `jarvy-telemetry-forwarder` Helm chart

A multi-perspective review (perf, security, QA, observability,
maintainability) produced a 27-item enhancement plan; all 27 items
shipped together. Chart version bump pending.

- **Probe split + pipeline-aware health.** Liveness no longer flips
  on `memory_limiter` backpressure (which would cascade-restart all
  replicas during burst — defeating the design). Readiness still
  flips so the LB sheds load. `health_check_v2`'s
  `check_collector_pipeline` exposes pipeline status on `/`;
  liveness gets a longer failureThreshold (6), readiness shorter
  periodSeconds (5). New `startupProbe` covers cold-pull on fresh
  nodes.
- **Graceful shutdown.** `terminationGracePeriodSeconds: 60` +
  `preStop: sleep 15` so the LB drains and the
  batch/exporter flushes in-flight records before SIGKILL.
- **Exporter queue saturation alert** — leading indicator that fires
  before `JarvyForwarderExporterFailing` starts dropping records.
  Backed by a recording rule (`jarvy_forwarder:exporter_queue_utilization:ratio`).
- **Pod restart alert** — closes the loop when pipeline alerts can't
  fire (pod never gets healthy enough to emit metrics).
- **Grafana dashboard** ConfigMap shipped via `grafana_dashboard=1`
  sidecar label. 10 panels: receiver rate, queue utilization,
  exporter rate, memory/CPU, tail-sampling decisions, allowlist
  drops, batch throughput, pod restarts, cert expiry.
- **Receiver auth** (`collector.receiverAuth.enabled`, opt-in)
  fronts the OTLP receiver with `bearertokenauth/receiver`. Multi-
  tenant deployments should enable.
- **Recording rules.** Repeated `rate(...)` over 5-10m windows
  hoisted into named recording rules; alerts + dashboard share one
  computation instead of recomputing per evaluation.
- **`networkPolicy.egressMode`**. Three modes: `wide` (legacy
  `to: []` on 443), `wide-except-rfc1918` (new default — excludes
  private IP ranges), `fqdn` (requires Cilium — restricts to the
  parsed exporter hostname).
- **DoS-protection gate**: non-Traefik GatewayClasses must supply
  `httpRoute.extraFilters` OR set `dosProtection.acceptUnprotected:
  true` — fails install otherwise. Closes the "I installed on Envoy
  and forgot the rate limit" exposure.
- **Split Service**: public OTLP Service (port 4318) +
  in-cluster metrics Service (port 8888). In-cluster scrapers
  cannot accidentally reach the OTLP receiver and self-metrics no
  longer mix with public ingress traffic.
- **Production-overlay digest pinning**: chart ships with
  `collector.image.digest` set to a real `sha256:` digest by
  default; CI scenario `production-overlay` asserts the rendered
  image string carries the digest.
- **Grafana dashboard's `runbook_url` anchors** all exist in
  `docs/operations/telemetry-forwarder.md` (11 new
  `{#alert-*}`-anchored subsections with diagnosis steps).
- **CI**: kind install + upgrade smoke test (k8s 1.31); helm
  3.14/3.16/3.18 render matrix; promtool PromRule validation;
  README ↔ schema drift check; runbook-anchor grep.

### Changed — `jarvy-telemetry-forwarder` Helm chart

- **CPU limit removed** from `collector.resources.limits`. CFS-quota
  throttling on an I/O-bound forwarder adds 10-100ms p99 latency on
  burst with no upside. Floor preserved via `requests.cpu: 100m`.
- **HPA `scaleDown` policy** is now explicit (`drop 1 pod / 60s`)
  instead of the K8s default (halve replicas per 15s) which causes
  replica thrash near `memory_limiter` pressure.
- **PDB uses `maxUnavailable: 1`** (not `minAvailable: 1`) so node
  drains proceed one pod at a time without stalling forever waiting
  for real-Ready. Mutually exclusive with `minAvailable` —
  template-time `fail()` catches both-set misconfiguration.
- **`pdb.minAvailable` + `pdb.maxUnavailable`** mutually exclusive
  (template `fail`). **`tls.certManager.enabled=true` +
  `tls.existingSecretName`** mutually exclusive (template `fail`).
- **`_helpers.tpl` labels order**: chart-managed labels are emitted
  LAST so `commonLabels` cannot overwrite `app.kubernetes.io/name`
  and steer NetworkPolicy / ServiceMonitor away from real pods.
- **`automountServiceAccountToken: false`** stays hardcoded in both
  ServiceAccount and Pod spec (no values knob); render-time CI
  asserts catch regressions.
- **`enableServiceLinks: false`** on the pod — saves env-var bloat
  on busy namespaces; speeds cold start.
- **ServiceMonitor**: `honorLabels` is now actually rendered (was a
  ghost setting). `path: /metrics`, `scheme: http`, and
  `scrapeTimeout` explicit so a future port change doesn't break
  scrape silently. ServiceMonitor selector now matches the new
  metrics-only Service (`app.kubernetes.io/component: metrics`).
- **ServiceMonitor `metricRelabelings`**: tightened keep-list. Drops
  high-cardinality `otelcol_processor_transform_*_modified` series
  (none of which exist — see Fixed below) and keeps the operational
  subset.
- **`saltStale` alert** rebuilt: now reads
  `external_secrets_sync_calls_total` (the only series that exists
  for "salt content was refreshed"). The old query referenced a
  non-existent `kube_secret_created` metric and would have stayed
  silent forever.
- **`allowlistDroppingKeys` alert** rebuilt: compares
  `otelcol_processor_incoming_items` vs `outgoing_items` on the
  `transform/keep_allowlist_attrs` processor. The old query
  referenced non-existent `*_modified` counters.
- **`bearertokenauth` extension** for the backend exporter, plus
  optional `bearertokenauth/receiver` for inbound auth.
- **Container `securityContext`** explicitly sets
  `runAsNonRoot: true` and `seccompProfile: RuntimeDefault`
  (belt-and-suspenders over the pod-level setting). Schema rejects
  flipping `privileged`, `allowPrivilegeEscalation`,
  `readOnlyRootFilesystem`, or dropping `capabilities.drop: ALL`.
- **`exporterFailing` alert threshold units** documented as
  records/sec; docs/values comments aligned (was previously
  conflicting on per-second vs per-minute).
- **Gateway listener TLS `options:`** rendered through as-is so
  operators can pass GatewayClass-specific knobs (e.g.
  `gateway.envoyproxy.io/min-tls-version`).
- **README** updated: salt-rotation wording, accurate schema
  invariants list, new ConfigMap/dashboard/PrometheusRule entries
  in "What gets installed", egressMode and DoS-protection notes.

### Removed — `jarvy-telemetry-forwarder` Helm chart

- The `cilium.enabled` values knob is still accepted but is now a
  synonym for `egressMode: fqdn`; future versions may remove.

[helm-v0.4.0]: https://github.com/Cliftonz/Jarvy/releases/tag/helm-v0.4.0

---

The entries below belong to the Jarvy CLI's pending `[Unreleased]`
section; they ship with the next CLI tag, NOT with `helm-v0.4.0`.
Listed here so the helm-v0.4.0 release notes do not absorb them.

### Sandbox auto-detection (PRD-053)

- **Sandbox auto-detection (PRD-053).** New `src/sandbox/` module
  detects AI agent sandboxes (Claude Code, Cursor, e2b, Modal,
  Daytona, Replit), long-running container envs (GitHub Codespaces,
  Gitpod, devcontainers), and a generic `/.dockerenv` + non-TTY
  fallback. `crate::sandbox::is_seamless()` is the canonical
  "unattended" predicate; CI detection is now a strict subset.
  `JARVY_SANDBOX=0` disables detection, `JARVY_SANDBOX=1` forces
  generic-container (or whatever named provider also matches).
- **Seamless mode** wires through telemetry auto-disable, update-
  check suppression, first-run welcome suppression, brew auto-install
  block, and secrets non-interactive default — five subsystems that
  previously each carried their own `env::var("CI")` heuristic now
  share one predicate.
- **Verify-only fallback** in `jarvy setup`. When the sandbox cannot
  install tools (read-only rootfs, no user-scope package manager, no
  passwordless sudo), setup runs the doctor pipeline inline and exits
  `PREREQ_MISSING (3)` on gaps; clean runs return `0` with a
  verify-only success message. The probe records why via a
  `VerifyOnlyReason` enum (`NoJarvyHome` / `ReadOnlyRoot` /
  `NoInstallPath` / `Forced`) so support tickets explain which gate
  tripped.
- **Auto-baseline.** On the first seamless-mode run with zero gaps,
  Jarvy snapshots the current state as `.jarvy/state.json` so
  subsequent runs can do meaningful drift checks. Gated on a *full*
  doctor match — partial matches never auto-baseline (PRD-053 risk
  row 2). Works on both the install-capable and verify-only paths so
  pre-loaded sandbox images still get a baseline.
- **Seamless banner** on stderr, one line per process, summarizing
  which provider was detected and the `JARVY_SANDBOX=0` escape hatch.
  Muted by `--quiet`, `-q`, `--json`, `--format=json`,
  `--log-format=json`, or `JARVY_QUIET=1`. The corresponding
  `tracing::info!(event = "sandbox.detected")` fires regardless so
  `jarvy.log` records the decision even for JSON consumers.
- **`is_seamless_auto()`** — same as `is_seamless()` minus *forced*
  sandbox detection. Telemetry + update auto-disable now route
  through this variant so a hostile dotfile or compromised
  devcontainer base image that sets `JARVY_SANDBOX=1` cannot silence
  security-patch updates or anomaly telemetry on a victim's machine
  (PRD-053 security review F1).

### Changed

- **`JARVY_HOME` validation.** Paths must be absolute and contain no
  `..` traversal components; on Unix, existing paths must be owned by
  the current uid. Defends against `sudo -E jarvy ...` patterns where
  a less-privileged actor's env points a privileged jarvy run at
  `/etc` or `/root/.ssh` (PRD-053 security review F2).
- **Install-capability probe** writes to a per-PID `.probe-<pid>`
  filename via `OpenOptions::create_new(true)` (`O_CREAT|O_EXCL`)
  instead of `fs::write` to `.probe`. A pre-staged symlink at the
  probe path now errors out instead of being silently followed and
  clobbered (PRD-053 security review F3).
- **Banner emission moved after panic-hook install** in `main.rs` so
  any future stderr-write failure during banner emission produces a
  structured panic message instead of a default backtrace dump.
- **`detect()` and `ci::detect()` are now cached** via `OnceLock` —
  env vars and `/.dockerenv` do not change mid-run, and the previous
  implementation re-walked ~25 `getenv` calls per `is_seamless()`
  invocation × 4 callers per `jarvy setup`. Telemetry `ci_detected`
  event likewise fires at most once per process instead of once per
  call.
- **`InstallCapability::VerifyOnly` carries a `VerifyOnlyReason`** so
  log lines and tickets explain *which* probe tripped.

### Removed

- **`update::config::is_ci_environment` and the parallel shim in
  `onboarding::detection`**. Both were thin re-exports of
  `sandbox::is_seamless()`; in-tree callers now use the canonical
  predicate directly. Jarvy is a `bin` crate, no external library
  consumers to break.
- **Hand-rolled `which()` helper in `src/sandbox/mod.rs`** replaced
  by the `which` crate (already a project dep). Local impl ignored
  the Unix exec bit and only handled three Windows extensions.

### Security

- **Test images pinned by sha256 digest.** `debian:bookworm-slim` and
  `buildpack-deps:bookworm-scm` in `tests/sandbox_integration.rs`
  resolve to specific bytes regardless of registry tag drift or tag-
  replay MITM.
- **Read-only binary bind-mount.** The host's jarvy binary is mounted
  into integration-test containers via
  `Mount::bind_mount(...).with_access_mode(AccessMode::ReadOnly)` so
  a malicious container cannot truncate or replace the host binary
  mid-test (PRD-053 security review F8).

### Tests

- 10 new sandbox unit tests: forced-with/without named signal,
  `JARVY_SANDBOX=0 && CI=true` precedence, `is_seamless_auto` matrix,
  generic-container truth table, `VerifyOnlyReason` Display, force-
  verify-only probe short-circuit, banner idempotence.
- 4 new docker-backed integration tests: partial-match negative gate
  (must not auto-baseline on gaps), banner suppression with
  `--format=json`, banner suppression with `JARVY_QUIET=1`, verify-
  only must not overwrite an existing `state.json`.
- Cross-module env-isolation via `#[serial_test::serial(ci_sandbox_env)]`
  on every `ci::tests` and `sandbox::tests` function so the two
  suites cannot race on shared env vars (`CI`, `GITHUB_ACTIONS`,
  `CODESPACES`).

## [v0.1.0] — First feature-complete milestone (2026-05-27)

First feature-complete stable. Closes the round-2 hardening review
(45 items across two passes), ships clean-laptop onboarding, and
publishes 14 ready-to-copy `jarvy.toml` project templates.
Telemetry-enabled deployments now actually export records — four
compounding OTLP bugs that left env-only opt-in silently emitting
zero records are fixed (see `### Fixed` below). The public surface
from v0.0.5 is preserved; everything below is either additive,
fail-closed by default, or a tightening of internal invariants.

### Upgrading from v0.0.5

`jarvy update --channel beta` (and `jarvy update` in general) is broken in
v0.0.5 — it exits 0 without actually upgrading. Two pre-existing bugs in
v0.0.5: a hardcoded `version = "0.2"` clap string that makes v0.0.5 think
it is newer than v0.1.0, plus an update path that never triggers an
artifact download. Both are fixed in v0.1.0 but cannot be patched
retroactively. **v0.0.5 users must upgrade by reinstalling via their
package manager**, not via `jarvy update`:

- macOS (Homebrew tap restored): `brew upgrade jarvy`
- Debian/Ubuntu: `sudo apt install ./jarvy_0.1.0_amd64.deb`
- Fedora/RHEL: `sudo dnf install ./jarvy-0.1.0-1.x86_64.rpm`
- Arch (AUR): `yay -Syu jarvy-bin`
- Windows (winget): `winget upgrade Jarvy.Jarvy`
- Cargo: `cargo install jarvy --force`

From v0.1.0 onward, `jarvy update --channel beta` and `jarvy update`
work as documented.

### Added

- **Project templates.** `examples/<stack>/jarvy.toml` ships 14
  validated drop-in configs (node-npm/pnpm/bun, deno, python-api/uv,
  go-api, rust-cli/workspace, ruby-rails, java-spring, react-app,
  fullstack, k8s-platform). Companion docs at
  `docs/templates-index.md` give an AI-agent decision table mapping
  detect-by signals (lockfiles, manifests) to template URLs.
- **Clean-laptop onboarding.** New `Makefile` + idempotent
  `scripts/bootstrap.sh` give contributors a two-command setup
  (`curl install.sh | bash` then `make setup`). Bootstrap script
  honors `JARVY_CHANNEL` for stable/beta/nightly, falls back to
  `wget` if `curl` is missing, and forwards extra args to
  `jarvy setup`. shellcheck-clean.
- **`jarvy validate` recognizes the full top-level surface.**
  `[npm]`, `[pip]`, `[cargo]`, `[commands]`, `[drift]`, `[git]`,
  `[network]`, `[logging]` no longer trigger
  "unknown configuration section" warnings. Toolchain channel
  aliases (`stable`, `beta`, `nightly`, `lts`, `current`) are
  accepted as valid version strings — `rust = "stable"` validates
  cleanly.
- **`SecretError::PathEscapesProject`** + `JARVY_ALLOW_EXTERNAL_SECRETS`
  override. `[env.secrets] from_file` paths that resolve outside
  the project root and `$HOME` after symlink-resolving
  canonicalization are refused by default. Common legitimate paths
  (`~/.aws/credentials`, `<project>/.env.secret`) keep working.
  Override with `JARVY_ALLOW_EXTERNAL_SECRETS=1`.
- **`tools::pinned_installer::PinnedInstaller`** helper for the
  curl-bash class of installers. arctl, kmcp, and ollama (Linux
  fallback only) now fetch their installer scripts at a pinned
  commit, sha256-verify the body, and refuse to exec on mismatch —
  same pattern Homebrew already used. Refreshing a pinned installer
  requires updating the commit + sha256 constants together.
- **POSIX env-var grammar validation** before writing
  `[env.vars]` to shell rc files. Keys not matching
  `^[A-Za-z_][A-Za-z0-9_]*$` are skipped with a structured
  `event="env.refused_invalid_key"` warning instead of corrupting
  `~/.bashrc` / `~/.zshrc`.
- **`tools::install_method`** canonical classifier
  (`Brew`/`Cargo`/`Nvm`/`Pyenv`/`Rustup`/`Snap`/`System`/
  `NotFound`/`Unknown`). `commands::diagnose`, `commands::drift`,
  and `observability::bundle` all delegate here instead of
  hand-rolling three near-identical detectors.
- **Unsupported-tool feedback loop with telemetry-first delivery.**
  When a user (or AI agent) hits a tool Jarvy doesn't support, the
  run now surfaces a structured request payload — fuzzy Levenshtein
  suggestions with prefix-match boost, a `define_tool!` scaffold
  snippet, exit code `TOOL_UNSUPPORTED` (8), and a delivery channel.
  Telemetry is canonical: no GitHub account needed and zero triage
  work for the maintainer. The pre-filled `tool_request.yml` issue
  URL is surfaced only when telemetry is off, with
  `jarvy telemetry enable` offered as a one-time alternative. New
  `jarvy tools --request <name> [--open]` flag with pretty / JSON /
  YAML / TOML output. Setup-path returns exit 8 only when every
  configured tool was unknown — mixed runs still return 0 so partial
  setups succeed. Canonical `tool.unsupported` event with uniform
  field shape across both call sites; OTEL counter
  `jarvy.tool.unsupported` renamed from `…not_supported` to match.
- **`crates/jarvy-templates` workspace member** — dep-free crate
  shipping `validate_tool_name`, `render_tool_template`,
  `MAX_TOOL_NAME_LEN`, and the embedded `define_tool!` template.
  `cargo-jarvy` depends only on this crate now; clean-build time
  drops from minutes (full jarvy lib) to ~7s.

### Changed

- **Logging pipeline rewired** to `tracing_appender::rolling` for
  daily rotation + `tracing_appender::non_blocking` for buffered
  writes. `analytics::shutdown_logging()` flushes both the
  `SdkLoggerProvider` and the file `WorkerGuard` before
  `process::exit`, so buffered records aren't lost on early
  termination. `EnvFilter` now has a default-on floor of
  `warn,jarvy=info` if `RUST_LOG` is unset.
- **`Hook::run_with_policy`** collapsed from a 3-state `HookOutcome`
  enum to `Result<(), HookError>`. Production callers only ever
  checked `Fail` vs not-Fail; the warning-on-`continue_on_error`
  side effect already conveyed the difference. The new `Err` case
  returns the underlying `HookError` so `error_codes::HOOK_FAILED`
  callers keep working.
- **`Sanitizer::sanitize_borrowed`** returns `Cow<'_, str>` so the
  no-match path skips allocation entirely. `Sanitizer::sanitize`
  preserves the same fast path internally.
- **`tracing::warn!` → `tracing::error!`** on `tool.failed`,
  `hook.failed`, `hook.timeout`, `config.parse_error`, and
  `telemetry.endpoint.refused`. These are operator-actionable
  conditions, not advisory.
- **Subprocess spans.** `services::run_command` and
  `tools::common::run_capture` are now wrapped in
  `tracing::info_span!("subprocess.exec", cmd, args_count, ...)`
  with start/duration/exit_code events.
- **`paths.rs` cleanup.** `cache_dir` inlined into
  `remote_config_cache_dir` (only caller); `#![allow(dead_code)]`
  removed since every public function has external callers now.

### Security

- **CA-bundle trust check tightened.** `network::propagate` no
  longer accepts paths under the broad `~/.jarvy/` cache prefix —
  only `~/.jarvy/ca/` is trusted, with a trailing-slash anchor so
  `~/.jarvy/ca-attacker/...` can't slip through.
- **Cross-origin redirects refused** on
  `remote::validated_get` / `fetch_remote_config`. `ureq` agent
  now uses `.max_redirects(0)`; redirects must be revalidated
  through the policy gate.
- **Sigstore companion verification.** `update::release` returns
  `None` for cosign companion files when the `.sig`/`.pem` aren't
  exact-match siblings — a substring-match bug that would have let
  a malicious tarball claim sibling signatures was closed.
- **`exec.rs` deleted** (zero-caller speculative seam).
- **`team::inheritance::transform_github_url`** duplicate deleted;
  callers route through the canonical `remote::transform_github_url`
  so URL hardening lives in one place.

### Fixed

- `validate_get` rejected URLs with empty hosts under `file://`
  scheme but didn't match the documented "scheme not allowed"
  error string. Test relaxed to accept any error variant; behavior
  unchanged.
- `paths::remote_config_cache_dir` now reads `JARVY_HOME`
  consistently with the rest of `paths.rs` (was hand-rolling the
  override before).
- `update_rc_content` argument order documented; previously the
  test suite caller had `(content, &vars, &ctx, ShellType)` instead
  of the actual `(content, ShellType, &vars, &ctx)`.
- **OTLP env-only opt-in now actually exports.** Four compounding
  bugs caused `JARVY_TELEMETRY=1` + `JARVY_OTLP_ENDPOINT=…` to
  silently produce zero records, and even file-flag opt-in lost
  every metric point on short-lived commands:
  (1) `init_logging` gated on the file flag, missing the env
      override — the OTEL log layer was excluded from the
      subscriber whenever telemetry was opt-in via env only;
  (2) `opentelemetry-otlp` 0.31's `with_endpoint()` is the FULL URL
      not a base — a bare `http://localhost:4318` produced `POST /`
      and the collector 404'd every batch. New
      `analytics::resolve_otlp_endpoint(base, signal)` appends
      `/v1/{logs|metrics|traces}` idempotently;
  (3) `otlp_logs_endpoint()` ignored the file config's
      `[telemetry] endpoint` — setting it via
      `jarvy telemetry set-endpoint` silently failed to reroute
      logs. The logger builder now reads the merged
      `TelemetryConfig`;
  (4) `telemetry::shutdown()` was defined but never called from
      `main`, so the `SdkMeterProvider`'s 60s `PeriodicReader` had
      no chance to flush on `jarvy setup`-length runs.
      Now called alongside `analytics::shutdown_logging()` in
      the exit path.
- **`host.name` emitted as resource attribute, not per-event
  field.** Grafana Cloud was receiving plaintext
  `hostname=<machine>.local` from the `setup.inventory` event,
  defeating the chart-side anonymize pipeline (which only operated
  on resource-context attrs). Build a shared
  `opentelemetry_sdk::Resource` once at telemetry init with
  `service.name`, `service.version`, `host.name`, `os.type`,
  `os.description`; attach to both `SdkLoggerProvider` and
  `SdkMeterProvider`. Previously `service.name` defaulted to
  `unknown_service`, which broke stack-level filtering and made
  "where did this record come from" guesswork. Local file logger
  and stderr layers still print plaintext (those are operator-
  owned sinks, not the egress channel).
- **`emit_telemetry_hint_if_undecided`** now consults
  `telemetry::is_enabled()` first so a user running with
  `JARVY_TELEMETRY=1` doesn't see "telemetry is opt-in and
  currently off" right after a run that just emitted records.
- **Drift hash respects `--file`.** `jarvy drift` hashed
  `<project_dir>/jarvy.toml` regardless of the `--file` flag, so
  drift detection silently used the wrong file when a non-default
  config path was supplied.
- **`set_up_os` matches `env::consts::OS` casing.** A capitalization
  mismatch in the platform-dispatch table caused setup to fall
  through to the unknown-OS path on some platforms.

### Tests

- 1,633+ tests passing across lib + binary + integration suites
  (was ~1,580). Highlights of the new coverage:
  - `validated_get` rejection tests for HTTP-to-remote, disallowed
    host, `file://` scheme, missing scheme.
  - `Hook::run_with_policy` outcome matrix (dry-run / success /
    failure × continue_on_error true|false).
  - `verify_no_tar_escape` containment tests + symlink-escape
    refusal.
  - Cosign companion exact-match (no substring) regression.
  - Path-containment refusal + `JARVY_ALLOW_EXTERNAL_SECRETS=1`
    override path for `[env.secrets] from_file`.
  - Shell-interpreted-key table-driven test
    (`every_shell_interpreted_key_refuses_bang_prefix`) so adding
    a new shell-interpreted git config key lights up the test
    suite immediately.
- `#[serial_test::serial]` annotations added for
  `JARVY_ALLOW_*` env mutations to keep parallel runs isolated.

### Docs

- `CLAUDE.md` Logging section rewritten to match the actual
  `src/logging/` (thin re-export layer) and `src/observability/`
  (where rotation + sanitizer + analytics live) split.
- `examples/README.md` + `docs/templates-index.md` published as
  the human/AI-facing template indexes.
- `llms-full.txt` "Project Templates" section added (with
  `docs/llms.txt` + `docs/llms-full.txt` symlinks for the published
  docs site).

## [v0.0.5] — Chocolatey install script + bundled v0.0.4 fixes (2026-05-05)

Folds in everything queued for v0.0.4 (which was tagged but never
publicly published) plus a Chocolatey install-script fix.

### Fixed

- **Chocolatey package** v0.0.3 failed moderation with `404 Not Found`
  for the install URL. Two bugs in
  `dist/windows/chocolatey/tools/chocolateyinstall.ps1`:
  - URL pattern referenced
    `jarvy-vVERSION_PLACEHOLDER-x86_64-pc-windows-msvc.zip` — but
    cargo-packager produces `.msi` and `.exe`, no `.zip` for Windows.
  - VERSION_PLACEHOLDER and SHA256_PLACEHOLDER were never substituted
    because the publish workflow only ran sed against `jarvy.nuspec`,
    not the install script.

  Rewrote the install script to use `Install-ChocolateyPackage` with
  `-FileType msi` and silent install args, pointing at the actual
  `jarvy_<v>_x64_en-US.msi` asset. Updated
  `publish-packages.yml::update-chocolatey` to substitute both files
  AND pull the real msi SHA256 from `SHA256SUMS.txt` so the integrity
  check passes.
- **`cargo fmt --check`** drift in `src/team/inheritance.rs:760-768`
  (single-quoted TOML literals from v0.0.3 needed compaction).
- **OpenSSF Scorecard** failed on v0.0.3 tag with `Only the default
  branch main is supported`. ossf/scorecard-action explicitly refuses
  tag-push triggers. Restored `push: branches: [main]` for scorecard
  only — every other validating workflow stays tag-triggered.
- **Homebrew tap publish** now gracefully skips when
  `HOMEBREW_TAP_DEPLOY_KEY` is not configured. Previously the missing
  secret failed the whole `publish-packages.yml` workflow, masking
  the success of crates.io, AUR, winget, and Chocolatey jobs.

### Validated downstream (v0.0.3)

After the v0.0.3 fixes, the following propagation channels worked:

- ✅ crates.io: jarvy@0.0.3 + cargo-jarvy@0.0.3 published
- ✅ AUR (jarvy-bin)
- ✅ Submit to winget (publish-packages.yml job; separate winget.yml
  still needs manual first submission)
- ✅ GitHub Pages docs site (after maintainer enabled Pages)
- ❌ Chocolatey: failed moderation due to broken install script
  (v0.0.5 fixes)
- ⚠️  Homebrew tap: pending secret config (now non-blocking)

### Note

v0.0.4 was tagged but the draft was never publicly published —
v0.0.4's fixes ship together with the Chocolatey fix as v0.0.5 to
reduce propagation churn (one round of crates.io / AUR / etc.
updates instead of two back-to-back).

## [v0.0.4] — Lint formatting + scorecard + homebrew-tap guard (2026-05-05)

### Fixed

- **`cargo fmt --check`** failed in the Lint job on
  `src/team/inheritance.rs:760-768` because the v0.0.3 single-quoted
  TOML literal edits left format strings on multiple lines that
  rustfmt wanted compacted. Re-ran `cargo fmt` to normalize.
- **OpenSSF Scorecard** failed on the v0.0.3 tag with `Only the
  default branch main is supported`. ossf/scorecard-action explicitly
  refuses tag-push triggers; v0.0.3's trigger trim moved scorecard
  off main-push, which broke it. Restored `push: branches: [main]`
  for scorecard only — every other validating workflow stays
  tag-triggered. Release-tag scorecard runs produce no useful signal
  anyway since the action only inspects the default branch.
- **Homebrew tap publish** now gracefully skips when
  `HOMEBREW_TAP_DEPLOY_KEY` is not configured. Previously the whole
  `publish-packages.yml` workflow exited 1 with "API_TOKEN_GITHUB
  and SSH_DEPLOY_KEY are empty", masking the success of crates.io,
  AUR, winget, and Chocolatey jobs. New behavior: missing secret
  emits a warning ("set per docs/MAINTAINER_RELEASE_GUIDE.md") and
  the push step is skipped via `if:` guard.

### Validated downstream (v0.0.3)

After the v0.0.3 fixes, the following propagation channels worked:

- ✅ crates.io: jarvy@0.0.3 + cargo-jarvy@0.0.3 published
- ✅ Submit to winget (job inside publish-packages.yml; the separate
  winget.yml workflow still requires manual first submission per
  v0.0.3 release notes)
- ✅ Chocolatey
- ✅ AUR (jarvy-bin)
- ✅ GitHub Pages docs site (after maintainer enabled Pages in repo
  Settings)
- ⚠️  Homebrew tap: blocked on `HOMEBREW_TAP_DEPLOY_KEY` secret;
  v0.0.4 makes this a non-blocker so missing-secret no longer fails
  the whole workflow.

## [v0.0.3] — Unblock crates.io and Homebrew downstream publish (2026-05-05)

Patch release. v0.0.2 went live on the GitHub release page but the
crates.io and Homebrew workflows that fire on `release: published`
both failed, leaving `cargo install jarvy` and
`brew install Cliftonz/tap/jarvy` unavailable.

### Fixed

- **Cargo.toml** declared `readme = "README.md"` (uppercase) but the
  tracked file is `Readme.md` (mixed case). On macOS the difference
  is invisible (case-insensitive filesystem); on the Linux CI runner
  it failed `cargo publish` with `readme "README.md" does not appear
  to exist`. Both `Publish Crate` and `Publish to Package Managers`
  workflows hit the same error. Same fix in the `include = [...]`
  manifest list. Now matches what's actually in the git tree.
- **`.github/workflows/winget.yml`** was scaffolded from a different
  project's template and never customized — `identifier: Benji377.Tooka`
  and `fork-user: Benji377` referenced a totally unrelated package.
  Rewrote with placeholder TODO values for `Jarvy.Jarvy` /
  `Cliftonz` and changed the trigger from `release: published` to
  `workflow_dispatch` only. winget-releaser cannot create a brand-new
  package registration; the first submission must go through
  `wingetcreate new` and a hand-reviewed PR to microsoft/winget-pkgs.
  After that's merged the trigger can be flipped back.

### Removed

- Duplicate `.github/workflows/crates.yml` deleted. Both that and
  `publish-packages.yml::publish-crates-io` were firing on
  `release: published` and trying to `cargo publish`. Even if both
  had the right secret, the second one would race-fail with "crate
  version already exists". Kept the version inside `publish-packages.yml`
  because it composes with the Homebrew tap update via `needs:`.
- `docs/release-testing.md` and `docs/release-quirks-jarvy.md`
  references to `crates.yml` updated to point at the surviving
  workflow path.

### Known issues (not fixed in this release)

- **GitHub Pages** is not enabled for `Cliftonz/Jarvy` repo — the
  Deploy Docs workflow fails with `HttpError: Not Found ... Ensure
  GitHub Pages has been enabled`. Fix is in repo Settings → Pages,
  not in code. Until enabled, the docs site at jarvy.dev (or
  whichever Pages URL ends up provisioned) won't update on release.
- **winget first submission** still requires manual `wingetcreate new`
  intervention (see Fixed above for the workflow disable).

## [v0.0.2] — Cosign verify-command case fix (2026-05-05)

Patch release fixing the cosign verification snippet baked into
release notes, SECURITY.md, and docs/release-quirks-jarvy.md.

### Fixed

- **release notes / SECURITY.md / docs**: the
  `--certificate-identity-regexp` value used `Cliftonz/jarvy`
  (lowercase j). The actual Sigstore cert subject GitHub Actions
  produces is `Cliftonz/Jarvy/...` (capital J — the repo's
  canonical case). cosign's regex is case-sensitive, so users
  copy-pasting the verify command from the v0.0.1 release page
  saw "none of the expected identities matched" even though the
  signature was valid. Corrected all three sources to
  `Cliftonz/Jarvy/`. github.com URLs elsewhere in the repo are
  unchanged because GitHub URL matching is case-insensitive — only
  cosign's regex was affected.

## [v0.0.1] — Initial public release (2026-05-05)

First publicly tagged stable release. Validated through the
v0.1.0-rc.1 → v0.1.0-rc.9 soak cycle (same tree, version-string
only differs); cut as 0.0.1 to keep the first-stable surface narrow
and reserve room for 0.1.0 as the first feature-complete milestone.

### Features

- **provisioner:** Cross-platform tool provisioner driven by `jarvy.toml`
  (macOS, Linux, Windows) with native package managers
- **tools:** 154+ tool registry covering compilers, runtimes, CLIs, container
  tools, Kubernetes ecosystem (kubectl, helm, k9s, kagent, kmcp, arctl), cloud
  CLIs (gcloud, aws, az), security tools, observability (opentelemetry-collector),
  Dockerfile converter (dfc) (PRD-013)
- **tools:** Parallel version checking with rayon for ~5x speedup; batch
  package-manager operations
- **tools:** Declarative `define_tool!` macro for tool definitions (~2000 lines
  reduced)
- **tools:** Strict (`depends_on`) and flexible (`depends_on_one_of`) tool
  dependencies with topological install ordering (PRD-034)
- **hooks:** 29+ default post-install hooks for shell completion and
  configuration; idempotent, advisory, user-overridable
- **roles:** Role-based configurations with deep inheritance, version overrides,
  `roles list|show|diff` commands (PRD-033)
- **packages:** Language package deps via `[npm]`, `[pip]`, `[cargo]` —
  package-manager auto-detection, virtualenv support, lockfile install (PRD-039)
- **git:** Git configuration automation — identity, SSH/GPG signing, default
  branch, aliases, credential helper auto-detect per OS (PRD-041)
- **drift:** Configuration drift detection with SHA-256 file hashing, version
  policies, `jarvy drift check|status|accept|fix` (PRD-043)
- **update:** Self-updating with stable/beta/nightly channel selection,
  throttled checks, rollback, multi-method install detection (Homebrew, Cargo,
  apt, dnf, winget, Chocolatey, Scoop, binary fallback) (PRD-035)
- **telemetry:** OTEL-unified logs, metrics, optional traces; OTLP HTTP/gRPC
  endpoints; CI auto-disable; `jarvy telemetry status|enable|disable|test|preview`
  (PRD-022, PRD-050)
- **logging:** Persistent file logging with rotation, gzip compression,
  sensitive-data redaction; `jarvy logs view|stats|clean|config` (PRD-050)
- **ticket:** Debug bundles via `jarvy ticket create|show|list|clean` — ZIP with
  system info, tool versions, sanitized logs (PRD-050)
- **network:** Corporate proxy support — HTTP/HTTPS/SOCKS, NO_PROXY, custom CA
  bundles, per-tool overrides, secure password sources (PRD-019)
- **services:** Docker Compose and Tilt backend support
- **ci:** Auto-detection for 11 CI/CD providers with provider-specific output
- **env:** Environment variable management with `.env` generation and shell rc
  updates
- **mcp:** MCP server exposing tools and resources for AI assistants
- **interactive:** Menu mode when running `jarvy` without a subcommand
- **bootstrap:** `jarvy bootstrap`, `jarvy configure`, `jarvy diagnose` for
  onboarding (PRD-023)

### Distribution

- Multi-channel: crates.io, Homebrew tap, AUR (source + binary), `.deb`, `.rpm`,
  winget, Chocolatey, universal install scripts for macOS/Linux/Windows (PRD-012)
- **Prebuilt platforms**: macOS arm64, Linux x86_64 (musl), Linux aarch64,
  Linux armv7, Windows x86_64. macOS Intel (x86_64) **not shipped as prebuilt** —
  Intel users install via `cargo install jarvy` or Homebrew (both compile from
  source). See `docs/release-testing.md` for rationale.
- Sigstore keyless signing for all release artifacts (PRD-020)
- SBOM generation in SPDX 2.3 and CycloneDX 1.4 formats per release (PRD-020)
- GitHub build provenance attestation per release (PRD-020)
- Opt-in early-release channel: `JARVY_CHANNEL=beta` env var on install
  scripts; `[update] channel = "beta"` in `~/.jarvy/config.toml`;
  `jarvy update --channel beta`

### Quality & Security

- Clippy gate, mutation testing, fuzzing, coverage, benchmarks, OpenSSF
  Scorecard (PRD-018)
- Hybrid cross-platform E2E testing harness (PRD-038)
- Tag-signing enforcement (SSH or GPG) on release workflow
- Cosign keyless signing via GitHub OIDC for all release artifacts

### Infrastructure

- Semantic version checking with proper semver operators
- Cross-platform shell detection and hook execution
- Workspace lint configuration; Rust 2024 edition; MSRV 1.85

[Unreleased]: https://github.com/Cliftonz/jarvy/compare/v0.8.1...HEAD
[v0.8.2]: https://github.com/Cliftonz/jarvy/releases/tag/v0.8.2
[v0.8.1]: https://github.com/Cliftonz/jarvy/releases/tag/v0.8.1
[v0.8.0]: https://github.com/Cliftonz/jarvy/releases/tag/v0.8.0
[v0.7.0]: https://github.com/Cliftonz/jarvy/releases/tag/v0.7.0
[v0.5.2]: https://github.com/Cliftonz/jarvy/releases/tag/v0.5.2
[v0.5.1]: https://github.com/Cliftonz/jarvy/releases/tag/v0.5.1
[v0.3.0]: https://github.com/Cliftonz/Jarvy/releases/tag/v0.3.0
[v0.2.2]: https://github.com/Cliftonz/jarvy/releases/tag/v0.2.2
[v0.2.1]: https://github.com/Cliftonz/jarvy/releases/tag/v0.2.1
[v0.2.0]: https://github.com/Cliftonz/jarvy/releases/tag/v0.2.0
[v0.1.0]: https://github.com/Cliftonz/jarvy/releases/tag/v0.1.0
[v0.0.5]: https://github.com/Cliftonz/jarvy/releases/tag/v0.0.5
[v0.0.4]: https://github.com/Cliftonz/jarvy/releases/tag/v0.0.4
[v0.0.3]: https://github.com/Cliftonz/jarvy/releases/tag/v0.0.3
[v0.0.2]: https://github.com/Cliftonz/jarvy/releases/tag/v0.0.2
[v0.0.1]: https://github.com/Cliftonz/jarvy/releases/tag/v0.0.1
