---
title: "Self-Updating - Jarvy"
description: "Update Jarvy itself via Homebrew, Cargo, apt, winget, or direct binary, with rollback and channel support."
---

# Self-Updating

Jarvy ships with a built-in `update` command that detects how it was installed and uses the matching channel. There is no shell-script wrapper to maintain — the binary updates itself.

## Quick Use

```bash
jarvy update                # Check + install latest
jarvy update check          # Check only, don't install
jarvy update --version 1.2.3
jarvy update --channel beta
jarvy update --rollback     # Revert to prior version
jarvy update history        # See past updates
```

## Install Methods Detected

Jarvy inspects its own binary path and environment to pick the right update path. No method? It falls back to a direct binary download.

| Detected | How updates run |
|----------|------------------|
| Homebrew (macOS/Linux) | `brew upgrade jarvy` |
| Cargo | `cargo install jarvy` |
| apt (Debian/Ubuntu) | `apt-get install --only-upgrade jarvy` |
| dnf (Fedora/RHEL) | `dnf upgrade jarvy` |
| pacman (Arch) | AUR helper invocation |
| winget (Windows) | `winget upgrade jarvy` |
| Chocolatey | `choco upgrade jarvy` |
| Scoop | `scoop update jarvy` |
| Binary | Direct GitHub Releases download into install dir |

## Channels

```toml
# ~/.jarvy/config.toml
[update]
enabled = true
channel = "stable"        # stable | beta | nightly
check_interval = "24h"
auto_install = "never"    # never | patch-only | patch-minor | all
show_notifications = true
```

| Channel | Accepts |
|---------|---------|
| `stable` | Only `vX.Y.Z` tags |
| `beta` | `vX.Y.Z`, `vX.Y.Z-rc.N`, `vX.Y.Z-beta.N` |
| `nightly` | Everything including `-alpha.N` |

Override per-run with `jarvy update --channel beta`.

## Auto-Install Policies

| Policy | Behavior |
|--------|----------|
| `never` | Always prompt before installing |
| `patch-only` | Auto-install patch bumps (`1.2.3 → 1.2.4`) |
| `patch-minor` | Auto-install patch + minor (`1.2.3 → 1.3.0`) |
| `all` | Auto-install everything including major bumps |

Major bumps with `auto_install = "all"` is rarely what you want — Jarvy still respects the breaking-change indicator from the release.

## Environment Variables

| Variable | Effect |
|----------|--------|
| `JARVY_UPDATE=0` | Disable update checks entirely |
| `JARVY_UPDATE_CHANNEL=beta` | Override channel |
| `JARVY_PINNED_VERSION=1.2.3` | Refuse to update past this version |

CI environments (`CI=true`) skip auto-update checks regardless of config.

## Rollback

Every successful update writes a backup of the previous binary to `~/.jarvy/backups/`. Roll back when a release breaks something:

```bash
jarvy update --rollback              # Most recent backup
jarvy update history                 # List backups with versions + timestamps
```

`RollbackManager` keeps the last 3 versions by default, prunes older ones automatically.

## Binary Verification

Direct-download updates verify SHA-256 checksums published with each GitHub release before swapping the binary. Tampered downloads are rejected and the existing binary is preserved.

## Pre-release Validation

Switching to `channel = "beta"` is the easiest way to help validate releases before they ship to stable. Issues filed against a pre-release tag with `release-blocker` or `regression` labels block its promotion. See [release-testing.md](release-testing.md) for the gate criteria.

## CLI Reference

```bash
jarvy update                  # Check + install
jarvy update check            # Check only
jarvy update --version X      # Pin a version
jarvy update --channel C      # Override channel
jarvy update --rollback       # Restore previous
jarvy update history          # Show update log
jarvy update config           # Show update config
jarvy update enable           # Enable auto-checks
jarvy update disable          # Disable auto-checks
```

## Module

- Source: `src/update/`
- Key types: `UpdateConfig`, `Channel`, `InstallMethod`, `UpdateChecker`, `BinaryInstaller`, `RollbackManager`
- Binary swap: in-house two-rename `swap_into_place` (old binary moved to `<target>.old`, new binary renamed into place; unix deletes `.old` afterwards, Windows keeps it); GitHub Releases REST for version discovery
