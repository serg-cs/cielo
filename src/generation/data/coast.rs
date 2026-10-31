//! Matching coastal warnings to the municipalities on that coast.
//!
//! Land warning zones are listed for each municipality in the catalog. Coastal
//! zones (codes ending in `C`) are sea areas, so a municipality is matched
//! when its point lies within [`COASTAL_DISTANCE_KM`] of the warning polygon
//! and it belongs to the zone's province.

use crate::{aemet::WeatherWarning, catalog::Municipality};

pub(super) const COASTAL_DISTANCE_KM: f64 = 6.0;
const EARTH_RADIUS_KM: f64 = 6371.0;

pub(super) fn is_coastal_zone(zone: &str) -> bool {
    zone.ends_with(['C', 'c'])
}

/// Province code inside a Meteoalerta zone code such as `722802` or `611101C`.
fn zone_province(zone: &str) -> Option<&str> {
    zone.get(2..4)
}

/// Municipalities a coastal warning applies to.
pub(super) fn coastal_municipalities<'a>(
    warning: &WeatherWarning,
    municipalities: impl Iterator<Item = &'a Municipality>,
) -> Vec<&'a str> {
    let provinces = warning
        .zones
        .iter()
        .filter(|zone| is_coastal_zone(zone))
        .filter_map(|zone| zone_province(zone))
        .collect::<Vec<_>>();
    if provinces.is_empty() || warning.polygons.is_empty() {
        return Vec::new();
    }
    municipalities
        .filter(|municipality| provinces.contains(&&municipality.id[..2]))
        .filter(|municipality| {
            let (Some(latitude), Some(longitude)) = (municipality.latitude, municipality.longitude)
            else {
                return false;
            };
            warning.polygons.iter().any(|polygon| {
                distance_to_polygon_km((latitude, longitude), polygon) <= COASTAL_DISTANCE_KM
            })
        })
        .map(|municipality| municipality.id.as_str())
        .collect()
}

/// Distance from a point to a polygon (0 inside), using a local flat projection.
pub(super) fn distance_to_polygon_km(point: (f64, f64), polygon: &[(f64, f64)]) -> f64 {
    let scale_x = EARTH_RADIUS_KM * point.0.to_radians().cos() * std::f64::consts::PI / 180.0;
    let scale_y = EARTH_RADIUS_KM * std::f64::consts::PI / 180.0;
    let project = |(latitude, longitude): (f64, f64)| {
        (
            (longitude - point.1) * scale_x,
            (latitude - point.0) * scale_y,
        )
    };
    let points = polygon.iter().copied().map(project).collect::<Vec<_>>();
    if points.len() < 3 {
        return f64::INFINITY;
    }

    let mut inside = false;
    let mut nearest = f64::INFINITY;
    for index in 0..points.len() {
        let start = points[index];
        let end = points[(index + 1) % points.len()];
        if (start.1 > 0.0) != (end.1 > 0.0)
            && 0.0 < (end.0 - start.0) * -start.1 / (end.1 - start.1) + start.0
        {
            inside = !inside;
        }
        nearest = nearest.min(distance_to_segment(start, end));
    }
    if inside { 0.0 } else { nearest }
}

/// Distance from the origin to segment `a`-`b`.
fn distance_to_segment(start: (f64, f64), end: (f64, f64)) -> f64 {
    let (dx, dy) = (end.0 - start.0, end.1 - start.1);
    let length = dx.mul_add(dx, dy * dy);
    let along = if length == 0.0 {
        0.0
    } else {
        (-start.0.mul_add(dx, start.1 * dy) / length).clamp(0.0, 1.0)
    };
    along.mul_add(dx, start.0).hypot(along.mul_add(dy, start.1))
}
