// McpService implementation: wires the OSM operations into the mcp-core
// dispatch loop.

use mcp_core::telemetry::metrics::{self, Label};
use mcp_core::{CallError, McpService, ToolDef, ToolReply, async_trait};
use serde_json::{Value, json};

use crate::config::OsmConfig;
use crate::error::{McpError, OsmError, OsmMcpError};
use crate::operations::{lookup, nearby, reverse, route, search};

/// The MCP service implementation for OpenStreetMap.
///
/// Owns the shared `reqwest::Client` and the OSM endpoint configuration.
/// `McpService::call_tool` dispatches to the appropriate operation module and
/// maps domain errors to the correct `CallError` variant.
pub struct OsmService {
    client: reqwest::Client,
    config: OsmConfig,
}

impl OsmService {
    /// Create a service using the default OSM endpoints.
    pub fn new() -> Self {
        Self::with_config(OsmConfig::default())
    }

    /// Create a service with a specific OSM configuration.
    pub fn with_config(config: OsmConfig) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(config.user_agent.clone())
            // Bound every request: 10 s to establish TCP, 30 s for the full
            // response. The Overpass [timeout:25] is server-side only and does
            // not protect against a stalled TCP connection.
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(30))
            // A `reqwest::Client` only fails to build on TLS backend init,
            // which is an environment problem we cannot recover from at runtime.
            .build()
            .expect("reqwest client builder only fails on TLS backend initialization");
        Self { client, config }
    }
}

