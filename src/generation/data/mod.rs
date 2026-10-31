//! Weather data build: fetch AEMET's products for every place in the catalog
//! and write the published files (see the README for their format).
//!
//! Forecasts are required: without them the build fails and nothing is
//! published, so the files already online stay as they are. Warnings, fire
//! danger and beaches are optional: if one fails it is reported and left
//! out, and the rest is still published.

mod coast;
mod documents;
mod report;

#[cfg(test)]
mod tests;

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs::{self, File},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use jiff::Timestamp;
use serde::Serialize;
use tracing::{info, warn};

use crate::{
    aemet::{
        AemetClient, AemetProduct, BeachForecast, DailyForecast, EntryFailure, FireForecast,
        HourlyForecast, MunicipalityTimeZone, WeatherWarning,
    },
    catalog::{Catalog, validate_municipality_id},
};

use self::{
    coast::{coastal_municipalities, is_coastal_zone},
    documents::{
        BeachDocument, BeachesDocument, BundleDocument, MetaDocument, MunicipalityDocument,
        PlacesDocument, ProvinceDocument, SCHEMA_VERSION, WarningDocument, WarningsDocument,
        local_time,
    },
    report::{DataBuildReport, ProductStatus, group_failures},
};
use super::{
    GENERATOR_IDENTITY,
    publisher::{OutputKind, create_staging_directory, publish_staging_directory},
};

const BUNDLE_RANGE_SIZE: u16 = 20;
pub(crate) const SCHEMA_DIRECTORY: &str = "v4";
pub(crate) const PLACES_FILE: &str = "v4/places.json";
pub(crate) const MARKER_FILE: &str = ".cielo-data";
/// Largest share of municipalities that may fail before the build is stopped.
const MAX_FAILED_PERCENT: usize = 5;
/// The build must publish at least this many municipalities.
const MIN_PUBLISHED: usize = 7_500;
/// Largest share of beaches that may fail before beaches are left out.
const MAX_FAILED_BEACH_PERCENT: usize = 50;

#[derive(Debug)]
pub(crate) struct DataBuildSummary {
    pub(crate) municipalities: usize,
    pub(crate) failed: usize,
    pub(crate) files: usize,
    pub(crate) bytes: usize,
}

/// Local marker that identifies a generated data directory; never uploaded.
#[derive(Serialize)]
struct MarkerDocument<'a> {
    generator: &'a str,
    schema: u8,
}

/// Forecasts of every municipality, with the documents that failed.
struct Forecasts {
    hourly: Vec<HourlyForecast>,
    daily: Vec<DailyForecast>,
    failures: Vec<EntryFailure>,
    entries: usize,
}

pub(crate) async fn generate_weather_data(
    client: &AemetClient,
    output: &Path,
) -> Result<DataBuildSummary> {
    let catalog = Catalog::embedded()?;
    let (output, staging) = create_staging_directory(output, OutputKind::Data)?;
    let mut writer = Writer::new(staging.path());
    let mut report = DataBuildReport::default();
    let mut failures = Vec::new();
    let now = Timestamp::now();

    // Fetch everything at once; the client spaces out API calls.
    let (forecasts, warnings, fire, beaches) = tokio::join!(
        fetch_forecasts(client),
        client.warnings(now),
        client.fire(),
        client.beaches(&catalog.beaches),
    );
    let fire = match fire {
        Ok(fire) => {
            report
                .products
                .push((AemetProduct::Fire, ProductStatus::Published(1)));
            Some(fire)
        }
        Err(error) => {
            warn!(
                error = format!("{error:#}"),
                "publishing forecasts without fire danger"
            );
            report.products.push((
                AemetProduct::Fire,
                ProductStatus::Failed(format!("{error:#}")),
            ));
            None
        }
    };

    let published = write_forecasts(
        &catalog,
        forecasts.context("failed to fetch AEMET forecasts")?,
        fire.as_ref(),
        now,
        &mut writer,
        &mut report,
        &mut failures,
    )?;
    write_warnings(&catalog, warnings, &mut writer, &mut report, &mut failures)?;
    write_beaches(beaches, now, &mut writer, &mut report, &mut failures)?;
    writer.json(
        Path::new(MARKER_FILE),
        &MarkerDocument {
            generator: GENERATOR_IDENTITY,
            schema: SCHEMA_VERSION,
        },
    )?;

    report.published = published;
    report.groups = group_failures(&failures);
    if report.failed > 0 || report.products.iter().any(|(_, status)| !status.is_ok()) {
        warn!("{}", report.render_text());
    } else {
        info!("{}", report.render_text());
    }
    if let Err(error) = report.write_step_summary() {
        warn!(
            error = format!("{error:#}"),
            "failed to write GitHub step summary"
        );
    }

    publish_staging_directory(&staging, &output, OutputKind::Data)?;
    Ok(DataBuildSummary {
        municipalities: published,
        failed: report.failed,
        files: writer.files,
        bytes: writer.bytes,
    })
}

