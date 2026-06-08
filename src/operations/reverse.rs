// Reverse geocoding via Nominatim `/reverse`.
// https://nominatim.org/release-docs/develop/api/Reverse/

use crate::config::OsmConfig;
use crate::error::{OsmError, Result};
use crate::operations::nominatim::{Place, place_to_json};
use serde_json::Value;

/// Resolve a latitude/longitude to the nearest addressable place.
///
/// `zoom` is the Nominatim detail level (0 = country … 18 = building); when
/// `None`, Nominatim's default of 18 is used. `language` is an optional
/// `Accept-Language` value.
pub async fn reverse(
    client: &reqwest::Client,
    config: &OsmConfig,
    latitude: f64,
    longitude: f64,
    zoom: Option<u32>,
    language: Option<&str>,
) -> Result<Value> {
    let url = format!("{}/reverse", config.nominatim_base());

    let mut params: Vec<(&str, String)> = vec![
        ("lat", latitude.to_string()),
        ("lon", longitude.to_string()),
        ("format", "jsonv2".to_string()),
        ("addressdetails", "1".to_string()),
    ];
    if let Some(z) = zoom {
        params.push(("zoom", z.to_string()));
    }
    if let Some(lang) = language {
        params.push(("accept-language", lang.to_string()));
    }

    let resp = client.get(&url).query(&params).send().await?;
    let status = resp.status();
    if !status.is_success() {
        return Err(
            OsmError::ApiError(format!("Nominatim reverse returned HTTP {}", status)).into(),
        );
    }

    // Reverse returns a single object, or `{"error": "Unable to geocode"}`
    // when there is nothing near the coordinate. Inspect the raw value first so
    // we can map the error sentinel to a structured NotFound.
    let value: Value = resp.json().await?;
    if let Some(err) = value.get("error") {
        let msg = err.as_str().unwrap_or("Unable to geocode");
        return Err(OsmError::NotFound(format!(
            "No address found for ({}, {}): {}",
            latitude, longitude, msg
        ))
        .into());
    }

    let place: Place = serde_json::from_value(value)?;
    Ok(place_to_json(place))
}