impl Default for OsmService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl McpService for OsmService {
    fn tools(&self) -> Vec<ToolDef> {
        vec![
            ToolDef::new(
                "osm_search",
                "Forward geocode: search OpenStreetMap (via Nominatim) for places matching a free-form query, returning coordinates, OSM ids, category/type, a structured address, importance, and bounding box. Use it to find a city, address, or point of interest on the map and get the coordinates to use as a start or destination for directions. Returns up to 'limit' results ordered by relevance.",
                json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Free-form search text. Examples: 'London', 'Eiffel Tower', '1600 Pennsylvania Avenue NW, Washington DC'."
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of results. Range: 1-40 (default: 10).",
                            "minimum": 1,
                            "maximum": 40
                        },
                        "language": {
                            "type": "string",
                            "description": "Preferred language for result names as an Accept-Language value (ISO 639-1). Default: server default ('en'). Example: 'de', 'fr'."
                        },
                        "countrycodes": {
                            "type": "string",
                            "description": "Optional comma-separated ISO 3166-1 alpha-2 country filter. Example: 'gb,fr' limits results to the UK and France."
                        }
                    },
                    "required": ["query"]
                }),
            ),
            ToolDef::new(
                "osm_reverse",
                "Reverse geocode: resolve a latitude/longitude to the nearest addressable place using Nominatim - answer 'what's at these coordinates' or find the nearby address for a GPS location. Returns a single place with its display name, structured address, OSM id, and category/type.",
                json!({
                    "type": "object",
                    "properties": {
                        "latitude": {
                            "type": "number",
                            "description": "Latitude in decimal degrees (WGS84). Range: -90 to 90."
                        },
                        "longitude": {
                            "type": "number",
                            "description": "Longitude in decimal degrees (WGS84). Range: -180 to 180."
                        },
                        "zoom": {
                            "type": "integer",
                            "description": "Level of detail, 0 (country) to 18 (building). Default: 18. Lower values return broader areas (e.g. 10 = city).",
                            "minimum": 0,
                            "maximum": 18
                        },
                        "language": {
                            "type": "string",
                            "description": "Preferred language for the result name as an Accept-Language value (ISO 639-1). Default: 'en'."
                        }
                    },
                    "required": ["latitude", "longitude"]
                }),
            ),
            ToolDef::new(
                "osm_lookup",
                "Look up specific OSM objects by id via Nominatim and return their details (address, coordinates, category/type). Use when you already have OSM ids, for example from a previous osm_search or osm_nearby result.",
                json!({
                    "type": "object",
                    "properties": {
                        "osm_ids": {
                            "type": "string",
                            "description": "Comma-separated OSM ids, each a type prefix N (node), W (way), or R (relation) followed by the numeric id. Up to 50 ids. Example: 'R146656,W104393803,N240109189'."
                        },
                        "language": {
                            "type": "string",
                            "description": "Preferred language for result names as an Accept-Language value (ISO 639-1). Default: 'en'."
                        }
                    },
                    "required": ["osm_ids"]
                }),
            ),
            ToolDef::new(
                "osm_nearby",
                "Find nearby places, amenities, and points of interest tagged with a given key (optionally key=value) within a radius of a coordinate, using the Overpass API. Returns features (nodes/ways/relations) sorted nearest-first with name, coordinates, distance in meters, and all tags. Returns an empty array when nothing matches. Example: find cafes near a point with key='amenity', value='cafe'.",
                json!({
                    "type": "object",
                    "properties": {
                        "latitude": {
                            "type": "number",
                            "description": "Center latitude in decimal degrees (WGS84)."
                        },
                        "longitude": {
                            "type": "number",
                            "description": "Center longitude in decimal degrees (WGS84)."
                        },
                        "key": {
                            "type": "string",
                            "description": "OSM tag key to match. Examples: 'amenity', 'shop', 'tourism', 'highway'."
                        },
                        "value": {
                            "type": "string",
                            "description": "Optional OSM tag value. When omitted, matches any feature that has the key. Examples: 'cafe', 'restaurant', 'supermarket'."
                        },
                        "radius": {
                            "type": "integer",
                            "description": "Search radius in meters. Range: 1-50000 (default: 1000).",
                            "minimum": 1,
                            "maximum": 50000
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of features to return. Range: 1-200 (default: 25).",
                            "minimum": 1,
                            "maximum": 200
                        }
                    },
                    "required": ["latitude", "longitude", "key"]
                }),
            ),
            ToolDef::new(
                "osm_route",
                "Get driving directions and travel time between two or more coordinates using OSRM. Returns total distance (meters) and travel time (duration in seconds), the route geometry as GeoJSON, snapped waypoints, and per-leg details with optional turn-by-turn navigation steps. Supports 'driving', 'walking', and 'cycling' profiles (the public OSRM demo server primarily supports 'driving').",
                json!({
                    "type": "object",
                    "properties": {
                        "coordinates": {
                            "type": "array",
                            "description": "Ordered list of at least two waypoints, each an object with 'latitude' and 'longitude' in decimal degrees. The route visits them in order.",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "latitude": { "type": "number" },
                                    "longitude": { "type": "number" }
                                },
                                "required": ["latitude", "longitude"]
                            },
                            "minItems": 2
                        },
                        "profile": {
                            "type": "string",
                            "description": "Travel profile: 'driving', 'walking', or 'cycling'. Default: 'driving'.",
                            "enum": ["driving", "walking", "cycling"]
                        },
                        "steps": {
                            "type": "boolean",
                            "description": "Include turn-by-turn step instructions in each leg. Default: false."
                        }
                    },
                    "required": ["coordinates"]
                }),
            ),
        ]
    }

    async fn call_tool(&self, name: &str, args: &Value) -> Result<ToolReply, CallError> {
        match name {
            "osm_search" => self.call_search(args).await,
            "osm_reverse" => self.call_reverse(args).await,
            "osm_lookup" => self.call_lookup(args).await,
            "osm_nearby" => self.call_nearby(args).await,
            "osm_route" => self.call_route(args).await,
            other => Err(CallError::tool(format!("unknown tool: {other}"))),
        }
    }
}

