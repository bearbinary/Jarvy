//! Binary installer for direct download updates
//!
//! Downloads and installs Jarvy binaries directly from GitHub releases
//! when package manager updates are not available.

#![allow(dead_code)] // Public API for binary installation

use crate::update::method::UpdateError;
use crate::update::release::{GitHubRelease, ReleaseAsset};
use crate::update::rollback::RollbackManager;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

/// Binary installer for direct updates
pub struct BinaryInstaller {
    /// Backup directory for rollback support
    backup_dir: PathBuf,
    /// Staging directory for downloads
    staging_dir: PathBuf,
}

impl BinaryInstaller {
    /// Create a new binary installer
    pub fn new() -> io::Result<Self> {
        let backup_dir = crate::paths::backup_dir().map_err(io::Error::other)?;
        let staging_dir = crate::paths::staging_dir().map_err(io::Error::other)?;

        // Tighten permissions on the staging + backup dirs.
        // Staging holds the candidate binary post-download but pre-verify
        // — between `download_asset` and `verify_sigstore_signature` there
        // is a TOCTOU window where another local user could swap the file
        // if the dir is world-readable. Backup holds the pre-update binary
        // (smaller risk but same class).
        crate::paths::ensure_dir_0700(&staging_dir)?;
        crate::paths::ensure_dir_0700(&backup_dir)?;

        Ok(Self {
            backup_dir,
            staging_dir,
        })
    }

    /// Install a release via direct binary download.
    ///
    /// Defaults to fail-closed Sigstore signature verification. Pass
    /// `allow_unsigned = true` only when the operator has explicitly opted
    /// into unsigned updates via the `--allow-unsigned` CLI flag or
    /// `JARVY_ALLOW_UNSIGNED_UPDATE=1` env var.
    pub fn install(&self, release: &GitHubRelease) -> Result<InstallResult, UpdateError> {
        self.install_with_options(release, false)
    }

    /// Install a release with explicit signature-policy control.
    pub fn install_with_options(
        &self,
        release: &GitHubRelease,
        allow_unsigned: bool,
    ) -> Result<InstallResult, UpdateError> {
        // Get current binary path
        let current_exe = std::env::current_exe().map_err(|e| {
            UpdateError::InstallationFailed(format!("Cannot find current exe: {}", e))
        })?;

        // Find the appropriate asset for this platform
        let asset = release.asset_for_platform().ok_or_else(|| {
            UpdateError::DownloadFailed("No binary for this platform".to_string())
        })?;

        println!(
            "Downloading jarvy v{} for {}...",
            release.version(),
            crate::update::release::get_current_target()
        );

        // Download the binary archive
        let archive_path = self.download_asset(asset)?;

        // Verify checksum if available
        if let Some(checksum_asset) = release.checksum_asset() {
            println!("Verifying checksum...");
            let checksums = self.download_checksums(checksum_asset)?;
            self.verify_archive_checksum(&archive_path, &asset.name, &checksums)?;
        }

        // Download Sigstore signature companions before verification.
        //
        // `verify_sigstore_signature` looks for `<archive>.sig` and
        // `<archive>.pem` next to the archive on disk. They are NOT in the
        // archive — they're separate GitHub release assets. Previously the
        // installer never fetched them, so verification always returned
        // `SignatureFilesMissing` and `--allow-unsigned` rubber-stamped the
        // install (security review F-4). Now we fetch both before calling
        // `verify_sigstore_signature`, and missing companion assets are a
        // hard error rather than silent fallthrough.
        let allow_unsigned = allow_unsigned || super::signature::unsigned_override_from_env();
        if !allow_unsigned {
            for ext in ["sig", "pem"] {
                let companion = release.cosign_companion(&asset.name, ext).ok_or_else(|| {
                    UpdateError::InstallationFailed(format!(
                        "Sigstore companion `{}.{ext}` not found in release assets; \
                         refuse to install. Set JARVY_ALLOW_UNSIGNED_UPDATE=1 only if \
                         you have audited this release out-of-band.",
                        asset.name,
                    ))
                })?;
                self.download_asset(companion)?;
            }
        }

        let outcome = super::signature::verify_sigstore_signature(&archive_path)
            .map_err(|e| UpdateError::InstallationFailed(format!("signature error: {e}")))?;
        if let Err(reason) =
            super::signature::signature_outcome_is_acceptable(&outcome, allow_unsigned)
        {
            eprintln!("\x1b[31m[SECURITY]\x1b[0m {reason}");
            return Err(UpdateError::InstallationFailed(reason));
        }
        if matches!(outcome, super::signature::SignatureOutcome::Verified) {
            println!(
                "\x1b[32m[VERIFIED]\x1b[0m Sigstore signature verified for {}",
                archive_path.display()
            );
        }

        // Extract the binary
        println!("Extracting binary...");
        let binary_path = self.extract_binary(&archive_path)?;

        // Backup current binary
        println!("Backing up current version...");
        let backup_path = self.backup_current(&current_exe)?;

        // Replace with new binary
        println!("Installing update...");
        self.replace_binary(&binary_path, &current_exe)?;

        // Verify new binary works
        if !self.verify_installation(&current_exe)? {
            // Rollback on failure
            eprintln!("Installation verification failed, rolling back...");
            self.restore_backup(&backup_path, &current_exe)?;
            return Err(UpdateError::InstallationFailed(
                "New binary verification failed".to_string(),
            ));
        }

        // Record rollback info
        RollbackManager::record_update(
            crate::version::JARVY_VERSION,
            release.version(),
            &backup_path,
        )?;

        // Cleanup staging
        let _ = fs::remove_dir_all(&self.staging_dir);
        let _ = fs::create_dir_all(&self.staging_dir);

        Ok(InstallResult {
            previous_version: crate::version::JARVY_VERSION.to_string(),
            new_version: release.version().to_string(),
            backup_path,
        })
    }

