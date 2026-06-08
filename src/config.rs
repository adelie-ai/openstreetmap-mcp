// Runtime configuration: which OpenStreetMap service endpoints to talk to and
// the User-Agent to identify ourselves with.
//
// Why this is configurable: the public OSM endpoints (Nominatim, Overpass,
// OSRM) enforce usage policies and rate limits, so operators frequently want
// to point this server at a self-hosted or commercial instance instead. A
// valid, contactful `User-Agent` is also *required* by the Nominatim usage
// policy — see <https://operations.osmfoundation.org/policies/nominatim/>.

/// Default Nominatim base URL (geocoding / reverse geocoding / lookup).
pub const DEFAULT_NOMINATIM_URL: &str = "https://nominatim.openstreetmap.org";
/// Default Overpass API interpreter endpoint (feature queries).
pub const DEFAULT_OVERPASS_URL: &str = "https://overpass-api.de/api/interpreter";
/// Default OSRM routing base URL (the public demo server).
pub const DEFAULT_OSRM_URL: &str = "https://router.project-osrm.org";

/// The default User-Agent. Identifies the software and version per the
/// Nominatim usage policy.
pub const DEFAULT_USER_AGENT: &str = concat!("openstreetmap-mcp/", env!("CARGO_PKG_VERSION"));

/// Endpoints and identification used when talking to OpenStreetMap services.
#[derive(Debug, Clone)]
pub struct OsmConfig {
    /// Nominatim base URL (no trailing slash).
    pub nominatim_url: String,
    /// Overpass API interpreter URL.
    pub overpass_url: String,
    /// OSRM routing base URL (no trailing slash).
    pub osrm_url: String,
    /// User-Agent header sent with every request.
    pub user_agent: String,
}

impl Default for OsmConfig {
    fn default() -> Self {
        Self {
            nominatim_url: DEFAULT_NOMINATIM_URL.to_string(),
            overpass_url: DEFAULT_OVERPASS_URL.to_string(),
            osrm_url: DEFAULT_OSRM_URL.to_string(),
            user_agent: DEFAULT_USER_AGENT.to_string(),
        }
    }
}

impl OsmConfig {
    /// Nominatim base URL with any trailing slash trimmed, so callers can
    /// append `/search`, `/reverse`, etc. without producing a double slash.
    pub fn nominatim_base(&self) -> &str {
        self.nominatim_url.trim_end_matches('/')
    }

    /// OSRM base URL with any trailing slash trimmed.
    pub fn osrm_base(&self) -> &str {
        self.osrm_url.trim_end_matches('/')
    }
}
