#![deny(warnings)]

// Look up specific OSM objects by id via Nominatim `/lookup`.
// https://nominatim.org/release-docs/develop/api/Lookup/

use crate::config::OsmConfig;
use crate::error::{OsmError, Result};
use crate::operations::nominatim::{Place, place_to_json};
use serde_json::Value;

/// Nominatim accepts at most 50 ids per lookup request.
const MAX_IDS: usize = 50;

/// Validate the `osm_ids` string: a comma-separated list of tokens, each a
/// single type prefix (`N`/`W`/`R`, case-insensitive) followed by digits, e.g.
/// `"R146656,W104393803,N240109189"`.
///
/// Why validate here: Nominatim silently ignores malformed ids, so a typo would
/// otherwise surface as a confusing "not found" rather than a clear parameter
/// error.
fn validate_osm_ids(osm_ids: &str) -> Result<()> {
    let tokens: Vec<&str> = osm_ids.split(',').map(str::trim).collect();
    if tokens.iter().any(|t| t.is_empty()) {
        return Err(OsmError::InvalidParameters(
            "osm_ids must not contain empty entries".to_string(),
        )
        .into());
    }
    if tokens.len() > MAX_IDS {
        return Err(OsmError::InvalidParameters(format!(
            "osm_ids accepts at most {} ids per request, got {}",
            MAX_IDS,
            tokens.len()
        ))
        .into());
    }
    for t in &tokens {
        let mut chars = t.chars();
        let prefix = chars.next().unwrap_or(' ').to_ascii_uppercase();
        let rest: String = chars.collect();
        let prefix_ok = matches!(prefix, 'N' | 'W' | 'R');
        let digits_ok = !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit());
        if !prefix_ok || !digits_ok {
            return Err(OsmError::InvalidParameters(format!(
                "invalid OSM id '{}': expected a N/W/R prefix followed by digits (e.g. N240109189)",
                t
            ))
            .into());
        }
    }
    Ok(())
}

/// Look up one or more OSM objects by id and return their details.
pub async fn lookup(
    client: &reqwest::Client,
    config: &OsmConfig,
    osm_ids: &str,
    language: Option<&str>,
) -> Result<Value> {
    validate_osm_ids(osm_ids)?;
    let url = format!("{}/lookup", config.nominatim_base());

    let mut params: Vec<(&str, String)> = vec![
        ("osm_ids", osm_ids.to_string()),
        ("format", "jsonv2".to_string()),
        ("addressdetails", "1".to_string()),
    ];
    if let Some(lang) = language {
        params.push(("accept-language", lang.to_string()));
    }

    let resp = client.get(&url).query(&params).send().await?;
    let status = resp.status();
    if !status.is_success() {
        return Err(
            OsmError::ApiError(format!("Nominatim lookup returned HTTP {}", status)).into(),
        );
    }

    let places: Vec<Place> = resp.json().await?;
    if places.is_empty() {
        return Err(
            OsmError::NotFound(format!("No objects found for osm_ids: {}", osm_ids)).into(),
        );
    }

    let results: Vec<Value> = places.into_iter().map(place_to_json).collect();
    Ok(Value::Array(results))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_well_formed_ids() {
        assert!(validate_osm_ids("R146656,W104393803,N240109189").is_ok());
        assert!(validate_osm_ids("n240109189").is_ok());
    }

    #[test]
    fn rejects_bad_prefix_or_missing_digits() {
        assert!(validate_osm_ids("X123").is_err());
        assert!(validate_osm_ids("N").is_err());
        assert!(validate_osm_ids("123").is_err());
        assert!(validate_osm_ids("N123,,W456").is_err());
    }
}
