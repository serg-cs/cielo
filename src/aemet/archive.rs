use std::{
    collections::HashSet,
    io::{Cursor, Read},
};

use anyhow::{Context, Result, bail};
use flate2::read::GzDecoder;

const MAX_TOTAL_SIZE: u64 = 512 * 1024 * 1024;
const MAX_ENTRIES: usize = 20_000;

/// Largest document in a forecast or warnings archive.
pub(super) const MAX_DOCUMENT_ENTRY: u64 = 4 * 1024 * 1024;

/// One regular file inside an AEMET archive.
pub(super) struct ArchiveEntry {
    pub(super) path: String,
    pub(super) body: Vec<u8>,
}

/// Read every regular file in a tar or tar.gz archive.
///
/// Problems with the archive itself are errors. Directories are skipped and
/// duplicate paths are rejected.
pub(super) fn read_archive(
    bytes: &[u8],
    label: &str,
    max_entry_size: u64,
) -> Result<Vec<ArchiveEntry>> {
    let reader: Box<dyn Read + '_> = if bytes.starts_with(&[0x1f, 0x8b]) {
        Box::new(GzDecoder::new(Cursor::new(bytes)))
    } else {
        Box::new(Cursor::new(bytes))
    };
    let mut archive = tar::Archive::new(reader);
    let mut entries = Vec::new();
    let mut paths = HashSet::new();
    let mut total_size = 0_u64;

    for entry in archive
        .entries()
        .with_context(|| format!("invalid {label} archive"))?
    {
        let mut entry = entry.with_context(|| format!("invalid {label} archive entry"))?;
        let entry_type = entry.header().entry_type();
        if entry_type.is_dir() {
            continue;
        }
        if !entry_type.is_file() {
            bail!("{label} archive contains a non-file entry");
        }
        if entries.len() >= MAX_ENTRIES {
            bail!("{label} archive contains too many entries");
        }

        let path = entry
            .path()
            .with_context(|| format!("invalid {label} archive path"))?
            .to_str()
            .with_context(|| format!("{label} archive path is not valid UTF-8"))?
            .to_owned();
        if !paths.insert(path.clone()) {
            bail!("duplicate path in {label} archive: {path}");
        }
        let size = entry
            .header()
            .size()
            .with_context(|| format!("invalid {label} archive entry size"))?;
        if size > max_entry_size {
            bail!("{label} archive entry is too large: {path}");
        }
        total_size = total_size.saturating_add(size);
        if total_size > MAX_TOTAL_SIZE {
            bail!("{label} archive is too large when decompressed");
        }

        let mut body = Vec::with_capacity(usize::try_from(size).unwrap_or_default());
        entry
            .read_to_end(&mut body)
            .with_context(|| format!("failed to read {label} archive entry {path}"))?;
        entries.push(ArchiveEntry { path, body });
    }

    if entries.is_empty() {
        bail!("{label} archive is empty");
    }
    Ok(entries)
}

/// File name without directories.
pub(super) fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}
