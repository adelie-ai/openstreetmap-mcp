// Library crate for openstreetmap-mcp

pub mod config;
pub mod error;
pub mod operations;
pub mod service;

#[cfg(test)]
mod server_config_tests {
    use super::*;

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
