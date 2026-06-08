#![deny(warnings)]

// Tool registry and MCP tool definitions.

use crate::config::OsmConfig;
use crate::error::{McpError, Result};
use crate::operations::{lookup, nearby, reverse, route, search};
use serde_json::Value;

/// Tool registry that owns the shared HTTP client and OSM configuration and
/// dispatches MCP tool calls to the matching operation.
pub struct ToolRegistry {
    client: reqwest::Client,
    config: OsmConfig,
}

impl ToolRegistry {
    /// Create a registry using the default OSM endpoints.
    pub fn new() -> Self {
        Self::with_config(OsmConfig::default())
    }

    /// Create a registry with a specific OSM configuration. The configured
    /// `user_agent` is baked into the HTTP client, satisfying the Nominatim
    /// usage policy's identification requirement.
    pub fn with_config(config: OsmConfig) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(config.user_agent.clone())
            // A `reqwest::Client` only fails to build on TLS backend init,
            // which is an environment problem we cannot recover from at runtime.
            .build()
            .expect("reqwest client builder only fails on TLS backend initialization");
        Self { client, config }
    }

    /// Get all tools in MCP format.
    pub fn list_tools(&self) -> Value {
        serde_json::json!([
            {
                "name": "osm_search",
                "description": "Forward geocode: search OpenStreetMap (via Nominatim) for places matching a free-form query and return matching results with coordinates, OSM ids, category/type, a structured address, importance, and bounding box. Use for cities, addresses, and points of interest. Returns up to 'limit' results ordered by relevance.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Free-form search text. Examples: 'London', 'Eiffel Tower', '1600 Pennsylvania Avenue NW, Washington DC'."
                        },
                        "limit": {
                            "type": "number",
                            "description": "Maximum number of results. Range: 1-40 (default: 10)."
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
                }
            },
            {
                "name": "osm_reverse",
                "description": "Reverse geocode: resolve a latitude/longitude to the nearest addressable place using Nominatim. Returns a single place with its display name, structured address, OSM id, and category/type.",
                "inputSchema": {
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
                            "type": "number",
                            "description": "Level of detail, 0 (country) to 18 (building). Default: 18. Lower values return broader areas (e.g. 10 = city)."
                        },
                        "language": {
                            "type": "string",
                            "description": "Preferred language for the result name as an Accept-Language value (ISO 639-1). Default: 'en'."
                        }
                    },
                    "required": ["latitude", "longitude"]
                }
            },
            {
                "name": "osm_lookup",
                "description": "Look up specific OSM objects by id via Nominatim and return their details (address, coordinates, category/type). Use when you already have OSM ids, for example from a previous osm_search or osm_nearby result.",
                "inputSchema": {
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
                }
            },
            {
                "name": "osm_nearby",
                "description": "Find OpenStreetMap features tagged with a given key (optionally key=value) within a radius of a coordinate, using the Overpass API. Returns features (nodes/ways/relations) sorted nearest-first with name, coordinates, distance in meters, and all tags. Returns an empty array when nothing matches. Example: find cafes near a point with key='amenity', value='cafe'.",
                "inputSchema": {
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
                            "type": "number",
                            "description": "Search radius in meters. Range: 1-50000 (default: 1000)."
                        },
                        "limit": {
                            "type": "number",
                            "description": "Maximum number of features to return. Range: 1-200 (default: 25)."
                        }
                    },
                    "required": ["latitude", "longitude", "key"]
                }
            },
            {
                "name": "osm_route",
                "description": "Compute a route between two or more coordinates using OSRM. Returns total distance (meters) and duration (seconds), the route geometry as GeoJSON, snapped waypoints, and per-leg details (with optional turn-by-turn steps). Note the public OSRM demo server primarily supports the 'driving' profile.",
                "inputSchema": {
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
                }
            }
        ])
    }

    /// Execute a tool call by name with the given arguments.
    pub async fn execute_tool(&self, tool_name: &str, arguments: &Value) -> Result<Value> {
        match tool_name {
            "osm_search" => self.execute_search(arguments).await,
            "osm_reverse" => self.execute_reverse(arguments).await,
            "osm_lookup" => self.execute_lookup(arguments).await,
            "osm_nearby" => self.execute_nearby(arguments).await,
            "osm_route" => self.execute_route(arguments).await,
            _ => Err(McpError::ToolNotFound(tool_name.to_string()).into()),
        }
    }

    async fn execute_search(&self, args: &Value) -> Result<Value> {
        let query = require_str(args, "query")?;
        let limit = get_u64(args, "limit").unwrap_or(10) as u32;
        let language = get_str(args, "language");
        let countrycodes = get_str(args, "countrycodes");

        let result = search::search(
            &self.client,
            &self.config,
            query,
            limit,
            language,
            countrycodes,
        )
        .await?;
        Ok(mcp_tool_result_json(result))
    }

    async fn execute_reverse(&self, args: &Value) -> Result<Value> {
        let latitude = require_f64(args, "latitude")?;
        let longitude = require_f64(args, "longitude")?;
        let zoom = get_u64(args, "zoom").map(|z| z as u32);
        let language = get_str(args, "language");

        let result = reverse::reverse(
            &self.client,
            &self.config,
            latitude,
            longitude,
            zoom,
            language,
        )
        .await?;
        Ok(mcp_tool_result_json(result))
    }

    async fn execute_lookup(&self, args: &Value) -> Result<Value> {
        let osm_ids = require_str(args, "osm_ids")?;
        let language = get_str(args, "language");

        let result = lookup::lookup(&self.client, &self.config, osm_ids, language).await?;
        Ok(mcp_tool_result_json(result))
    }

    async fn execute_nearby(&self, args: &Value) -> Result<Value> {
        let latitude = require_f64(args, "latitude")?;
        let longitude = require_f64(args, "longitude")?;
        let key = require_str(args, "key")?;
        let value = get_str(args, "value");
        let radius = get_u64(args, "radius").unwrap_or(1000) as u32;
        let limit = get_u64(args, "limit").unwrap_or(25) as u32;

        let result = nearby::nearby(
            &self.client,
            &self.config,
            latitude,
            longitude,
            radius,
            key,
            value,
            limit,
        )
        .await?;
        Ok(mcp_tool_result_json(result))
    }

    async fn execute_route(&self, args: &Value) -> Result<Value> {
        let coordinates = parse_coordinates(args)?;
        let profile = get_str(args, "profile").unwrap_or("driving");
        let steps = args.get("steps").and_then(Value::as_bool).unwrap_or(false);

        let result = route::route(&self.client, &self.config, &coordinates, profile, steps).await?;
        Ok(mcp_tool_result_json(result))
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Require a non-empty string argument.
fn require_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            McpError::InvalidToolParameters(format!("Missing required parameter: {}", key)).into()
        })
}

