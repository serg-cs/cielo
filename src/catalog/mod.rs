//! The places Cielo knows: provinces, municipalities and beaches.
//!
//! They are reviewed CSV files in the repository's `catalog/` folder,
//! embedded in the binary so builds never download them. Municipality names
//! are curated Spanish display names; the other values come from AEMET and
//! are refreshed with `cielo catalog update` (see `catalog/README.md`).

mod csv;
mod update;

#[cfg(test)]
mod tests;

use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result, bail};

pub(crate) use update::update_catalog;

const PROVINCES_FILE: &str = "provinces.csv";
const MUNICIPALITIES_FILE: &str = "municipalities.csv";
const BEACHES_FILE: &str = "beaches.csv";

const PROVINCES_HEADER: [&str; 2] = ["code", "name"];
const MUNICIPALITIES_HEADER: [&str; 7] = [
    "id",
    "name",
    "latitude",
    "longitude",
    "altitude",
    "population",
    "warning_zone",
];
const BEACHES_HEADER: [&str; 5] = ["id", "name", "municipality", "latitude", "longitude"];

const EMBEDDED_PROVINCES: &str = include_str!("../../catalog/provinces.csv");
const EMBEDDED_MUNICIPALITIES: &str = include_str!("../../catalog/municipalities.csv");
const EMBEDDED_BEACHES: &str = include_str!("../../catalog/beaches.csv");

/// A municipality: its curated name and AEMET's data about it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Municipality {
    /// INE code, such as `28079`.
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) latitude: Option<f64>,
    pub(crate) longitude: Option<f64>,
    pub(crate) altitude_metres: Option<i32>,
    pub(crate) population: Option<u32>,
    /// Meteoalerta warning zone, such as `722802`.
    pub(crate) warning_zone: Option<String>,
}

/// A beach with an AEMET forecast.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Beach {
    /// AEMET beach code, such as `0301401`.
    pub(crate) id: String,
    pub(crate) name: String,
    /// INE code of its municipality.
    pub(crate) municipality_id: String,
    pub(crate) latitude: f64,
    pub(crate) longitude: f64,
}

/// Every place, validated: codes are well formed and unique, every
/// municipality has a province and every beach a municipality.
#[derive(Debug)]
pub(crate) struct Catalog {
    /// Province names by two-digit code.
    pub(crate) provinces: BTreeMap<String, String>,
    pub(crate) municipalities: BTreeMap<String, Municipality>,
    pub(crate) beaches: Vec<Beach>,
}

impl Catalog {
    /// The catalog built into the binary.
    pub(crate) fn embedded() -> Result<Self> {
        Self::parse(
            EMBEDDED_PROVINCES,
            EMBEDDED_MUNICIPALITIES,
            EMBEDDED_BEACHES,
        )
    }

    /// The catalog files in a directory.
    pub(crate) fn read(directory: &Path) -> Result<Self> {
        let read = |file: &str| {
            fs::read_to_string(directory.join(file))
                .with_context(|| format!("failed to read {}", directory.join(file).display()))
        };
        Self::parse(
            &read(PROVINCES_FILE)?,
            &read(MUNICIPALITIES_FILE)?,
            &read(BEACHES_FILE)?,
        )
    }

    /// Write the catalog files to a directory, sorted by code.
    pub(crate) fn write(&self, directory: &Path) -> Result<()> {
        let provinces = self
            .provinces
            .iter()
            .map(|(code, name)| vec![code.clone(), name.clone()])
            .collect::<Vec<_>>();
        let municipalities = self
            .municipalities
            .values()
            .map(|municipality| {
                vec![
                    municipality.id.clone(),
                    municipality.name.clone(),
                    optional(municipality.latitude.map(|value| format!("{value:.5}"))),
                    optional(municipality.longitude.map(|value| format!("{value:.5}"))),
                    optional(municipality.altitude_metres.map(|value| value.to_string())),
                    optional(municipality.population.map(|value| value.to_string())),
                    optional(municipality.warning_zone.clone()),
                ]
            })
            .collect::<Vec<_>>();
        let mut beaches = self.beaches.clone();
        beaches.sort_by(|left, right| left.id.cmp(&right.id));
        let beaches = beaches
            .iter()
            .map(|beach| {
                vec![
                    beach.id.clone(),
                    beach.name.clone(),
                    beach.municipality_id.clone(),
                    format!("{:.5}", beach.latitude),
                    format!("{:.5}", beach.longitude),
                ]
            })
            .collect::<Vec<_>>();
        for (file, contents) in [
            (PROVINCES_FILE, csv::write(&PROVINCES_HEADER, &provinces)),
            (
                MUNICIPALITIES_FILE,
                csv::write(&MUNICIPALITIES_HEADER, &municipalities),
            ),
            (BEACHES_FILE, csv::write(&BEACHES_HEADER, &beaches)),
        ] {
            fs::write(directory.join(file), contents)
                .with_context(|| format!("failed to write {}", directory.join(file).display()))?;
        }
        Ok(())
    }