async fn fetch_forecasts(client: &AemetClient) -> Result<Forecasts> {
    let (hourly, daily) = tokio::try_join!(client.hourly_forecasts(), client.daily_forecasts())?;
    let mut failures = hourly.failures;
    failures.extend(daily.failures);
    Ok(Forecasts {
        entries: hourly.entries.max(daily.entries),
        hourly: hourly.items,
        daily: daily.items,
        failures,
    })
}

/// Write the forecast bundles and the list of places; returns how many
/// municipalities were published.
fn write_forecasts(
    catalog: &Catalog,
    forecasts: Forecasts,
    fire: Option<&FireForecast>,
    now: Timestamp,
    writer: &mut Writer,
    report: &mut DataBuildReport,
    failures: &mut Vec<EntryFailure>,
) -> Result<usize> {
    report.products.push((
        AemetProduct::Hourly,
        ProductStatus::Published(forecasts.hourly.len()),
    ));
    report.products.push((
        AemetProduct::Daily,
        ProductStatus::Published(forecasts.daily.len()),
    ));
    let failed = forecasts
        .failures
        .iter()
        .map(|failure| failure.id.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    failures.extend(forecasts.failures);
    check_failures(failed, forecasts.entries, report, failures)?;

    let hourly = forecasts
        .hourly
        .iter()
        .map(|forecast| (forecast.id.as_str(), forecast))
        .collect::<HashMap<_, _>>();
    let daily = forecasts
        .daily
        .iter()
        .map(|forecast| (forecast.id.as_str(), forecast))
        .collect::<HashMap<_, _>>();
    let ids = hourly
        .keys()
        .chain(daily.keys())
        .copied()
        .collect::<BTreeSet<_>>();

    // Stamped with AEMET's latest issue time, not the build time, so an
    // unchanged list is byte-for-byte identical and is not uploaded again.
    let latest_issue = hourly
        .values()
        .filter_map(|forecast| forecast.issued)
        .chain(daily.values().filter_map(|forecast| forecast.issued))
        .max()
        .unwrap_or(now);
    let mut places = PlacesDocument {
        schema: SCHEMA_VERSION,
        issued: local_time(latest_issue, &MunicipalityTimeZone::EuropeMadrid.rules()?),
        ..PlacesDocument::default()
    };
    let mut bundles = BTreeMap::<PathBuf, BTreeMap<&str, MunicipalityDocument>>::new();
    for id in ids {
        let Some(municipality) = catalog.municipalities.get(id) else {
            failures.push(EntryFailure {
                id: id.to_owned(),
                product: AemetProduct::Hourly,
                error: anyhow::anyhow!(
                    "municipality {id} is not in the catalog; run `cielo catalog update`"
                ),
            });
            continue;
        };
        let fire_danger = match (fire, municipality.latitude, municipality.longitude) {
            (Some(fire), Some(latitude), Some(longitude)) => Some(fire.sample(latitude, longitude)),
            _ => None,
        };
        let time_zone = MunicipalityTimeZone::for_municipality(id).rules()?;
        let document = MunicipalityDocument::new(
            hourly.get(id).copied(),
            daily.get(id).copied(),
            MetaDocument::new(municipality),
            fire_danger.as_ref(),
            &time_zone,
        );
        bundles
            .entry(bundle_path(id)?)
            .or_default()
            .insert(id, document);
        places.ids.push(id.to_owned());
        places.names.push(municipality.name.clone());
        places.pop.push(municipality.population);
    }

    let published = places.ids.len();
    if published < MIN_PUBLISHED {
        bail!("only {published} municipalities have usable forecasts (minimum {MIN_PUBLISHED})");
    }

    add_provinces_and_beaches(catalog, &mut places);

    for (path, municipalities) in bundles {
        writer.json(
            &path,
            &BundleDocument {
                schema: SCHEMA_VERSION,
                municipalities,
            },
        )?;
    }
    writer.json(Path::new(PLACES_FILE), &places)?;
    Ok(published)
}

/// Stop the build when too many municipalities failed.
fn check_failures(
    failed: usize,
    entries: usize,
    report: &mut DataBuildReport,
    failures: &[EntryFailure],
) -> Result<()> {
    report.source_entries = entries;
    report.failed = failed;
    if failed * 100 > entries * MAX_FAILED_PERCENT {
        report.groups = group_failures(failures);
        let _ = report.write_step_summary();
        bail!(
            "{failed} of {entries} AEMET municipalities have unusable data, above the {MAX_FAILED_PERCENT}% limit\n{}",
            report.render_text()
        );
    }
    Ok(())
}

/// The provinces and beaches of the published municipalities.
fn add_provinces_and_beaches(catalog: &Catalog, places: &mut PlacesDocument) {
    let published = places
        .ids
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    places.provinces = published
        .iter()
        .map(|id| &id[..2])
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|code| {
            Some(ProvinceDocument {
                code: code.to_owned(),
                name: catalog.provinces.get(code)?.clone(),
            })
        })
        .collect();
    for beach in &catalog.beaches {
        if published.contains(beach.municipality_id.as_str()) {
            places.beaches.ids.push(beach.id.clone());
            places.beaches.names.push(beach.name.clone());
            places.beaches.m.push(beach.municipality_id.clone());
        }
    }
}