impl OsmService {
    // `args` carries the query, coordinate, tag, or id and is skipped: a tool
    // argument is content, so it must never become a span field (D10). The
    // span still gives this handler's own work its own timing, nested under
    // mcp-core's `mcp.tools.call` span.
    #[tracing::instrument(skip(self, args))]
    async fn call_search(&self, args: &Value) -> Result<ToolReply, CallError> {
        let query = require_str(args, "query")?;
        let limit = get_u64(args, "limit").unwrap_or(10) as u32;
        let language = get_str(args, "language");
        let countrycodes = get_str(args, "countrycodes");

        let outcome = search::search(
            &self.client,
            &self.config,
            query,
            limit,
            language,
            countrycodes,
        )
        .await;
        record_upstream_failure("osm_search", &outcome);
        let result = outcome.map_err(osm_to_call_error)?;

        Ok(ToolReply::json(&result)?)
    }

    #[tracing::instrument(skip(self, args))]
    async fn call_reverse(&self, args: &Value) -> Result<ToolReply, CallError> {
        let latitude = require_f64(args, "latitude")?;
        let longitude = require_f64(args, "longitude")?;
        validate_coord(latitude, longitude)?;
        let zoom = get_u64(args, "zoom").map(|z| z as u32);
        let language = get_str(args, "language");

        let outcome = reverse::reverse(
            &self.client,
            &self.config,
            latitude,
            longitude,
            zoom,
            language,
        )
        .await;
        record_upstream_failure("osm_reverse", &outcome);
        let result = outcome.map_err(osm_to_call_error)?;

        Ok(ToolReply::json(&result)?)
    }

    #[tracing::instrument(skip(self, args))]
    async fn call_lookup(&self, args: &Value) -> Result<ToolReply, CallError> {
        let osm_ids = require_str(args, "osm_ids")?;
        let language = get_str(args, "language");

        let outcome = lookup::lookup(&self.client, &self.config, osm_ids, language).await;
        record_upstream_failure("osm_lookup", &outcome);
        let result = outcome.map_err(osm_to_call_error)?;

        Ok(ToolReply::json(&result)?)
    }

    #[tracing::instrument(skip(self, args))]
    async fn call_nearby(&self, args: &Value) -> Result<ToolReply, CallError> {
        let latitude = require_f64(args, "latitude")?;
        let longitude = require_f64(args, "longitude")?;
        validate_coord(latitude, longitude)?;
        let key = require_str(args, "key")?;
        let value = get_str(args, "value");
        let radius = get_u64(args, "radius").unwrap_or(1000) as u32;
        let limit = get_u64(args, "limit").unwrap_or(25) as u32;

        let outcome = nearby::nearby(
            &self.client,
            &self.config,
            latitude,
            longitude,
            radius,
            key,
            value,
            limit,
        )
        .await;
        record_upstream_failure("osm_nearby", &outcome);
        let result = outcome.map_err(osm_to_call_error)?;

        Ok(ToolReply::json(&result)?)
    }

    #[tracing::instrument(skip(self, args))]
    async fn call_route(&self, args: &Value) -> Result<ToolReply, CallError> {
        let coordinates = parse_coordinates(args)?;
        for &(lat, lon) in &coordinates {
            validate_coord(lat, lon)?;
        }
        let profile = get_str(args, "profile").unwrap_or("driving");
        let steps = args.get("steps").and_then(Value::as_bool).unwrap_or(false);

        let outcome = route::route(&self.client, &self.config, &coordinates, profile, steps).await;
        record_upstream_failure("osm_route", &outcome);
        let result = outcome.map_err(osm_to_call_error)?;

        Ok(ToolReply::json(&result)?)
    }
}

/// Map an `OsmMcpError` to the appropriate `CallError` variant.
///
/// - Caller-side parameter errors (invalid params, invalid tool params) map to
///   `CallError::InvalidParams` (JSON-RPC -32602).
/// - Everything else (upstream failures, not found, etc.) maps to
///   `CallError::Tool` so the model sees `isError: true` content and can react.
fn osm_to_call_error(e: OsmMcpError) -> CallError {
    match &e {
        OsmMcpError::Osm(OsmError::InvalidParameters(_))
        | OsmMcpError::Mcp(McpError::InvalidToolParameters(_)) => {
            CallError::invalid_params(e.to_string())
        }
        _ => CallError::tool(e.to_string()),
    }
}

