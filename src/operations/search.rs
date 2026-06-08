#![deny(warnings)]

// Forward geocoding via Nominatim `/search`.
// https://nominatim.org/release-docs/develop/api/Search/

use crate::config::OsmConfig;
use crate::error::{OsmError, Result};
use crate::operations::nominatim::{Place, place_to_json};
use serde_json::Value;

/// Maximum results Nominatim's `/search` will return for a single request.
const MAX_LIMIT: u32 = 40;

/// Search for places matching a free-form query and return matching results.
///
/// `limit` is clamped to `1..=40`. `language` is an optional
/// `Accept-Language` value (e.g. `"de"`, `"fr"`). `countrycodes` is an optional
/// comma-separated ISO 3166-1 alpha-2 filter (e.g. `"gb,fr"`).
pub async fn search(
    client: &reqwest::Client,
    config: &OsmConfig,
    query: &str,
    limit: u32,
    language: Option<&str>,
    countrycodes: Option<&str>,
) -> Result<Value> {
    let limit = limit.clamp(1, MAX_LIMIT);
    let url = format!("{}/search", config.nominatim_base());

    let mut params: Vec<(&str, String)> = vec![
        ("q", query.to_string()),
        ("format", "jsonv2".to_string()),
        ("addressdetails", "1".to_string()),
        ("limit", limit.to_string()),
    ];
    if let Some(lang) = language {
        params.push(("accept-language", lang.to_string()));
    }
    if let Some(cc) = countrycodes {
        params.push(("countrycodes", cc.to_string()));
    }

    let resp = client.get(&url).query(&params).send().await?;
    let status = resp.status();
    if !status.is_success() {
        return Err(
            OsmError::ApiError(format!("Nominatim search returned HTTP {}", status)).into(),
        );
    }

    let places: Vec<Place> = resp.json().await?;
    if places.is_empty() {
        return Err(OsmError::NotFound(format!("No places found for query: {}", query)).into());
    }

    let results: Vec<Value> = places.into_iter().map(place_to_json).collect();
    Ok(Value::Array(results))
}
