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

    log_search_request(&url, query, limit);
    let resp = client.get(&url).query(&params).send().await?;
    let status = resp.status();
    if !status.is_success() {
        return Err(
            OsmError::ApiError(format!("Nominatim search returned HTTP {}", status)).into(),
        );
    }

    let places: Vec<Place> = resp.json().await?;
    // Return an empty array when Nominatim finds nothing, matching osm_nearby
    // behaviour. An empty result is a valid answer; an LLM can branch on it
    // without special-casing an error variant.
    let results: Vec<Value> = places.into_iter().map(place_to_json).collect();
    Ok(Value::Array(results))
}

/// Log that a Nominatim `/search` request is starting.
///
/// `query` is a tool argument -- content, never an id -- so it stays at
/// DEBUG and is never attached to a span (a span field would leave the
/// process with `otel` on regardless of level). Kept as its own function so
/// a test can drive it directly, without a real network call.
fn log_search_request(url: &str, query: &str, limit: u32) {
    tracing::debug!(url, query, limit, "querying nominatim search");
}

#[cfg(test)]
mod tests {
    use crate::operations::nominatim::{Place, place_to_json};
    use crate::operations::test_capture::capture_events;
    use serde_json::Value;

    #[test]
    fn empty_search_returns_empty_array_not_error() {
        // Verify that the empty-result path produces an empty JSON array rather
        // than an OsmError::NotFound. We test the mapping directly because the
        // Nominatim network call is not mocked in unit tests.
        let empty: Vec<Place> = vec![];
        let results: Vec<Value> = empty.into_iter().map(place_to_json).collect();
        let out = Value::Array(results);
        assert_eq!(out, Value::Array(vec![]));
    }

    /// mcp-core#40: `query` is a tool argument -- content, never an id -- so
    /// the per-request log must stay at DEBUG, carrying the exact query, and
    /// nothing else must log while `search` starts a request.
    #[test]
    fn log_search_request_puts_the_query_at_debug_only() {
        const SENTINEL: &str = "MARKER-osm-search-9f3d1c2a";
        let events =
            capture_events(|| super::log_search_request("https://example.com/search", SENTINEL, 7));

        assert_eq!(
            events.len(),
            1,
            "querying nominatim search must log exactly one event: {events:?}"
        );
        let event = &events[0];
        assert_eq!(
            event.level,
            tracing::Level::DEBUG,
            "the outbound search request must log at DEBUG, so it stays off the INFO band"
        );
        assert_eq!(
            event.fields.get("query").map(String::as_str),
            Some(SENTINEL),
            "the event must carry the query that was searched for: {event:?}"
        );
        assert_eq!(
            event.fields.get("limit").map(String::as_str),
            Some("7"),
            "the event must carry the effective limit: {event:?}"
        );
    }
}