    /// Download a release asset
    fn download_asset(&self, asset: &ReleaseAsset) -> Result<PathBuf, UpdateError> {
        let target_path = self.staging_dir.join(&asset.name);

        // Release-asset URLs 302 to release-assets.githubusercontent.com;
        // the shared no-redirect agent turns that into an empty-body
        // "success" (v0.6.0-rc.1 sev-1). Use the redirect-following agent
        // and refuse anything but a final 200 so a policy regression fails
        // loudly instead of staging a 0-byte archive.
        let response = crate::net::github_release_download_agent()
            .get(&asset.browser_download_url)
            .header("User-Agent", crate::net::USER_AGENT)
            .call()
            .map_err(|e| UpdateError::DownloadFailed(e.to_string()))?;
        if response.status() != 200 {
            return Err(UpdateError::DownloadFailed(format!(
                "HTTP {} for {}",
                response.status(),
                asset.name
            )));
        }

        // BufWriter so the ~8KB chunks ureq emits don't translate to
        // ~3,800 raw write(2) syscalls for a 30 MB tarball.
        let file = File::create(&target_path)
            .map_err(|e| UpdateError::DownloadFailed(format!("Cannot create file: {}", e)))?;
        let mut writer = std::io::BufWriter::with_capacity(64 * 1024, file);

        let mut body = response.into_body();
        let mut reader = body.as_reader();
        io::copy(&mut reader, &mut writer)
            .map_err(|e| UpdateError::DownloadFailed(format!("Download failed: {}", e)))?;
        writer
            .flush()
            .map_err(|e| UpdateError::DownloadFailed(format!("Flush failed: {}", e)))?;

        Ok(target_path)
    }

    /// Download checksums file
    fn download_checksums(&self, asset: &ReleaseAsset) -> Result<String, UpdateError> {
        // Same redirect + status story as `download_asset`.
        let response = crate::net::github_release_download_agent()
            .get(&asset.browser_download_url)
            .header("User-Agent", crate::net::USER_AGENT)
            .call()
            .map_err(|e| UpdateError::DownloadFailed(e.to_string()))?;
        if response.status() != 200 {
            return Err(UpdateError::DownloadFailed(format!(
                "HTTP {} for {}",
                response.status(),
                asset.name
            )));
        }

        let mut body_content = String::new();
        let mut body = response.into_body();
        body.as_reader()
            .read_to_string(&mut body_content)
            .map_err(|e| UpdateError::DownloadFailed(e.to_string()))?;

        Ok(body_content)
    }