/// Write `warnings.json`; if warnings failed, the published file stays as it is.
fn write_warnings(
    catalog: &Catalog,
    warnings: Result<(Vec<WeatherWarning>, Vec<EntryFailure>)>,
    writer: &mut Writer,
    report: &mut DataBuildReport,
    failures: &mut Vec<EntryFailure>,
) -> Result<()> {
    let (warnings, warning_failures) = match warnings {
        Ok(warnings) => warnings,
        Err(error) => {
            warn!(error = format!("{error:#}"), "publishing without warnings");
            report.products.push((
                AemetProduct::Warnings,
                ProductStatus::Failed(format!("{error:#}")),
            ));
            return Ok(());
        }
    };
    failures.extend(warning_failures);
    let documents = warnings
        .iter()
        .map(|warning| {
            let coastal = warning.zones.iter().any(|zone| is_coastal_zone(zone));
            let municipalities = if coastal {
                coastal_municipalities(warning, catalog.municipalities.values())
            } else {
                Vec::new()
            };
            WarningDocument::new(warning, municipalities)
        })
        .collect::<Vec<_>>();
    writer.json(
        &Path::new(SCHEMA_DIRECTORY).join("warnings.json"),
        &WarningsDocument {
            schema: SCHEMA_VERSION,
            warnings: documents,
        },
    )?;
    report.products.push((
        AemetProduct::Warnings,
        ProductStatus::Published(warnings.len()),
    ));
    Ok(())
}

