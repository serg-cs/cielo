//! Wildfire danger forecast rasters (AEMET "peligro de incendios").

use std::{collections::BTreeMap, io::Cursor};

use anyhow::{Context, Result, bail};
use jiff::{ToSpan, civil::Date};
use tiff::{
    decoder::{Decoder, DecodingResult},
    tags::Tag,
};

use super::{
    archive::{file_name, read_archive},
    forecast::parse_date,
    models::FireDanger,
};

/// One danger raster in geographic coordinates (WGS84 degrees).
#[derive(Debug)]
pub(super) struct FireGrid {
    west: f64,
    north: f64,
    pixel_width: f64,
    pixel_height: f64,
    width: usize,
    height: usize,
    values: Vec<f32>,
}

impl FireGrid {
    /// Highest danger level in the 3x3 pixels around a point, if any.
    fn sample(&self, latitude: f64, longitude: f64) -> Option<u8> {
        let column = ((longitude - self.west) / self.pixel_width).floor();
        let row = ((self.north - latitude) / self.pixel_height).floor();
        #[allow(clippy::cast_precision_loss)]
        if column < -1.0 || row < -1.0 || column > self.width as f64 || row > self.height as f64 {
            return None;
        }
        #[allow(clippy::cast_possible_truncation)]
        let (column, row) = (column as i64, row as i64);
        let mut highest = None;
        for dy in -1..=1 {
            for dx in -1..=1 {
                let (x, y) = (column + dx, row + dy);
                let (Ok(x), Ok(y)) = (usize::try_from(x), usize::try_from(y)) else {
                    continue;
                };
                if x >= self.width || y >= self.height {
                    continue;
                }
                let value = self.values[y * self.width + x];
                if !value.is_finite() {
                    continue;
                }
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let level = value.round().clamp(0.0, 255.0) as u8;
                if (1..=6).contains(&level) {
                    highest = highest.max(Some(level));
                }
            }
        }
        highest
    }
}

/// Danger rasters for each forecast date and area.
#[derive(Debug, Default)]
pub(crate) struct FireForecast {
    /// `(date, area)` where area is `p` (peninsula and Baleares) or `c` (Canarias).
    grids: BTreeMap<(Date, char), FireGrid>,
}

impl FireForecast {
    /// Danger levels at a point for every available date.
    pub(crate) fn sample(&self, latitude: f64, longitude: f64) -> FireDanger {
        let area = if latitude < 30.5 && longitude < -12.0 {
            'c'
        } else {
            'p'
        };
        self.grids
            .iter()
            .filter(|((_, grid_area), _)| *grid_area == area)
            .filter_map(|((date, _), grid)| Some((*date, grid.sample(latitude, longitude)?)))
            .collect()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.grids.is_empty()
    }
}

/// Parse AEMET's fire danger download (a tar.gz of `GeoTIFF` rasters).
///
/// File names look like `down_20261030_peligro_p_D03.tif`: issue date, area
/// and days ahead.
pub(super) fn parse_fire_archive(bytes: &[u8]) -> Result<FireForecast> {
    let mut forecast = FireForecast::default();
    for entry in read_archive(bytes, "fire danger", 32 * 1024 * 1024)? {
        let name = file_name(&entry.path);
        let Some((date, area)) = raster_key(name) else {
            continue;
        };
        let grid = parse_grid(&entry.body).with_context(|| format!("invalid raster {name}"))?;
        forecast.grids.insert((date, area), grid);
    }
    if forecast.is_empty() {
        bail!("fire danger archive does not contain any rasters");
    }
    Ok(forecast)
}

fn raster_key(name: &str) -> Option<(Date, char)> {
    let stem = name.strip_suffix(".tif")?;
    let parts = stem.split('_').collect::<Vec<_>>();
    let [_, issued, "peligro", area, day] = parts.as_slice() else {
        return None;
    };
    let area = match *area {
        "p" => 'p',
        "c" => 'c',
        _ => return None,
    };
    let days_ahead = day.strip_prefix('D')?.parse::<i64>().ok()?;
    let date = parse_date(issued)?.checked_add(days_ahead.days()).ok()?;
    Some((date, area))
}

pub(super) fn parse_grid(bytes: &[u8]) -> Result<FireGrid> {
    let mut decoder = Decoder::new(Cursor::new(bytes)).context("invalid TIFF")?;
    let (width, height) = decoder.dimensions().context("missing TIFF dimensions")?;
    let scale = decoder
        .get_tag_f64_vec(Tag::ModelPixelScaleTag)
        .context("missing GeoTIFF pixel scale")?;
    let tiepoint = decoder
        .get_tag_f64_vec(Tag::ModelTiepointTag)
        .context("missing GeoTIFF tie point")?;
    let (Some(&pixel_width), Some(&pixel_height)) = (scale.first(), scale.get(1)) else {
        bail!("invalid GeoTIFF pixel scale");
    };
    let [raster_x, raster_y, _, west, north, ..] = tiepoint.as_slice() else {
        bail!("invalid GeoTIFF tie point");
    };
    if !(0.0001..=1.0).contains(&pixel_width) || !(0.0001..=1.0).contains(&pixel_height) {
        bail!("unexpected GeoTIFF pixel scale {pixel_width}x{pixel_height}");
    }
    if !(-30.0..=10.0).contains(west) || !(20.0..=50.0).contains(north) {
        bail!("GeoTIFF origin {west},{north} is not in geographic coordinates over Spain");
    }

    let values = match decoder.read_image().context("failed to read TIFF data")? {
        DecodingResult::F32(values) => values,
        #[allow(clippy::cast_possible_truncation)]
        DecodingResult::F64(values) => values.into_iter().map(|value| value as f32).collect(),
        DecodingResult::U8(values) => values.into_iter().map(f32::from).collect(),
        DecodingResult::U16(values) => values.into_iter().map(f32::from).collect(),
        DecodingResult::I16(values) => values.into_iter().map(f32::from).collect(),
        _ => bail!("unsupported TIFF sample format"),
    };
    let width = usize::try_from(width)?;
    let height = usize::try_from(height)?;
    if values.len() != width * height {
        bail!(
            "TIFF has {} samples for {width}x{height} pixels",
            values.len()
        );
    }
    Ok(FireGrid {
        west: west - raster_x * pixel_width,
        north: north + raster_y * pixel_height,
        pixel_width,
        pixel_height,
        width,
        height,
        values,
    })
}
