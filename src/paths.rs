//! Path resolution for policy checks: cwd-relative paths, `..` normalization.
//!
//! Policy checks combine this with `std::fs::canonicalize` to follow symlinks. This is still
//! **not** an OS sandbox; bugs here can break policy.

use std::env;
use std::io;
use std::path::{Component, Path, PathBuf};

/// Reject empty paths.
pub fn resolve_user_path(path: &Path) -> io::Result<PathBuf> {
    if path.as_os_str().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path must not be empty",
        ));
    }
    let combined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()?.join(path)
    };
    normalize_dotdots(&combined)
}

/// Lexically normalize `.` and `..` after making the path absolute (does not touch symlinks).
fn normalize_dotdots(path: &Path) -> io::Result<PathBuf> {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Prefix(_) => out.push(comp.as_os_str()),
            Component::RootDir => out.push(comp.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                let _ = out.pop();
            }
            Component::Normal(s) => out.push(s),
        }
    }
    if out.as_os_str().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path normalized to empty",
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct CdGuard(PathBuf);
    impl Drop for CdGuard {
        fn drop(&mut self) {
            let _ = env::set_current_dir(&self.0);
        }
    }

    #[test]
    fn rejects_empty_path() {
        assert!(resolve_user_path(Path::new("")).is_err());
    }

    #[test]
    fn normalizes_dotdot_under_cwd() {
        let tmp = tempfile::tempdir().unwrap();
        let logs = tmp.path().join("logs");
        fs::create_dir_all(&logs).unwrap();
        let cwd = tmp.path().join("other");
        fs::create_dir_all(&cwd).unwrap();

        let old = env::current_dir().unwrap();
        let _guard = CdGuard(old);
        env::set_current_dir(&cwd).unwrap();
        let rel = Path::new("../logs/secret.log");
        let g = resolve_user_path(rel).unwrap();
        assert!(
            g.ends_with("logs/secret.log") || g.ends_with("logs\\secret.log"),
            "{g:?}"
        );
        assert!(!g.components().any(|c| c.as_os_str() == ".."));
    }
}