/// Write `beaches/<PP>.json`, one file per province, unless too many beaches failed.
fn write_beaches(
    beaches: (Vec<BeachForecast>, Vec<EntryFailure>),
    now: Timestamp,
    writer: &mut Writer,
    report: &mut DataBuildReport,
    failures: &mut Vec<EntryFailure>,
) -> Result<()> {
    let (forecasts, beach_failures) = beaches;
    let total = forecasts.len() + beach_failures.len();
    let failed = beach_failures.len();
    failures.extend(beach_failures);
    if total == 0 || failed * 100 > total * MAX_FAILED_BEACH_PERCENT {
        report.products.push((
            AemetProduct::Beaches,
            ProductStatus::Failed(format!("{failed} of {total} beaches failed")),
        ));
        return Ok(());
    }

    let mut provinces = BTreeMap::<&str, BTreeMap<&str, BeachDocument>>::new();
    let mut latest = BTreeMap::<&str, Timestamp>::new();
    for forecast in &forecasts {
        let province = &forecast.municipality_id[..2];
        let time_zone =
            MunicipalityTimeZone::for_municipality(&forecast.municipality_id).rules()?;
        provinces
            .entry(province)
            .or_default()
            .insert(&forecast.id, BeachDocument::new(forecast, &time_zone));
        if let Some(issued) = forecast.issued {
            let entry = latest.entry(province).or_insert(issued);
            *entry = (*entry).max(issued);
        }
    }
    let madrid = MunicipalityTimeZone::EuropeMadrid.rules()?;
    for (province, beaches) in provinces {
        // AEMET's issue time, so an unchanged file is not uploaded again.
        let issued = latest.get(province).copied().unwrap_or(now);
        writer.json(
            &Path::new(SCHEMA_DIRECTORY)
                .join("beaches")
                .join(format!("{province}.json")),
            &BeachesDocument {
                schema: SCHEMA_VERSION,
                issued: local_time(issued, &madrid),
                beaches,
            },
        )?;
    }
    report.products.push((
        AemetProduct::Beaches,
        ProductStatus::Published(forecasts.len()),
    ));
    Ok(())
}

/// Bundle holding a municipality: `v4/forecast/<PP>/<NNN>.json`, 20 IDs per file.
pub(crate) fn bundle_path(municipality_id: &str) -> Result<PathBuf> {
    validate_municipality_id(municipality_id)?;
    let number = municipality_id[2..]
        .parse::<u16>()
        .with_context(|| format!("invalid municipality ID: {municipality_id}"))?;
    let start = number / BUNDLE_RANGE_SIZE * BUNDLE_RANGE_SIZE;
    Ok(Path::new(SCHEMA_DIRECTORY)
        .join("forecast")
        .join(&municipality_id[..2])
        .join(format!("{start:03}.json")))
}

/// Writes generated files and counts them.
struct Writer {
    root: PathBuf,
    files: usize,
    bytes: usize,
}

impl Writer {
    fn new(root: &Path) -> Self {
        Self {
            root: root.to_owned(),
            files: 0,
            bytes: 0,
        }
    }

    fn json(&mut self, relative: &Path, value: &impl Serialize) -> Result<()> {
        let bytes = serde_json::to_vec(value)
            .with_context(|| format!("failed to serialize {}", relative.display()))?;
        let path = self.root.join(relative);
        let parent = path
            .parent()
            .context("generated data file does not have a parent directory")?;
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
        let file =
            File::create(&path).with_context(|| format!("failed to create {}", path.display()))?;
        let mut writer = BufWriter::new(file);
        writer
            .write_all(&bytes)
            .and_then(|()| writer.flush())
            .with_context(|| format!("failed to write {}", path.display()))?;
        self.files += 1;
        self.bytes += bytes.len();
        Ok(())
    }
}
