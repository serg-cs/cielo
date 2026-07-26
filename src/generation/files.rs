use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail};

#[derive(Default)]
pub(super) struct GeneratedFiles {
    files: BTreeMap<PathBuf, Vec<u8>>,
}

impl GeneratedFiles {
    pub(super) fn insert(
        &mut self,
        path: impl AsRef<Path>,
        bytes: impl Into<Vec<u8>>,
    ) -> Result<()> {
        let path = validate_relative_path(path.as_ref())?;
        if self.files.insert(path.clone(), bytes.into()).is_some() {
            bail!(
                "generated output contains duplicate path {}",
                path.display()
            );
        }
        Ok(())
    }

    pub(super) fn file_count(&self) -> usize {
        self.files.len()
    }

    pub(super) fn total_bytes(&self) -> usize {
        self.files.values().map(Vec::len).sum()
    }

    pub(super) fn contains(&self, path: impl AsRef<Path>) -> bool {
        self.files.contains_key(path.as_ref())
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = (&Path, &[u8])> {
        self.files
            .iter()
            .map(|(path, bytes)| (path.as_path(), bytes.as_slice()))
    }

    pub(super) fn write_to(&self, output_directory: &Path) -> Result<()> {
        for (relative_path, bytes) in &self.files {
            let path = output_directory.join(relative_path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("failed to create generated directory {}", parent.display())
                })?;
            }
            std::fs::write(&path, bytes)
                .with_context(|| format!("failed to write generated file {}", path.display()))?;
        }
        Ok(())
    }
}

fn validate_relative_path(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        bail!("generated file path must be a non-empty relative path");
    }
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!(
            "generated file path contains an unsupported component: {}",
            path.display()
        );
    }
    Ok(path.to_owned())
}