    /// Verify archive checksum against downloaded checksums
    fn verify_archive_checksum(
        &self,
        archive_path: &Path,
        archive_name: &str,
        checksums: &str,
    ) -> Result<(), UpdateError> {
        // Find the line with our archive
        let expected = checksums
            .lines()
            .find(|line| line.contains(archive_name))
            .and_then(|line| line.split_whitespace().next())
            .ok_or(UpdateError::ChecksumMismatch)?;

        // Calculate actual checksum
        let actual = calculate_file_checksum(archive_path)?;

        if actual.to_lowercase() != expected.to_lowercase() {
            return Err(UpdateError::ChecksumMismatch);
        }

        Ok(())
    }

    /// Extract binary from archive
    fn extract_binary(&self, archive_path: &Path) -> Result<PathBuf, UpdateError> {
        let archive_name = archive_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        let extract_dir = self.staging_dir.join("extract");
        fs::create_dir_all(&extract_dir)
            .map_err(|e| UpdateError::InstallationFailed(e.to_string()))?;

        if archive_name.ends_with(".tar.gz") || archive_name.ends_with(".tgz") {
            self.extract_tar_gz(archive_path, &extract_dir)?;
        } else if archive_name.ends_with(".zip") {
            self.extract_zip(archive_path, &extract_dir)?;
        } else {
            return Err(UpdateError::InstallationFailed(
                "Unknown archive format".to_string(),
            ));
        }

        // Find the jarvy binary in extracted files
        let binary_name = if cfg!(windows) { "jarvy.exe" } else { "jarvy" };
        self.find_binary(&extract_dir, binary_name)
    }

    /// Extract tar.gz archive.
    ///
    /// Uses the system `tar` with flags that prevent classic path-traversal
    /// vectors (`--no-same-owner`, no absolute paths, no symlink writes
    /// outside `dest`). Each entry's resolved path is also re-validated to
    /// be inside `dest` post-extract to defend against older tar versions
    /// that don't honor every flag.
    fn extract_tar_gz(&self, archive: &Path, dest: &Path) -> Result<(), UpdateError> {
        use std::process::Command;

        let dest_canon = fs::canonicalize(dest).map_err(|e| {
            UpdateError::InstallationFailed(format!("canonicalize destination: {e}"))
        })?;

        // Args that are accepted by both GNU tar and BSD tar:
        //   -x   extract
        //   -z   gzip
        //   -f   from file
        //   --no-same-owner   never restore archived uid/gid
        //   --no-same-permissions  never restore archived mode bits
        // Note: BSD tar interprets -P inverted compared to GNU; we instead
        // post-validate that no extracted path escaped `dest`.
        let status = Command::new("tar")
            .args([
                "-xzf",
                archive.to_string_lossy().as_ref(),
                "-C",
                dest_canon.to_string_lossy().as_ref(),
                "--no-same-owner",
                "--no-same-permissions",
            ])
            .status()
            .map_err(|e| {
                UpdateError::InstallationFailed(format!("tar extraction failed: {}", e))
            })?;

        if !status.success() {
            return Err(UpdateError::InstallationFailed(
                "tar extraction returned non-zero".to_string(),
            ));
        }

        // Walk the extracted tree and refuse to install if any entry resolved
        // outside the destination — catches symlink/PaxHeader escapes that
        // older GNU tar versions are vulnerable to.
        if let Err(e) = verify_no_tar_escape(&dest_canon) {
            return Err(UpdateError::InstallationFailed(format!(
                "tar archive contained a path that escaped {}: {}",
                dest_canon.display(),
                e
            )));
        }

        Ok(())
    }

    /// Extract zip archive.
    ///
    /// Mirrors the post-extract canonicalize-and-contain check the tar path
    /// performs (security review F-14): the `zip` crate filters basic `..`
    /// segments but does not consistently catch symlink targets that
    /// resolve outside the destination. On Windows where Jarvy ships a
    /// `.zip` instead of `.tar.gz`, this is the only extraction path and
    /// must enforce the same containment.
    fn extract_zip(&self, archive: &Path, dest: &Path) -> Result<(), UpdateError> {
        let file =
            File::open(archive).map_err(|e| UpdateError::InstallationFailed(e.to_string()))?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| UpdateError::InstallationFailed(e.to_string()))?;

        archive
            .extract(dest)
            .map_err(|e| UpdateError::InstallationFailed(e.to_string()))?;

        let dest_canon = fs::canonicalize(dest).map_err(|e| {
            UpdateError::InstallationFailed(format!("could not canonicalize zip extract dir: {e}"))
        })?;
        if let Err(e) = verify_no_tar_escape(&dest_canon) {
            return Err(UpdateError::InstallationFailed(format!(
                "zip archive contained a path that escaped {}: {}",
                dest_canon.display(),
                e
            )));
        }

