use anyhow::{Context, Result, bail};
use std::{
    fs::{File, OpenOptions, TryLockError},
    path::Path,
};

#[derive(Debug)]
#[must_use = "retain the guard until all state operations finish"]
pub(crate) struct StateLock {
    _file: File,
}

impl StateLock {
    pub(crate) fn acquire(state: &Path) -> Result<Self> {
        // Never unlink this file: replacing its inode would split ownership between owners.
        let path = state.join("state.lock");
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options
            .open(&path)
            .with_context(|| format!("open state lock {}", path.display()))?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => bail!(
                "state lock held for {}; another owner is using {}",
                state.display(),
                path.display()
            ),
            Err(TryLockError::Error(error)) => Err(error).with_context(|| {
                format!("lock state {} using {}", state.display(), path.display())
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_owner_is_denied_without_mutation_and_drop_releases_the_lock() {
        let state = std::env::temp_dir().join(format!("state-lock-{}", rand::random::<u64>()));
        std::fs::create_dir(&state).unwrap();
        let database = state.join("game.db");
        std::fs::write(&database, b"must not be opened or migrated").unwrap();
        let first = StateLock::acquire(&state).unwrap();
        let error = StateLock::acquire(&state).unwrap_err().to_string();
        assert!(error.contains("state lock held"), "{error}");
        assert!(error.contains(state.to_str().unwrap()), "{error}");
        assert!(error.contains("state.lock"), "{error}");
        assert_eq!(
            std::fs::read(&database).unwrap(),
            b"must not be opened or migrated"
        );
        drop(first);
        assert!(state.join("state.lock").is_file());
        let next = StateLock::acquire(&state).unwrap();
        drop(next);
        std::fs::remove_dir_all(state).unwrap();
    }

    #[test]
    fn lock_open_failures_identify_the_exact_state_path() {
        let missing = std::env::temp_dir().join(format!("missing-state-{}", rand::random::<u64>()));
        let error = format!("{:#}", StateLock::acquire(&missing).unwrap_err());
        assert!(error.contains("open state lock"), "{error}");
        assert!(error.contains(missing.to_str().unwrap()), "{error}");
    }
}
