//! Rollback mechanism for failed updates
//!
//! Provides ability to restore previous version after a failed update.

#![allow(dead_code)] // Public API for update rollback

use crate::update::method::UpdateError;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Rollback information stored after an update
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackInfo {
    /// Previous version that was replaced
    pub previous_version: String,
    /// New version that was installed
    pub new_version: String,
    /// Path to the backup file
    pub backup_path: PathBuf,
    /// Timestamp when update was performed
    pub updated_at: u64,
}

impl RollbackInfo {
    /// Get rollback info file path (canonical resolver in `crate::paths`).
    fn path() -> Option<PathBuf> {
        crate::paths::rollback_info_json().ok()
    }

    /// Load rollback info from disk
    pub fn load() -> Option<Self> {
        let path = Self::path()?;
        let content = fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    /// Save rollback info to disk
    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::path().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "No home directory")
        })?;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let content = serde_json::to_string_pretty(self)?;
        fs::write(path, content)
    }

    /// Clear rollback info from disk
    pub fn clear() -> std::io::Result<()> {
        if let Some(path) = Self::path()
            && path.exists()
        {
            fs::remove_file(path)?;
        }
        Ok(())
    }
}

/// Rollback manager for handling update rollbacks
pub struct RollbackManager;

impl RollbackManager {
    /// Record an update for potential rollback
    pub fn record_update(
        previous_version: &str,
        new_version: &str,
        backup_path: &Path,
    ) -> Result<(), UpdateError> {
        let info = RollbackInfo {
            previous_version: previous_version.to_string(),
            new_version: new_version.to_string(),
            backup_path: backup_path.to_path_buf(),
            updated_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };

        info.save().map_err(|e| {
            UpdateError::RollbackFailed(format!("Cannot save rollback info: {}", e))
        })?;

        Ok(())
    }

    /// Execute a rollback to previous version
    pub fn rollback() -> Result<RollbackResult, UpdateError> {
        let info = RollbackInfo::load().ok_or_else(|| {
            UpdateError::RollbackFailed("No rollback information available".to_string())
        })?;

        // Verify backup exists
        if !info.backup_path.exists() {
            return Err(UpdateError::RollbackFailed(format!(
                "Backup file not found: {}",
                info.backup_path.display()
            )));
        }

        // Get current binary path
        let current_exe = std::env::current_exe()
            .map_err(|e| UpdateError::RollbackFailed(format!("Cannot find current exe: {}", e)))?;

        // Restore the backup
        Self::restore_backup(&info.backup_path, &current_exe)?;

        // Clear rollback info
        let _ = RollbackInfo::clear();

        Ok(RollbackResult {
            restored_version: info.previous_version,
            replaced_version: info.new_version,
        })
    }

    /// Check if rollback is available
    pub fn can_rollback() -> bool {
        RollbackInfo::load()
            .map(|info| info.backup_path.exists())
            .unwrap_or(false)
    }

    /// Get rollback information if available
    pub fn info() -> Option<RollbackInfo> {
        RollbackInfo::load().filter(|info| info.backup_path.exists())
    }

    /// Restore backup file to target path
    fn restore_backup(backup: &Path, target: &Path) -> Result<(), UpdateError> {
        // `fs::copy` wrote INTO the running executable, which Linux refuses
        // with ETXTBSY ("Text file busy") — the rollback target IS the
        // binary executing this code. Use the same two-rename swap as
        // `Installer::replace_binary`. Found by release-paths Path 4 on
        // v0.6.2 — the first run in which that path ever executed.
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

        let staged = crate::update::installer::stage_beside(backup, target)
            .map_err(|e| UpdateError::RollbackFailed(e.to_string()))?;

        if let Err(e) = crate::update::installer::swap_into_place(&staged, target) {
            let _ = fs::remove_file(&staged);
            return Err(UpdateError::RollbackFailed(format!(
                "Restore failed: {}",
                e
            )));
        }

        Ok(())
    }
}

/// Result of a successful rollback
#[derive(Debug)]
pub struct RollbackResult {
    /// Version that was restored
    pub restored_version: String,
    /// Version that was replaced
    pub replaced_version: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn restore_backup_stages_backup_before_swap() {
        let backup_dir = TempDir::new().unwrap();
        let target_dir = TempDir::new().unwrap();
        let backup = backup_dir.path().join("jarvy-backup");
        let target = target_dir.path().join("jarvy");
        fs::write(&backup, b"backup").unwrap();
        fs::write(&target, b"current").unwrap();

        RollbackManager::restore_backup(&backup, &target).unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"backup");
        assert!(!target_dir.path().join("jarvy.new").exists());
        assert!(
            backup.exists(),
            "restore should copy the backup, not consume it"
        );
    }

    #[test]
    fn restore_backup_deletes_staged_new_when_swap_fails() {
        let backup_dir = TempDir::new().unwrap();
        let target_dir = TempDir::new().unwrap();
        let backup = backup_dir.path().join("jarvy-backup");
        let target = target_dir.path().join("jarvy");
        fs::write(&backup, b"backup").unwrap();

        assert!(RollbackManager::restore_backup(&backup, &target).is_err());

        assert!(!target_dir.path().join("jarvy.new").exists());
    }

    #[test]
    fn test_rollback_info_serialization() {
        let info = RollbackInfo {
            previous_version: "1.0.0".to_string(),
            new_version: "1.1.0".to_string(),
            backup_path: PathBuf::from("/tmp/backup"),
            updated_at: 1234567890,
        };

        let json = serde_json::to_string(&info).unwrap();
        let parsed: RollbackInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.previous_version, "1.0.0");
        assert_eq!(parsed.new_version, "1.1.0");
        assert_eq!(parsed.updated_at, 1234567890);
    }

    #[test]
    fn test_can_rollback_no_info() {
        // Without any saved info, rollback should not be available
        // This test just verifies the function doesn't panic
        let _ = RollbackManager::can_rollback();
    }
}
