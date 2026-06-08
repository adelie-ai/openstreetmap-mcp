// OpenStreetMap operation implementations.
//
// Each module wraps one upstream OSM service call and normalizes its response
// into a stable JSON shape for the MCP tool layer.

pub mod lookup;
pub mod nearby;
pub mod nominatim;
pub mod reverse;
pub mod route;
pub mod search;