/// Optional string argument (absent or non-string → `None`).
fn get_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

/// Optional unsigned-integer argument, accepting JSON numbers and numeric
/// strings.
fn get_u64(args: &Value, key: &str) -> Option<u64> {
    let v = args.get(key)?;
    v.as_u64()
        .or_else(|| v.as_f64().map(|f| f.max(0.0) as u64))
        .or_else(|| v.as_str()?.parse::<u64>().ok())
}

/// Require a float argument, accepting JSON numbers and numeric strings.
fn require_f64(args: &Value, key: &str) -> Result<f64> {
    let v = args.get(key).ok_or_else(|| {
        McpError::InvalidToolParameters(format!("Missing required parameter: {}", key))
    })?;
    value_as_f64(v).ok_or_else(|| {
        McpError::InvalidToolParameters(format!("Parameter '{}' must be a number", key)).into()
    })
}

/// Coerce a JSON value into an `f64`, accepting numbers and numeric strings.
fn value_as_f64(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str()?.parse::<f64>().ok())
}

/// Parse the `coordinates` array for `osm_route` into `(lat, lon)` pairs.
/// Each element must be an object with numeric `latitude` and `longitude`.
fn parse_coordinates(args: &Value) -> Result<Vec<(f64, f64)>> {
    let arr = args
        .get("coordinates")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            McpError::InvalidToolParameters(
                "Missing required parameter: coordinates (array)".to_string(),
            )
        })?;

    let mut coords = Vec::with_capacity(arr.len());
    for (i, item) in arr.iter().enumerate() {
        let lat = item.get("latitude").and_then(value_as_f64).ok_or_else(|| {
            McpError::InvalidToolParameters(format!(
                "coordinates[{}] is missing a numeric 'latitude'",
                i
            ))
        })?;
        let lon = item
            .get("longitude")
            .and_then(value_as_f64)
            .ok_or_else(|| {
                McpError::InvalidToolParameters(format!(
                    "coordinates[{}] is missing a numeric 'longitude'",
                    i
                ))
            })?;
        coords.push((lat, lon));
    }
    Ok(coords)
}

/// Wrap a JSON value in the MCP tool-result content envelope.
fn mcp_tool_result_json(value: Value) -> Value {
    serde_json::json!({
        "content": [
            {
                "type": "json",
                "value": value,
            }
        ]
    })
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
    fn list_tools_exposes_the_five_osm_tools() {
        let registry = ToolRegistry::new();
        let tools = registry.list_tools();
        let names: Vec<&str> = tools
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t.get("name").and_then(Value::as_str))
            .collect();
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
}
