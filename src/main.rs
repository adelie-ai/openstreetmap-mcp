// Binary crate for openstreetmap-mcp.
//
// All JSON-RPC dispatch, transport framing, and CLI plumbing is owned by
// mcp-core.  This file only wires the server-specific configuration flags into
// the OsmService and hands it off.

use clap::Args;
use mcp_core::{ServerConfig, run};
use openstreetmap_mcp::config::{
    DEFAULT_NOMINATIM_URL, DEFAULT_OSRM_URL, DEFAULT_OVERPASS_URL, DEFAULT_USER_AGENT, OsmConfig,
};
use openstreetmap_mcp::service::OsmService;

/// Server-specific flags flattened into mcp-core's `serve` subcommand.
#[derive(Args)]
struct OsmArgs {
    /// Nominatim base URL (geocoding / reverse / lookup).
    #[arg(long, env = "OSM_NOMINATIM_URL", default_value = DEFAULT_NOMINATIM_URL)]
    nominatim_url: String,

    /// Overpass API interpreter URL (feature queries).
    #[arg(long, env = "OSM_OVERPASS_URL", default_value = DEFAULT_OVERPASS_URL)]
    overpass_url: String,

    /// OSRM routing base URL.
    #[arg(long, env = "OSM_OSRM_URL", default_value = DEFAULT_OSRM_URL)]
    osrm_url: String,

    /// User-Agent sent to OSM services. The Nominatim usage policy requires
    /// a descriptive, contactful value when using the public endpoint.
    #[arg(long, env = "OSM_USER_AGENT", default_value = DEFAULT_USER_AGENT)]
    user_agent: String,
}

#[tokio::main]
async fn main() -> mcp_core::Result<()> {
    let config =
        ServerConfig::new("openstreetmap-mcp", env!("CARGO_PKG_VERSION")).without_websocket();

    run::<OsmArgs, OsmService, _, _>(config, |args| async move {
        let osm_config = OsmConfig {
            nominatim_url: args.nominatim_url,
            overpass_url: args.overpass_url,
            osrm_url: args.osrm_url,
            user_agent: args.user_agent,
        };
        Ok(OsmService::with_config(osm_config))
    })
    .await
}
