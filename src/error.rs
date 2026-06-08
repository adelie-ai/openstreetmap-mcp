// Error types for the openstreetmap-mcp crate

use thiserror::Error;

/// Main error type for the openstreetmap-mcp application
#[derive(Error, Debug)]
pub enum OsmMcpError {
    /// OpenStreetMap operation errors
    #[error("OpenStreetMap error: {0}")]
    Osm(#[from] OsmError),

    /// JSON serialization/deserialization errors
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// MCP protocol errors
    #[error("MCP protocol error: {0}")]
    Mcp(#[from] McpError),

    /// Transport layer errors
    #[error("Transport error: {0}")]
    Transport(#[from] TransportError),

    /// IO errors
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// HTTP errors
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
}

/// OpenStreetMap operation errors.
///
/// Why: these mirror the failure modes of the upstream OSM services
/// (Nominatim, Overpass, OSRM) so callers can branch on a structured variant
/// instead of pattern-matching on a message string.
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

/// MCP protocol errors
#[derive(Error, Debug)]
pub enum McpError {
    /// Invalid protocol version
    #[error("Unsupported protocol version: {0}")]
    InvalidProtocolVersion(String),

    /// Invalid JSON-RPC message
    #[error("Invalid JSON-RPC message: {0}")]
    InvalidJsonRpc(String),

    /// Tool not found
    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    /// Invalid tool parameters
    #[error("Invalid tool parameters: {0}")]
    InvalidToolParameters(String),
}

/// Transport layer errors
#[derive(Error, Debug)]
pub enum TransportError {
    /// WebSocket connection error
    #[error("WebSocket connection error: {0}")]
    WebSocket(String),

    /// Invalid message format
    #[error("Invalid message format: {0}")]
    InvalidMessage(String),

    /// Connection closed
    #[error("Connection closed")]
    ConnectionClosed,

    /// IO error in transport
    #[error("Transport IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Result type alias for convenience
pub type Result<T> = std::result::Result<T, OsmMcpError>;