        Ok(())
    }

    /// Find binary in extracted directory
    fn find_binary(&self, dir: &Path, name: &str) -> Result<PathBuf, UpdateError> {
        // Try direct match first
        let direct = dir.join(name);
        if direct.exists() {
            return Ok(direct);
        }

        // Search recursively
        for entry in walkdir(dir).flatten() {
            if entry.file_name().to_string_lossy() == name {
                return Ok(entry.path().to_path_buf());
            }
        }

        Err(UpdateError::InstallationFailed(format!(
            "Binary {} not found in archive",
            name
        )))
    }

    /// Backup current binary
    fn backup_current(&self, current_exe: &Path) -> Result<PathBuf, UpdateError> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let backup_name = format!("jarvy-{}-{}", crate::version::JARVY_VERSION, timestamp);
        let backup_path = self.backup_dir.join(backup_name);

        fs::copy(current_exe, &backup_path)
            .map_err(|e| UpdateError::InstallationFailed(format!("Backup failed: {}", e)))?;

        Ok(backup_path)
    }

    /// Replace current binary with new one
    fn replace_binary(&self, new_binary: &Path, target: &Path) -> Result<(), UpdateError> {
        // Make new binary executable on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(new_binary)
                .map_err(|e| UpdateError::InstallationFailed(e.to_string()))?
                .permissions();
            perms.set_mode(0o755);
            fs::set_permissions(new_binary, perms)
                .map_err(|e| UpdateError::InstallationFailed(e.to_string()))?;
        }

        // Stage the new binary as a `<target>.new` sibling before swapping.
        // `new_binary` lives under the staging dir (often tmpfs), so a
        // straight rename into the install dir can fail with EXDEV. Copying
        // beside `target` first keeps the swap itself a same-filesystem
        // rename. `fs::copy` carries over the executable bit set above, so
        // no extra chmod is needed on `staged`.
        let staged = stage_beside(new_binary, target)
            .map_err(|e| UpdateError::InstallationFailed(e.to_string()))?;

        // Two-rename swap (`swap_into_place`): the running binary is moved
        // aside to `<target>.old` and the new one renamed into place. On
        // Windows a running executable can be renamed but not deleted or
        // overwritten, which is why the old binary is left as a sibling on
        // Windows rather than replaced in place. The rename can still
        // transiently fail with "Access is denied" while Defender / an
        // indexer holds a handle on the freshly extracted binary (issue
        // #63). The original 3 attempts at 500ms and 1s still lost to a
        // Defender scan on the v0.6.4-rc.1 release-paths run, so: 5
        // attempts, exponential 500ms to 4s (about 7.5s worst-case total),
        // and each retry prints; silent retries made it impossible to tell
        // from CI logs whether the backoff fired at all. Unix renames
        // don't contend with scanners; one attempt.
        let attempts: u32 = if cfg!(windows) { 5 } else { 1 };
        let mut last_err = None;
        for attempt in 0..attempts {
            if attempt > 0 {
                let delay = std::time::Duration::from_millis(500 * (1u64 << (attempt - 1)));
                eprintln!(
                    "Binary swap blocked (attempt {}/{}); retrying in {:.1}s — antivirus may be scanning the new binary...",
                    attempt,
                    attempts,
                    delay.as_secs_f32()
                );
                std::thread::sleep(delay);
            }
            match swap_into_place(&staged, target) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    last_err = Some(e);
                    if swap_is_stranded(target) {
                        break;
                    }
                }
            }
        }
        let _ = fs::remove_file(&staged);
        Err(UpdateError::InstallationFailed(
            last_err.map(|e| e.to_string()).unwrap_or_default(),
        ))
    }

    /// Verify the new installation works
    fn verify_installation(&self, exe_path: &Path) -> Result<bool, UpdateError> {
        use std::process::Command;

        let output = Command::new(exe_path)
            .arg("--version")
            .output()
            .map_err(|e| UpdateError::InstallationFailed(e.to_string()))?;

        Ok(output.status.success())
    }

    /// Restore backup after failed installation
    fn restore_backup(&self, backup: &Path, target: &Path) -> Result<(), UpdateError> {
        // Same ETXTBSY hazard as `Rollback::restore_backup`: the restore
        // target is the running executable, so `fs::copy` into it fails on
        // Linux. Two-rename swap via `swap_into_place`, matching
        // `replace_binary`. Copies the backup file; the original stays in
        // place.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(backup)
                .map_err(|e| UpdateError::RollbackFailed(e.to_string()))?
                .permissions();
            perms.set_mode(0o755);
            fs::set_permissions(backup, perms)
                .map_err(|e| UpdateError::RollbackFailed(e.to_string()))?;
        }

        let staged =
            stage_beside(backup, target).map_err(|e| UpdateError::RollbackFailed(e.to_string()))?;

        if let Err(e) = swap_into_place(&staged, target) {
            let _ = fs::remove_file(&staged);
            return Err(UpdateError::RollbackFailed(format!(
                "Restore failed: {}",
                e
            )));
        }

        Ok(())
    }
}

