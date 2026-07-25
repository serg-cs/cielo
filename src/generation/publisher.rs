use std::{
    env, fs,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use tempfile::{Builder, TempDir};
use tracing::warn;

use super::GENERATOR_IDENTITY;

const APP_GENERATOR_DECLARATION: &str = r#"<meta name="generator" content="cielo">"#;
const AEMET_SOURCE_NAME: &str = "AEMET";
const AEMET_SOURCE_URL: &str = "https://opendata.aemet.es/";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum OutputKind {
    App,
    Data,
}

impl OutputKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::App => "app",
            Self::Data => "data",
        }
    }
}

#[derive(Deserialize)]
struct CatalogIdentity {
    generator: String,
    source: SourceIdentity,
    #[serde(rename = "municipalities")]
    _municipalities: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct SourceIdentity {
    name: String,
    url: String,
}

pub(super) fn create_staging_directory(
    output_directory: &Path,
    output_kind: OutputKind,
) -> Result<(PathBuf, TempDir)> {
    let output_directory = validate_output_directory(output_directory, output_kind)?;
    let parent = output_directory
        .parent()
        .context("output directory does not have a parent")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create output parent {}", parent.display()))?;
    let staging = Builder::new()
        .prefix(".cielo-staging-")
        .tempdir_in(parent)
        .with_context(|| format!("failed to create staging directory in {}", parent.display()))?;
    Ok((output_directory, staging))
}

pub(super) fn publish_staging_directory(
    staging: &TempDir,
    output_directory: &Path,
    output_kind: OutputKind,
) -> Result<()> {
    // Revalidate immediately before publishing to prevent concurrent path substitution.
    validate_existing_output(output_directory, output_kind)?;
    if !output_directory.exists() {
        return fs::rename(staging.path(), output_directory).with_context(|| {
            format!(
                "failed to publish generated directory {}",
                output_directory.display()
            )
        });
    }

    let parent = output_directory
        .parent()
        .context("output directory does not have a parent")?;
    let backup = Builder::new()
        .prefix(".cielo-backup-")
        .tempdir_in(parent)
        .with_context(|| format!("failed to create backup directory in {}", parent.display()))?;
    let previous_output = backup.path().join("output");
    fs::rename(output_directory, &previous_output).with_context(|| {
        format!(
            "failed to move previous generated directory {}",
            output_directory.display()
        )
    })?;

    if let Err(publish_error) = fs::rename(staging.path(), output_directory) {
        let restore_result = fs::rename(&previous_output, output_directory);
        return match restore_result {
            Ok(()) => Err(publish_error).with_context(|| {
                format!(
                    "failed to publish generated directory {}; previous output restored",
                    output_directory.display()
                )
            }),
            Err(restore_error) => {
                let backup_path = backup.keep();
                bail!(
                    "failed to publish generated directory {} ({publish_error}); \
                     also failed to restore the previous output ({restore_error}); \
                     previous files remain at {}",
                    output_directory.display(),
                    backup_path.join("output").display()
                )
            }
        };
    }

    if let Err(error) = backup.close() {
        warn!(%error, "failed to remove previous generated output backup");
    }
    Ok(())
}

fn validate_output_directory(path: &Path, output_kind: OutputKind) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        bail!("output directory cannot be empty");
    }
    if path
        .components()
        .any(|component| component == Component::ParentDir)
    {
        bail!("output directory cannot contain '..'");
    }

    let current_directory = env::current_dir().context("failed to determine current directory")?;
    let absolute = if path.is_absolute() {
        clean_path(path)
    } else {
        clean_path(&current_directory.join(path))
    };
    if absolute == clean_path(&current_directory) || absolute.parent().is_none() {
        bail!("output directory must not be the current directory or filesystem root");
    }
    if absolute.file_name().is_none() {
        bail!("output directory must have a final path component");
    }

    validate_existing_output(&absolute, output_kind)?;
    Ok(absolute)
}

fn validate_existing_output(path: &Path, output_kind: OutputKind) -> Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to inspect output directory {}", path.display()));
        }
    };
    if metadata.file_type().is_symlink() {
        bail!(
            "output directory must not be a symbolic link: {}",
            path.display()
        );
    }
    if !metadata.is_dir() {
        bail!("output path is not a directory: {}", path.display());
    }
    if fs::read_dir(path)
        .with_context(|| format!("failed to read output directory {}", path.display()))?
        .next()
        .is_none()
    {
        return Ok(());
    }

    let recognized = match output_kind {
        OutputKind::App => is_generated_app(path)?,
        OutputKind::Data => is_generated_data(path)?,
    };
    if !recognized {
        bail!(
            "refusing to replace non-empty directory {} because it is not recognized as Cielo {} output",
            path.display(),
            output_kind.as_str()
        );
    }
    Ok(())
}

fn is_generated_app(path: &Path) -> Result<bool> {
    let Some(index) = read_optional_file(&path.join("index.html"))? else {
        return Ok(false);
    };
    Ok(index
        .windows(APP_GENERATOR_DECLARATION.len())
        .any(|window| window == APP_GENERATOR_DECLARATION.as_bytes()))
}

fn is_generated_data(path: &Path) -> Result<bool> {
    let Some(catalog) = read_optional_file(&path.join("municipalities.json"))? else {
        return Ok(false);
    };
    let Ok(identity) = serde_json::from_slice::<CatalogIdentity>(&catalog) else {
        return Ok(false);
    };
    Ok(identity.generator == GENERATOR_IDENTITY
        && identity.source.name == AEMET_SOURCE_NAME
        && identity.source.url == AEMET_SOURCE_URL)
}

fn read_optional_file(path: &Path) -> Result<Option<Vec<u8>>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to inspect generated file {}", path.display()));
        }
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Ok(None);
    }

    fs::read(path)
        .map(Some)
        .with_context(|| format!("failed to read generated file {}", path.display()))
}

fn clean_path(path: &Path) -> PathBuf {
    path.components()
        .filter(|component| *component != Component::CurDir)
        .collect()
}
