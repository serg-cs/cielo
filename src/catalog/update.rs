//! `cielo catalog update`: refresh the catalog from AEMET.
//!
//! AEMET's values (coordinates, altitude, population, warning zone, the
//! beach list) are replaced. Curated municipality names are kept; a new
//! municipality gets AEMET's name in display form and is listed so its name
//! can be reviewed, since AEMET writes many names inverted or in a
//! co-official language.

use std::path::Path;

use anyhow::Result;

use super::{Beach, Catalog, Municipality};
use crate::aemet::AemetClient;

/// What an update changed, for the person running it.
#[derive(Debug, Default)]
pub(crate) struct CatalogChanges {
    pub(crate) municipalities: usize,
    pub(crate) beaches: usize,
    /// New municipalities: ID, display name given, AEMET's name.
    pub(crate) added: Vec<(String, String, String)>,
    /// Municipalities AEMET no longer lists: ID and name.
    pub(crate) removed: Vec<(String, String)>,
    pub(crate) beaches_added: Vec<String>,
    pub(crate) beaches_removed: Vec<String>,
    /// Beaches skipped because their municipality is not in AEMET's list.
    pub(crate) beaches_skipped: Vec<String>,
}

/// Refresh the catalog files in `directory` from AEMET.
pub(crate) async fn update_catalog(
    client: &AemetClient,
    directory: &Path,
) -> Result<CatalogChanges> {
    let current = Catalog::read(directory)?;
    let (master, beaches) = tokio::try_join!(client.municipality_list(), client.beach_list())?;
    let (catalog, changes) = merge(&current, master, beaches);
    catalog.write(directory)?;
    Ok(changes)
}

/// Combine the current catalog with AEMET's lists.
pub(super) fn merge(
    current: &Catalog,
    master: Vec<Municipality>,
    beaches: Vec<Beach>,
) -> (Catalog, CatalogChanges) {
    let mut changes = CatalogChanges::default();
    let mut catalog = Catalog {
        provinces: current.provinces.clone(),
        municipalities: std::collections::BTreeMap::new(),
        beaches: Vec::new(),
    };
    for mut municipality in master {
        if !catalog.provinces.contains_key(&municipality.id[..2]) {
            continue;
        }
        if let Some(existing) = current.municipalities.get(&municipality.id) {
            municipality.name.clone_from(&existing.name);
        } else {
            let aemet_name = std::mem::take(&mut municipality.name);
            municipality.name = display_name(&aemet_name);
            changes.added.push((
                municipality.id.clone(),
                municipality.name.clone(),
                aemet_name,
            ));
        }
        catalog
            .municipalities
            .insert(municipality.id.clone(), municipality);
    }
    for (id, municipality) in &current.municipalities {
        if !catalog.municipalities.contains_key(id) {
            changes
                .removed
                .push((id.clone(), municipality.name.clone()));
        }
    }
    for beach in beaches {
        if catalog.municipalities.contains_key(&beach.municipality_id) {
            catalog.beaches.push(beach);
        } else {
            changes
                .beaches_skipped
                .push(format!("{} {}", beach.id, beach.name));
        }
    }
    let old = current
        .beaches
        .iter()
        .map(|beach| beach.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let new = catalog
        .beaches
        .iter()
        .map(|beach| beach.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    changes.beaches_added = new.difference(&old).map(|id| (*id).to_owned()).collect();
    changes.beaches_removed = old.difference(&new).map(|id| (*id).to_owned()).collect();
    changes.municipalities = catalog.municipalities.len();
    changes.beaches = catalog.beaches.len();
    (catalog, changes)
}

/// AEMET's name in display form: the article moved to the front
/// (`Palmas de Gran Canaria, Las` → `Las Palmas de Gran Canaria`) and only
/// the first of two forms kept (`Alicante/Alacant` → `Alicante`).
pub(super) fn display_name(aemet_name: &str) -> String {
    let name = aemet_name.split('/').next().unwrap_or(aemet_name).trim();
    match name.rsplit_once(", ") {
        Some((base, article)) if article.split_whitespace().count() == 1 && article.len() <= 4 => {
            if article.ends_with('\'') {
                format!("{article}{base}")
            } else {
                format!("{article} {base}")
            }
        }
        _ => name.to_owned(),
    }
}