/// The previous `target` is moved to a `<target>.old` sibling; unix removes
/// it best-effort once the new binary is in place, Windows keeps it because
/// a running executable cannot be unlinked there. Two renames instead of a
/// delete-then-rename: a running executable can be renamed on every
/// platform, but Windows refuses to delete or overwrite it, and Linux
/// refuses to write into it (ETXTBSY). A stale `.old` from an earlier swap
/// is replaced.
pub(crate) fn swap_into_place(source: &Path, target: &Path) -> io::Result<()> {
    let old = sibling_with_suffix(target, ".old");
    fs::rename(target, &old)?;
    match fs::rename(source, target) {
        Ok(()) => {
            #[cfg(unix)]
            let _ = fs::remove_file(&old);
            Ok(())
        }
        Err(cause) => match fs::rename(&old, target) {
            Ok(()) => Err(cause),
            Err(_) => Err(stranded_error(cause, &old, target)),
        },
    }
}

/// Keeps the eventual `swap_into_place` a same-filesystem rename even when
/// `source` lives elsewhere (tmpfs staging dir, or the backup dir).
pub(crate) fn stage_beside(source: &Path, target: &Path) -> io::Result<PathBuf> {
    let staged = sibling_with_suffix(target, ".new");
    fs::copy(source, &staged)?;
    Ok(staged)
}

fn swap_is_stranded(target: &Path) -> bool {
    !target.exists()
}

fn sibling_with_suffix(target: &Path, suffix: &str) -> PathBuf {
    let mut name = target
        .file_name()
        .expect("update target is a file path")
        .to_os_string();
    name.push(suffix);
    target.with_file_name(name)
}

fn stranded_error(cause: io::Error, old: &Path, target: &Path) -> io::Error {
    io::Error::new(
        cause.kind(),
        format!(
            "failed to install new binary: {cause}; the previous binary is at {} and could not be moved back, rename it to {} manually",
            old.display(),
            target.display()
        ),
    )
}

/// Walk every entry under `dest_canon` and verify the canonicalized path is
/// still contained in `dest_canon`. Refuses symlinks pointing outside the
/// extraction root. Errors describe the offending path.
fn verify_no_tar_escape(dest_canon: &Path) -> Result<(), String> {
    // Inline shallow recursive walk; we don't want a new dependency.
    fn walk(root: &Path, dir: &Path) -> Result<(), String> {
        let entries = fs::read_dir(dir).map_err(|e| format!("read_dir failed: {e}"))?;
        for entry in entries.flatten() {
            let path = entry.path();
            let metadata = match entry.metadata() {
                Ok(m) => m,
                Err(e) => return Err(format!("metadata failed for {}: {e}", path.display())),
            };
            // For symlinks, canonicalize follows the link target; ensure the
            // target resolves inside `root`.
            let resolved = fs::canonicalize(&path)
                .map_err(|e| format!("canonicalize failed for {}: {e}", path.display()))?;
            if !resolved.starts_with(root) {
                return Err(format!(
                    "path {} resolves to {} which is outside {}",
                    path.display(),
                    resolved.display(),
                    root.display()
                ));
            }
            if metadata.is_dir() && !metadata.is_symlink() {
                walk(root, &path)?;
            }
        }
        Ok(())
    }
    walk(dest_canon, dest_canon)
}

