// Library crate for openstreetmap-mcp

use mcp_core::ServerConfig;

pub mod config;
pub mod error;
pub mod operations;
pub mod service;

pub use service::OsmService;

/// Server-level MCP `instructions` blurb, returned in the `initialize` response.
///
/// Why: the daemon captures this string and uses it as the server's searchable
/// description, so it is the server-grain hint for *what this server is for* and
/// *when to reach for it* during tool discovery. Keep it honest about the actual
/// tools (`osm_search`, `osm_reverse`, `osm_lookup`, `osm_nearby`, `osm_route`)
/// and the free public-endpoint usage constraints - do not over-claim.
pub const SERVER_INSTRUCTIONS: &str = "OpenStreetMap location intelligence over free public OSM services (Nominatim, Overpass, OSRM) with no API key or setup required: geocoding, nearby-place search, and routing. Reach for this whenever a request involves a place or address - finding the coordinates of a city, landmark, or address (osm_search), identifying what is at a latitude/longitude (osm_reverse), listing nearby amenities or points of interest such as cafes, restaurants, or shops within a radius (osm_nearby), getting driving, walking, or cycling directions with distance and travel time between locations (osm_route), or resolving known OSM object ids (osm_lookup). A common pattern is to osm_search for a place first to obtain its coordinates, then feed those into osm_nearby or osm_route. Coordinates are decimal-degree latitude/longitude (WGS84), and the public endpoints are rate-limited, so reuse results rather than issuing many rapid calls.";

/// Build the mcp-core [`ServerConfig`] for this server: name, version, the
/// [`SERVER_INSTRUCTIONS`] blurb, and the transport policy (no websocket).
///
/// Why a helper: keeps the server-level configuration in the library so it is
/// unit-testable, with `main` reduced to wiring it to the runtime.
pub fn server_config() -> ServerConfig {
    ServerConfig::new("openstreetmap-mcp", env!("CARGO_PKG_VERSION"))
        .without_websocket()
        .instructions(SERVER_INSTRUCTIONS)
}

/// Construct the OpenStreetMap service with built-in defaults (public Nominatim/Overpass/OSRM,
/// default user-agent), for in-process (compiled-in) hosting.
pub fn build_service() -> OsmService {
    OsmService::new()
}

#[cfg(test)]
mod server_config_tests {
    use super::*;

    #[test]
    fn build_service_exposes_tools() {
        use mcp_core::McpService;
        let svc = build_service();
        assert!(
            !svc.tools().is_empty(),
            "osm build_service() must expose at least one tool"
        );
    }

    #[test]
    fn server_config_exposes_nonempty_instructions() {
        let config = server_config();
        let instructions = config
            .instructions
            .expect("server_config must set an MCP instructions blurb");
        assert!(
            !instructions.trim().is_empty(),
            "server instructions must be non-empty"
        );
    }

    #[test]
    fn server_instructions_name_every_tool() {
        let lower = SERVER_INSTRUCTIONS.to_lowercase();
        for tool in [
            "osm_search",
            "osm_reverse",
            "osm_lookup",
            "osm_nearby",
            "osm_route",
        ] {
            assert!(
                lower.contains(tool),
                "server instructions should name {tool} so the model learns the key tools, \
                 got: {SERVER_INSTRUCTIONS}"
            );
        }
    }

    #[test]
    fn server_instructions_describe_purpose_and_usage() {
        let lower = SERVER_INSTRUCTIONS.to_lowercase();
        assert!(
            lower.contains("openstreetmap"),
            "server instructions should name OpenStreetMap, got: {SERVER_INSTRUCTIONS}"
        );
        // Natural discovery terms spanning the server's capabilities.
        for term in ["geocod", "nearby", "rout", "directions"] {
            assert!(
                lower.contains(term),
                "server instructions should mention '{term}' for discovery, \
                 got: {SERVER_INSTRUCTIONS}"
            );
        }
        // Critical usage note: the public endpoints need no API key.
        assert!(
            lower.contains("api key"),
            "server instructions should note the no-API-key usage detail, \
             got: {SERVER_INSTRUCTIONS}"
        );
    }
}
