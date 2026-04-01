//! Workspace layout for a run: temp storage, optional manifest copy, audit path.

use std::path::{Path, PathBuf};

pub struct Sandbox {
    pub run_root: PathBuf,
    pub work_dir: PathBuf,
    pub temp_dir: PathBuf,
}

impl Sandbox {
    pub fn create(runs_base: &Path, run_id: &str) -> std::io::Result<Self> {
        let run_root = runs_base.join(run_id);
        let work_dir = run_root.join("workspace");
        let temp_dir = run_root.join("tmp");
        std::fs::create_dir_all(&work_dir)?;
        std::fs::create_dir_all(&temp_dir)?;
        Ok(Self {
            run_root,
            work_dir,
            temp_dir,
        })
    }

    pub fn copy_manifest(&self, manifest_path: &Path) -> std::io::Result<PathBuf> {
        let name = manifest_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("manifest.yaml");
        let dest = self.run_root.join(format!("manifest-used-{name}"));
        std::fs::copy(manifest_path, &dest)?;
        Ok(dest)
    }
}