/// Classify an `OsmMcpError` into the bounded reason [`record_upstream_failure`]
/// counts, or `None` for a decline this crate's own caller or the upstream
/// service's business logic produced -- rule 8.2 keeps that out of a failure
/// counter.
///
/// Exhaustive over [`OsmMcpError`] (and its nested [`OsmError`] /
/// [`McpError`]), so a new variant forces this classification to be
/// revisited rather than silently landing as "not counted". A
/// `reqwest::Error` splits on `.is_timeout()`, since a stalled upstream and
/// a refused connection point an operator at different causes.
fn upstream_failure_reason(err: &OsmMcpError) -> Option<&'static str> {
    match err {
        OsmMcpError::Osm(OsmError::ApiError(_)) => Some("api_error"),
        OsmMcpError::Http(e) if e.is_timeout() => Some("timeout"),
        OsmMcpError::Http(_) => Some("http_error"),
        OsmMcpError::Json(_) => Some("bad_response"),
        OsmMcpError::Io(_) => Some("io_error"),
        OsmMcpError::Osm(OsmError::NotFound(_))
        | OsmMcpError::Osm(OsmError::InvalidParameters(_))
        | OsmMcpError::Mcp(McpError::InvalidToolParameters(_)) => None,
    }
}

/// Count an upstream failure against `osm.upstream_failures`.
///
/// `tool` is always one of the five `&'static str` literals the call sites
/// above pass, so the label is bounded there rather than by anything a
/// caller supplies; `reason` is bounded the same way, by
/// [`upstream_failure_reason`]'s fixed set of return values. Neither label
/// is ever built from a query, a coordinate, or any other caller-controlled
/// string.
fn record_upstream_failure<T>(tool: &'static str, outcome: &Result<T, OsmMcpError>) {
    if let Err(err) = outcome
        && let Some(reason) = upstream_failure_reason(err)
    {
        metrics::increment(
            "osm.upstream_failures",
            &[Label::new("tool", tool), Label::new("reason", reason)],
        );
    }
}

// ── Argument helpers ──────────────────────────────────────────────────────────

/// Require a non-empty string argument.
fn require_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, CallError> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| CallError::invalid_params(format!("Missing required parameter: {key}")))
}

/// Optional string argument (absent or non-string → `None`).
fn get_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

/// Optional unsigned-integer argument, accepting JSON numbers and numeric strings.
fn get_u64(args: &Value, key: &str) -> Option<u64> {
    let v = args.get(key)?;
    v.as_u64()
        .or_else(|| v.as_f64().map(|f| f.max(0.0) as u64))
        .or_else(|| v.as_str()?.parse::<u64>().ok())
}

/// Require a float argument, accepting JSON numbers and numeric strings.
fn require_f64(args: &Value, key: &str) -> Result<f64, CallError> {
    let v = args
        .get(key)
        .ok_or_else(|| CallError::invalid_params(format!("Missing required parameter: {key}")))?;
    value_as_f64(v)
        .ok_or_else(|| CallError::invalid_params(format!("Parameter '{key}' must be a number")))
}

/// Coerce a JSON value into an `f64`, accepting numbers and numeric strings.
fn value_as_f64(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str()?.parse::<f64>().ok())
}

/// Validate that `(latitude, longitude)` are finite and within WGS84 range.
///
/// Non-finite values (NaN, ±Inf) would be silently embedded in URLs, producing
/// unpredictable upstream behaviour or garbage results.
fn validate_coord(latitude: f64, longitude: f64) -> Result<(), CallError> {
    if !latitude.is_finite() || !longitude.is_finite() {
        return Err(CallError::invalid_params(format!(
            "Coordinates must be finite numbers, got latitude={latitude} longitude={longitude}"
        )));
    }
    if !(-90.0..=90.0).contains(&latitude) {
        return Err(CallError::invalid_params(format!(
            "Latitude must be in [-90, 90], got {latitude}"
        )));
    }
    if !(-180.0..=180.0).contains(&longitude) {
        return Err(CallError::invalid_params(format!(
            "Longitude must be in [-180, 180], got {longitude}"
        )));
    }
    Ok(())
}