    /// Parse and validate the three files.
    pub(crate) fn parse(provinces: &str, municipalities: &str, beaches: &str) -> Result<Self> {
        let mut catalog = Self {
            provinces: BTreeMap::new(),
            municipalities: BTreeMap::new(),
            beaches: Vec::new(),
        };
        for row in csv::read(provinces, &PROVINCES_HEADER, PROVINCES_FILE)? {
            let [code, name] = <[String; 2]>::try_from(row).expect("checked width");
            if code.len() != 2 || !code.bytes().all(|byte| byte.is_ascii_digit()) {
                bail!("{PROVINCES_FILE}: invalid province code {code}");
            }
            check_display_name(&name)
                .with_context(|| format!("{PROVINCES_FILE}: province {code}"))?;
            if catalog.provinces.insert(code.clone(), name).is_some() {
                bail!("{PROVINCES_FILE}: province {code} appears twice");
            }
        }
        for row in csv::read(municipalities, &MUNICIPALITIES_HEADER, MUNICIPALITIES_FILE)? {
            let municipality = parse_municipality(row)?;
            if !catalog.provinces.contains_key(&municipality.id[..2]) {
                bail!(
                    "{MUNICIPALITIES_FILE}: municipality {} has no province",
                    municipality.id
                );
            }
            let id = municipality.id.clone();
            if catalog
                .municipalities
                .insert(id.clone(), municipality)
                .is_some()
            {
                bail!("{MUNICIPALITIES_FILE}: municipality {id} appears twice");
            }
        }
        let mut beach_ids = std::collections::BTreeSet::new();
        for row in csv::read(beaches, &BEACHES_HEADER, BEACHES_FILE)? {
            let beach = parse_beach(row)?;
            if !catalog.municipalities.contains_key(&beach.municipality_id) {
                bail!(
                    "{BEACHES_FILE}: beach {} is in unknown municipality {}",
                    beach.id,
                    beach.municipality_id
                );
            }
            if !beach_ids.insert(beach.id.clone()) {
                bail!("{BEACHES_FILE}: beach {} appears twice", beach.id);
            }
            catalog.beaches.push(beach);
        }
        Ok(catalog)
    }
}

/// Check a five-digit INE municipality code.
pub(crate) fn validate_municipality_id(id: &str) -> Result<()> {
    if id.len() != 5 || !id.bytes().all(|byte| byte.is_ascii_digit()) {
        bail!("invalid municipality ID: {id}");
    }
    Ok(())
}

fn parse_municipality(row: Vec<String>) -> Result<Municipality> {
    let [id, name, latitude, longitude, altitude, population, zone] =
        <[String; 7]>::try_from(row).expect("checked width");
    validate_municipality_id(&id).context(MUNICIPALITIES_FILE)?;
    check_display_name(&name)
        .with_context(|| format!("{MUNICIPALITIES_FILE}: municipality {id}"))?;
    let context = || format!("{MUNICIPALITIES_FILE}: municipality {id}");
    let latitude = parse_optional::<f64>(&latitude).with_context(context)?;
    let longitude = parse_optional::<f64>(&longitude).with_context(context)?;
    if latitude.is_some_and(|value| !(25.0..=45.0).contains(&value))
        || longitude.is_some_and(|value| !(-20.0..=6.0).contains(&value))
    {
        bail!("{}: coordinates outside Spain", context());
    }
    if !zone.is_empty() && (zone.len() != 6 || !zone.bytes().all(|byte| byte.is_ascii_digit())) {
        bail!("{}: invalid warning zone {zone}", context());
    }
    Ok(Municipality {
        name,
        latitude,
        longitude,
        altitude_metres: parse_optional(&altitude).with_context(context)?,
        population: parse_optional(&population).with_context(context)?,
        warning_zone: (!zone.is_empty()).then_some(zone),
        id,
    })
}

fn parse_beach(row: Vec<String>) -> Result<Beach> {
    let [id, name, municipality_id, latitude, longitude] =
        <[String; 5]>::try_from(row).expect("checked width");
    if id.len() != 7 || !id.bytes().all(|byte| byte.is_ascii_digit()) {
        bail!("{BEACHES_FILE}: invalid beach code {id}");
    }
    validate_municipality_id(&municipality_id)
        .with_context(|| format!("{BEACHES_FILE}: beach {id}"))?;
    if name.is_empty() || name.trim() != name {
        bail!("{BEACHES_FILE}: beach {id} has an empty or untrimmed name");
    }
    let context = || format!("{BEACHES_FILE}: beach {id}");
    Ok(Beach {
        latitude: latitude.parse().with_context(context)?,
        longitude: longitude.parse().with_context(context)?,
        id,
        name,
        municipality_id,
    })
}

/// Names shown in the app: trimmed, one form only (no `A/B` alternatives).
fn check_display_name(name: &str) -> Result<()> {
    if name.is_empty() || name.trim() != name || name.chars().any(char::is_control) {
        bail!("empty or untrimmed name {name:?}");
    }
    if name.contains('/') {
        bail!("{name:?} lists alternative forms; keep the Spanish one");
    }
    Ok(())
}

fn parse_optional<T: std::str::FromStr>(value: &str) -> Result<Option<T>>
where
    T::Err: std::error::Error + Send + Sync + 'static,
{
    if value.is_empty() {
        return Ok(None);
    }
    value
        .parse()
        .map(Some)
        .with_context(|| format!("invalid value {value:?}"))
}

fn optional(value: Option<String>) -> String {
    value.unwrap_or_default()
}
