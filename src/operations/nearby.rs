#![deny(warnings)]

// Find nearby OSM features by tag via the Overpass API.
// https://wiki.openstreetmap.org/wiki/Overpass_API

use crate::config::OsmConfig;
use crate::error::{OsmError, Result};
use serde::Deserialize;
use serde_json::{Value, json};

/// Largest search radius we allow, in metres. Overpass `around` queries get
/// expensive quickly; 50 km is a generous ceiling for "nearby".
const MAX_RADIUS_M: u32 = 50_000;
/// Hard cap on the number of features requested, to keep responses bounded.
const MAX_LIMIT: u32 = 200;

#[derive(Debug, Deserialize)]
struct OverpassResponse {
    #[serde(default)]
    elements: Vec<OverpassElement>,
}

#[derive(Debug, Deserialize)]
struct OverpassElement {
    #[serde(rename = "type")]
    kind: String,
    id: u64,
    lat: Option<f64>,
    lon: Option<f64>,
    center: Option<Center>,
    #[serde(default)]
    tags: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct Center {
    lat: f64,
    lon: f64,
}

/// A tag is `key` alone (presence) or `key=value`. We embed it into Overpass QL
/// inside double quotes, so reject any character that could break out of the
/// quoted string or inject additional query syntax.
fn validate_tag_component(label: &str, s: &str) -> Result<()> {
    if s.is_empty() {
        return Err(OsmError::InvalidParameters(format!("{} must not be empty", label)).into());
    }
    if s.chars()
        .any(|c| c == '"' || c == '\\' || c == '\n' || c == '\r' || c == ']' || c == '[')
    {
        return Err(OsmError::InvalidParameters(format!(
            "{} contains characters that are not allowed in an OSM tag: {:?}",
            label, s
        ))
        .into());
    }
    Ok(())
}

/// Great-circle distance between two coordinates in metres (haversine).
fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const EARTH_RADIUS_M: f64 = 6_371_000.0;
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let a = (dlat / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_M * a.sqrt().asin()
}

/// Build the Overpass QL for a tag filter applied to nodes, ways, and
/// relations within `radius` metres of (`lat`, `lon`).
fn build_query(
    key: &str,
    value: Option<&str>,
    radius: u32,
    lat: f64,
    lon: f64,
    limit: u32,
) -> String {
    let selector = match value {
        Some(v) => format!("[\"{}\"=\"{}\"]", key, v),
        None => format!("[\"{}\"]", key),
    };
    let around = format!("(around:{},{},{})", radius, lat, lon);
    format!(
        "[out:json][timeout:25];\n(\n  node{sel}{ar};\n  way{sel}{ar};\n  relation{sel}{ar};\n);\nout center {limit};",
        sel = selector,
        ar = around,
        limit = limit
    )
}

/// Find OSM features tagged with `key` (optionally `key=value`) within `radius`
/// metres of a coordinate, sorted nearest-first.
///
/// Returns an empty array when nothing matches — finding zero features is a
/// valid answer, not an error.
#[allow(clippy::too_many_arguments)]
pub async fn nearby(
    client: &reqwest::Client,
    config: &OsmConfig,
    latitude: f64,
    longitude: f64,
    radius: u32,
    key: &str,
    value: Option<&str>,
    limit: u32,
) -> Result<Value> {
    validate_tag_component("key", key)?;
    if let Some(v) = value {
        validate_tag_component("value", v)?;
    }
    let radius = radius.clamp(1, MAX_RADIUS_M);
    let limit = limit.clamp(1, MAX_LIMIT);

    let query = build_query(key, value, radius, latitude, longitude, limit);

    // Overpass accepts the raw OverpassQL as the POST body.
    let resp = client.post(&config.overpass_url).body(query).send().await?;
    let status = resp.status();
    if !status.is_success() {
        return Err(OsmError::ApiError(format!("Overpass returned HTTP {}", status)).into());
    }

    let body: OverpassResponse = resp.json().await?;

    let mut features: Vec<(f64, Value)> = body
        .elements
        .into_iter()
        .filter_map(|el| {
            let (flat, flon) = match (el.lat, el.lon, &el.center) {
                (Some(la), Some(lo), _) => (la, lo),
                (_, _, Some(c)) => (c.lat, c.lon),
                _ => return None,
            };
            let distance = haversine_m(latitude, longitude, flat, flon);
            let name = el
                .tags
                .as_ref()
                .and_then(|t| t.get("name"))
                .and_then(|n| n.as_str())
                .map(str::to_string);
            let feature = json!({
                "osm_type": el.kind,
                "osm_id": el.id,
                "name": name,
                "latitude": flat,
                "longitude": flon,
                "distance_meters": distance.round(),
                "tags": el.tags,
            });
            Some((distance, feature))
        })
        .collect();

    features.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let results: Vec<Value> = features.into_iter().map(|(_, f)| f).collect();
    Ok(Value::Array(results))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_includes_all_three_element_types_and_value() {
        let q = build_query("amenity", Some("cafe"), 1000, 51.5, -0.12, 25);
        assert!(q.contains("node[\"amenity\"=\"cafe\"](around:1000,51.5,-0.12)"));
        assert!(q.contains("way[\"amenity\"=\"cafe\"]"));
        assert!(q.contains("relation[\"amenity\"=\"cafe\"]"));
        assert!(q.contains("out center 25;"));
    }

    #[test]
    fn key_only_query_omits_value() {
        let q = build_query("wheelchair", None, 500, 0.0, 0.0, 10);
        assert!(q.contains("node[\"wheelchair\"](around:500,0,0)"));
        assert!(!q.contains('='));
    }

    #[test]
    fn rejects_injection_in_tag() {
        assert!(validate_tag_component("key", "amenity\"]; out;//").is_err());
        assert!(validate_tag_component("key", "amenity").is_ok());
    }

    #[test]
    fn haversine_is_zero_for_same_point() {
        assert!(haversine_m(51.5, -0.12, 51.5, -0.12) < 1e-6);
    }

    #[test]
    fn haversine_london_to_paris_is_about_340km() {
        let d = haversine_m(51.5074, -0.1278, 48.8566, 2.3522);
        assert!((d - 343_000.0).abs() < 10_000.0, "got {} m", d);
    }
}