/// Parse the `coordinates` array for `osm_route` into `(lat, lon)` pairs.
fn parse_coordinates(args: &Value) -> Result<Vec<(f64, f64)>, CallError> {
    let arr = args
        .get("coordinates")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            CallError::invalid_params("Missing required parameter: coordinates (array)")
        })?;

    let mut coords = Vec::with_capacity(arr.len());
    for (i, item) in arr.iter().enumerate() {
        let lat = item.get("latitude").and_then(value_as_f64).ok_or_else(|| {
            CallError::invalid_params(format!("coordinates[{i}] is missing a numeric 'latitude'"))
        })?;
        let lon = item
            .get("longitude")
            .and_then(value_as_f64)
            .ok_or_else(|| {
                CallError::invalid_params(format!(
                    "coordinates[{i}] is missing a numeric 'longitude'"
                ))
            })?;
        coords.push((lat, lon));
    }
    Ok(coords)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn require_str_rejects_empty_and_missing() {
        let args = json!({ "query": "" });
        assert!(require_str(&args, "query").is_err());
        assert!(require_str(&args, "missing").is_err());
        let ok = json!({ "query": "London" });
        assert_eq!(require_str(&ok, "query").unwrap(), "London");
    }

    #[test]
    fn require_f64_accepts_numbers_and_strings() {
        assert_eq!(
            require_f64(&json!({"latitude": 51.5}), "latitude").unwrap(),
            51.5
        );
        assert_eq!(
            require_f64(&json!({"latitude": "51.5"}), "latitude").unwrap(),
            51.5
        );
        assert!(require_f64(&json!({"latitude": "x"}), "latitude").is_err());
        assert!(require_f64(&json!({}), "latitude").is_err());
    }

    #[test]
    fn parse_coordinates_requires_two_numeric_pairs() {
        let args = json!({
            "coordinates": [
                {"latitude": 51.5, "longitude": -0.12},
                {"latitude": 48.85, "longitude": 2.35}
            ]
        });
        let coords = parse_coordinates(&args).unwrap();
        assert_eq!(coords, vec![(51.5, -0.12), (48.85, 2.35)]);

        let missing = json!({ "coordinates": [{"latitude": 1.0}] });
        assert!(parse_coordinates(&missing).is_err());
    }

    #[test]
    fn validate_coord_accepts_valid() {
        assert!(validate_coord(0.0, 0.0).is_ok());
        assert!(validate_coord(-90.0, -180.0).is_ok());
        assert!(validate_coord(90.0, 180.0).is_ok());
        assert!(validate_coord(51.5, -0.12).is_ok());
    }

    #[test]
    fn validate_coord_rejects_out_of_range() {
        assert!(validate_coord(91.0, 0.0).is_err(), "lat > 90 must fail");
        assert!(validate_coord(-91.0, 0.0).is_err(), "lat < -90 must fail");
        assert!(validate_coord(0.0, 181.0).is_err(), "lon > 180 must fail");
        assert!(validate_coord(0.0, -181.0).is_err(), "lon < -180 must fail");
    }

    #[test]
    fn validate_coord_rejects_non_finite() {
        assert!(validate_coord(f64::NAN, 0.0).is_err(), "NaN lat must fail");
        assert!(
            validate_coord(0.0, f64::INFINITY).is_err(),
            "Inf lon must fail"
        );
        assert!(
            validate_coord(f64::NEG_INFINITY, 0.0).is_err(),
            "-Inf lat must fail"
        );
    }

    #[test]
    fn tools_list_has_five_tools() {
        let svc = OsmService::new();
        let tools = svc.tools();
        assert_eq!(tools.len(), 5);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        for expected in [
            "osm_search",
            "osm_reverse",
            "osm_lookup",
            "osm_nearby",
            "osm_route",
        ] {
            assert!(names.contains(&expected), "missing tool {expected}");
        }
    }

    #[test]
    fn integer_schema_types_for_integer_params() {
        let svc = OsmService::new();
        let tools = svc.tools();
        let find = |name: &str| {
            tools
                .iter()
                .find(|t| t.name == name)
                .unwrap()
                .input_schema
                .clone()
        };

        let search = find("osm_search");
        assert_eq!(search["properties"]["limit"]["type"], "integer");

        let reverse = find("osm_reverse");
        assert_eq!(reverse["properties"]["zoom"]["type"], "integer");

        let nearby = find("osm_nearby");
        assert_eq!(nearby["properties"]["radius"]["type"], "integer");
        assert_eq!(nearby["properties"]["limit"]["type"], "integer");
    }

    /// Natural-language phrases a user is likely to type when looking for
    /// navigation, geography, or nearby-place help. On the FTS-only
    /// tool-discovery fallback (empty/NULL embeddings), terse descriptions rank
    /// poorly against these, so every natural-language-discovery OSM tool
    /// (route, forward/reverse geocode, nearby - not the id-based osm_lookup)
    /// must surface at least one. Refs adelie-ai/desktop-assistant#502.
    const NATURAL_SEARCH_TERMS: [&str; 5] = [
        "directions",
        "travel time",
        "distance",
        "navigation",
        "nearby",
    ];

    /// Fetch a tool's description (lowercased) by name, or panic if absent.
    fn tool_description(name: &str) -> String {
        OsmService::new()
            .tools()
            .into_iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| panic!("tool {name} not advertised"))
            .description
            .to_lowercase()
    }

    #[test]
    fn osm_route_description_mentions_driving_directions() {
        let desc = tool_description("osm_route");
        assert!(
            desc.contains("driving directions"),
            "osm_route description should mention 'driving directions' for FTS discovery, got: {desc}"
        );
    }

    #[test]
    fn osm_route_description_mentions_travel_time() {
        let desc = tool_description("osm_route");
        assert!(
            desc.contains("travel time"),
            "osm_route description should mention 'travel time' for FTS discovery, got: {desc}"
        );
    }

    #[test]
    fn osm_tools_descriptions_contain_natural_search_terms() {
        // Parameterized over the route, geocode (forward + reverse), and nearby
        // tools, the ones a user reaches for with natural phrasing.
        for tool in ["osm_route", "osm_search", "osm_reverse", "osm_nearby"] {
            let desc = tool_description(tool);
            assert!(
                NATURAL_SEARCH_TERMS.iter().any(|term| desc.contains(term)),
                "{tool} description should contain at least one natural search term \
                 {NATURAL_SEARCH_TERMS:?}, got: {desc}"
            );
        }
    }

    // ── Telemetry: the upstream-failure metric (mcp-core#40) ───────────────

    /// Every genuine upstream fault must be classified with a bounded
    /// reason; every decline (a business "not found", or a caller/operator
    /// rejection) must classify to `None` so it never inflates a failure
    /// counter (rule 8.2). Exhaustive over `OsmMcpError` (and its nested
    /// `OsmError` / `McpError`), so a new variant forces this classification
    /// to be revisited rather than silently landing as "not counted".
    #[test]
    fn upstream_failure_reason_classifies_every_osm_mcp_error_variant() {
        assert_eq!(
            upstream_failure_reason(&OsmMcpError::Osm(OsmError::ApiError("HTTP 500".into()))),
            Some("api_error"),
            "a non-2xx upstream response is a fault, not a decline"
        );
        assert_eq!(
            upstream_failure_reason(&OsmMcpError::Json(
                serde_json::from_str::<Value>("not json").unwrap_err()
            )),
            Some("bad_response"),
            "an upstream body that fails to deserialize is a fault"
        );
        assert_eq!(
            upstream_failure_reason(&OsmMcpError::Io(std::io::Error::other("body read failed"))),
            Some("io_error"),
            "a body-read failure is a fault"
        );

        assert_eq!(
            upstream_failure_reason(&OsmMcpError::Osm(OsmError::NotFound("nothing here".into()))),
            None,
            "no result found is a normal business outcome, not a fault"
        );
        assert_eq!(
            upstream_failure_reason(&OsmMcpError::Osm(OsmError::InvalidParameters(
                "bad tag".into()
            ))),
            None,
            "a rejected caller parameter is a decline, not an upstream fault"
        );
        assert_eq!(
            upstream_failure_reason(&OsmMcpError::Mcp(McpError::InvalidToolParameters(
                "missing query".into()
            ))),
            None,
            "a rejected tool parameter is a decline, not an upstream fault"
        );
    }

    /// `reqwest::Error` carries no public constructor, so this drives the
    /// timeout/other split through a real (local, fast) network failure:
    /// connecting to a closed port refuses immediately rather than timing
    /// out, so it must classify as `http_error`, not `timeout`.
    #[test]
    fn upstream_failure_reason_classifies_a_connection_refusal_as_http_error() {
        let rt = tokio::runtime::Runtime::new().expect("a tokio runtime");
        let err = rt.block_on(async {
            reqwest::Client::new()
                .get("http://127.0.0.1:1/")
                .send()
                .await
                .expect_err("port 1 refuses the connection")
        });
        assert!(!err.is_timeout(), "a connection refusal is not a timeout");
        assert_eq!(
            upstream_failure_reason(&OsmMcpError::Http(err)),
            Some("http_error"),
            "a non-timeout transport failure is a fault, classified as http_error"
        );
    }

    // The metrics registry [`mcp_core::telemetry::metrics`] records into is
    // process-global, and `cargo test` runs a file's tests concurrently by
    // default. This guards every test in this module that touches the
    // registry so they run one at a time relative to each other; it holds no
    // data of its own.
    static METRICS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn lock_metrics() -> std::sync::MutexGuard<'static, ()> {
        METRICS_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn record_upstream_failure_increments_only_for_counted_reasons() {
        let _guard = lock_metrics();
        use mcp_core::telemetry::metrics::{self, Label};

        let labels = [
            Label::new("tool", "osm_search"),
            Label::new("reason", "api_error"),
        ];
        let before = counter_total("osm.upstream_failures", &labels);

        let ok: std::result::Result<Value, OsmMcpError> = Ok(json!([]));
        record_upstream_failure("osm_search", &ok);
        let decline: std::result::Result<Value, OsmMcpError> =
            Err(OsmMcpError::Osm(OsmError::NotFound("x".into())));
        record_upstream_failure("osm_search", &decline);
        assert_eq!(
            counter_total("osm.upstream_failures", &labels),
            before,
            "a successful call or a decline must not move the counter"
        );

        let fault: std::result::Result<Value, OsmMcpError> =
            Err(OsmMcpError::Osm(OsmError::ApiError("HTTP 500".into())));
        record_upstream_failure("osm_search", &fault);
        assert_eq!(
            counter_total("osm.upstream_failures", &labels),
            before + 1,
            "an upstream fault must increment the counter, labelled by tool and reason"
        );

        fn counter_total(name: &str, labels: &[Label]) -> u64 {
            metrics::global()
                .snapshot()
                .counters
                .iter()
                .find(|counter| counter.name == name && same_labels(&counter.labels, labels))
                .map_or(0, |counter| counter.total)
        }

        fn same_labels(recorded: &[Label], wanted: &[Label]) -> bool {
            recorded.len() == wanted.len()
                && wanted.iter().all(|want| {
                    recorded
                        .iter()
                        .any(|have| have.key() == want.key() && have.value() == want.value())
                })
        }
    }
}