impl Default for BinaryInstaller {
    fn default() -> Self {
        match Self::new() {
            Ok(installer) => installer,
            Err(e) => {
                eprintln!("Warning: failed to create BinaryInstaller: {e}");
                // Provide a fallback using temp directories
                Self {
                    backup_dir: std::env::temp_dir().join("jarvy-backup"),
                    staging_dir: std::env::temp_dir().join("jarvy-staging"),
                }
            }
        }
    }
}

/// Result of a successful installation
#[derive(Debug)]
pub struct InstallResult {
    pub previous_version: String,
    pub new_version: String,
    pub backup_path: PathBuf,
}

/// Calculate SHA256 checksum of a file
fn calculate_file_checksum(path: &Path) -> Result<String, UpdateError> {
    let mut file = File::open(path).map_err(|e| UpdateError::InstallationFailed(e.to_string()))?;

    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];

    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|e| UpdateError::InstallationFailed(e.to_string()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }

    Ok(hex::encode(hasher.finalize()))
}

/// Simple directory walker
fn walkdir(dir: &Path) -> impl Iterator<Item = io::Result<fs::DirEntry>> {
    let mut stack = vec![dir.to_path_buf()];
    std::iter::from_fn(move || {
        while let Some(current) = stack.pop() {
            if let Ok(entries) = fs::read_dir(&current) {
                for entry in entries.flatten() {
                    if entry.path().is_dir() {
                        stack.push(entry.path());
                    } else {
                        return Some(Ok(entry));
                    }
                }
            }
        }
        None
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_calculate_checksum() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");

        let mut file = File::create(&file_path).unwrap();
        file.write_all(b"hello world").unwrap();
        drop(file);

        let checksum = calculate_file_checksum(&file_path).unwrap();
        // SHA256 of "hello world"
        assert_eq!(
            checksum,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn test_installer_creation() {
        // Just verify installer can be created
        let installer = BinaryInstaller::new();
        assert!(installer.is_ok());
    }

    #[test]
    fn replace_binary_stages_new_binary_before_swap() {
        let installer = BinaryInstaller::new().unwrap();
        let source_dir = TempDir::new().unwrap();
        let target_dir = TempDir::new().unwrap();
        let new_binary = source_dir.path().join("jarvy.new");
        let target = target_dir.path().join("jarvy");
        fs::write(&new_binary, b"new").unwrap();
        fs::write(&target, b"old").unwrap();

        installer.replace_binary(&new_binary, &target).unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"new");
        assert!(
            new_binary.exists(),
            "staging should copy the source, not consume it"
        );
        assert!(!target_dir.path().join("jarvy.new").exists());
        #[cfg(unix)]
        assert!(!target_dir.path().join("jarvy.old").exists());
    }

    #[test]
    fn replace_binary_deletes_staged_new_when_swap_gives_up() {
        let installer = BinaryInstaller::new().unwrap();
        let source_dir = TempDir::new().unwrap();
        let target_dir = TempDir::new().unwrap();
        let new_binary = source_dir.path().join("jarvy.new");
        let target = target_dir.path().join("jarvy");
        fs::write(&new_binary, b"new").unwrap();

        assert!(installer.replace_binary(&new_binary, &target).is_err());

        assert!(!target_dir.path().join("jarvy.new").exists());
    }

    #[test]
    fn restore_backup_stages_backup_before_swap() {
        let installer = BinaryInstaller::new().unwrap();
        let backup_dir = TempDir::new().unwrap();
        let target_dir = TempDir::new().unwrap();
        let backup = backup_dir.path().join("jarvy-backup");
        let target = target_dir.path().join("jarvy");
        fs::write(&backup, b"backup").unwrap();
        fs::write(&target, b"current").unwrap();

        installer.restore_backup(&backup, &target).unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"backup");
        assert!(!target_dir.path().join("jarvy.new").exists());
        assert!(
            backup.exists(),
            "restore should copy the backup, not consume it"
        );
    }

    #[test]
    fn restore_backup_deletes_staged_new_when_swap_fails() {
        let installer = BinaryInstaller::new().unwrap();
        let backup_dir = TempDir::new().unwrap();
        let target_dir = TempDir::new().unwrap();
        let backup = backup_dir.path().join("jarvy-backup");
        let target = target_dir.path().join("jarvy");
        fs::write(&backup, b"backup").unwrap();

        assert!(installer.restore_backup(&backup, &target).is_err());

        assert!(!target_dir.path().join("jarvy.new").exists());
    }

    #[test]
    fn swap_into_place_moves_new_binary_and_keeps_old_as_sibling() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("jarvy.exe");
        let source = dir.path().join("jarvy.new");
        fs::write(&target, b"old").unwrap();
        fs::write(&source, b"new").unwrap();

        swap_into_place(&source, &target).unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"new");
        #[cfg(windows)]
        assert_eq!(fs::read(dir.path().join("jarvy.exe.old")).unwrap(), b"old");
        assert!(!source.exists());
    }

    #[test]
    fn swap_into_place_replaces_stale_old_sibling() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("jarvy");
        let source = dir.path().join("jarvy.new");
        let old_sibling = sibling_with_suffix(&target, ".old");
        fs::write(&target, b"old").unwrap();
        fs::write(&source, b"new").unwrap();
        fs::write(&old_sibling, b"stale").unwrap();

        swap_into_place(&source, &target).unwrap();

        #[cfg(windows)]
        assert_eq!(fs::read(&old_sibling).unwrap(), b"old");
        #[cfg(unix)]
        assert!(!old_sibling.exists());
    }

    #[cfg(unix)]
    #[test]
    fn swap_into_place_deletes_old_sibling_on_unix_after_success() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("jarvy");
        let source = dir.path().join("jarvy.new");
        fs::write(&target, b"old").unwrap();
        fs::write(&source, b"new").unwrap();

        swap_into_place(&source, &target).unwrap();

        assert!(!sibling_with_suffix(&target, ".old").exists());
    }

    #[test]
    fn swap_is_stranded_true_when_target_missing() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("jarvy");

        assert!(swap_is_stranded(&target));
    }

    #[test]
    fn swap_is_stranded_false_when_target_present() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("jarvy");
        fs::write(&target, b"old").unwrap();

        assert!(!swap_is_stranded(&target));
    }

    #[test]
    fn swap_into_place_restores_target_when_source_missing() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("jarvy");
        let source = dir.path().join("jarvy.new");
        fs::write(&target, b"old").unwrap();

        assert!(swap_into_place(&source, &target).is_err());

        assert_eq!(fs::read(&target).unwrap(), b"old");
        assert!(!sibling_with_suffix(&target, ".old").exists());
    }

    #[test]
    fn stranded_error_names_old_path_and_target() {
        let cause = io::Error::new(io::ErrorKind::PermissionDenied, "access is denied");
        let old = Path::new("/opt/jarvy/jarvy.old");
        let target = Path::new("/opt/jarvy/jarvy");

        let err = stranded_error(cause, old, target);

        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        let msg = err.to_string();
        assert!(msg.contains("/opt/jarvy/jarvy.old"), "{msg}");
        assert!(
            msg.contains("rename it to /opt/jarvy/jarvy manually"),
            "{msg}"
        );
        assert!(msg.contains("access is denied"), "{msg}");
    }

    // ----- verify_no_tar_escape (round-2 QA F5).
    // The path-traversal verifier IS the supply-chain defense for
    // unpacked binaries. A regression that always returns Ok(()) would
    // not have been caught by any test before this.

    #[test]
    fn verify_no_tar_escape_accepts_contained_tree() {
        let dest = TempDir::new().unwrap();
        let canon = fs::canonicalize(dest.path()).unwrap();
        let inner = canon.join("subdir");
        fs::create_dir_all(&inner).unwrap();
        File::create(inner.join("payload.txt"))
            .unwrap()
            .write_all(b"x")
            .unwrap();
        verify_no_tar_escape(&canon).expect("contained tree must pass");
    }

    #[test]
    #[cfg(unix)]
    fn verify_no_tar_escape_rejects_symlink_to_outside() {
        // Build a tarball-like layout where a symlink inside the
        // extraction root points to /etc — the classic supply-chain
        // breakout. The verifier must reject the whole tree.
        let dest = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let outside_canon = fs::canonicalize(outside.path()).unwrap();
        let canon = fs::canonicalize(dest.path()).unwrap();

        // Create the target file outside, then symlink to it from inside.
        let target = outside_canon.join("captured.txt");
        File::create(&target).unwrap().write_all(b"x").unwrap();
        let symlink = canon.join("escaped");
        std::os::unix::fs::symlink(&target, &symlink).unwrap();

        let err = verify_no_tar_escape(&canon).expect_err("escape must be refused");
        assert!(
            err.contains("outside") || err.contains("resolves"),
            "got {err:?}"
        );
    }
}
