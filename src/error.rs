// Error types for the openstreetmap-mcp crate.
//
// mcp-core owns protocol and transport errors; this module contains only the
// domain-level errors produced by the OSM operation modules.

use thiserror::Error;

/// Top-level error type for the openstreetmap-mcp library.
#[derive(Error, Debug)]
pub enum OsmMcpError {
    /// OpenStreetMap operation errors.
    #[error("OpenStreetMap error: {0}")]
    Osm(#[from] OsmError),

    /// JSON serialization/deserialization errors.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// MCP parameter errors (e.g. invalid tool parameters).
    #[error("MCP error: {0}")]
    Mcp(#[from] McpError),

    /// IO errors (e.g. from reqwest body reads).
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// HTTP errors (reqwest).
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
}

/// OpenStreetMap operation errors.
///
/// These mirror the failure modes of the upstream OSM services (Nominatim,
/// Overpass, OSRM) so callers can branch on a structured variant instead of
/// pattern-matching on a message string.
#[derive(Error, Debug)]
pub enum OsmError {
    /// No matching place / address / route was found.
    #[error("Not found: {0}")]
    NotFound(String),

    /// An upstream OSM service returned an error or an unexpected payload.
    #[error("API error: {0}")]
    ApiError(String),

    /// The caller supplied invalid parameters (out of range, malformed, etc.).
    #[error("Invalid parameters: {0}")]
    InvalidParameters(String),
}

/// MCP-level parameter errors.
#[derive(Error, Debug)]
pub enum McpError {
    /// A tool was called with missing or structurally invalid parameters.
    #[error("Invalid tool parameters: {0}")]
    InvalidToolParameters(String),
}

/// Result type alias for the OSM operation modules.
pub type Result<T> = std::result::Result<T, OsmMcpError>;
