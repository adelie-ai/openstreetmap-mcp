#![deny(warnings)]

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};

// ── MCP stdio harness ─────────────────────────────────────────────────────────

struct McpStdioClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
    next_id: u64,
}

impl McpStdioClient {
    fn start() -> Self {
        let exe = env!("CARGO_BIN_EXE_openstreetmap-mcp");

        let mut child = Command::new(exe)
            .args(["serve", "--mode", "stdio"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn openstreetmap-mcp serve --mode stdio");

        let stdin = child.stdin.take().expect("child stdin");
        let stdout = child.stdout.take().expect("child stdout");

        Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
        }
    }

    fn send(&mut self, obj: &Value) {
        let s = serde_json::to_string(obj).expect("serialize jsonrpc");
        self.stdin
            .write_all(s.as_bytes())
            .and_then(|_| self.stdin.write_all(b"\n"))
            .and_then(|_| self.stdin.flush())
            .expect("write jsonrpc line");
    }

    fn read_msg(&mut self) -> Value {
        let mut line = String::new();
        loop {
            line.clear();
            let n = self.stdout.read_line(&mut line).expect("read line");
            if n == 0 {
                panic!("mcp server closed stdout");
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
                return v;
            }
        }
    }

    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;

        self.send(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}));

        loop {
            let msg = self.read_msg();
            if msg.get("id").and_then(|v| v.as_u64()) != Some(id) {
                continue;
            }
            if let Some(err) = msg.get("error") {
                return Err(err.to_string());
            }
            return Ok(msg);
        }
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(&json!({"jsonrpc":"2.0","method":method,"params":params}));
    }

    fn initialize(&mut self) {
        self.call(
            "initialize",
            json!({"protocolVersion":"2025-11-25","capabilities":{}}),
        )
        .expect("initialize");
        self.notify("initialized", json!({}));
    }

    fn tool_call(&mut self, name: &str, arguments: Value) -> Result<Value, String> {
        let resp = self.call("tools/call", json!({"name":name,"arguments":arguments}))?;
        resp.get("result")
            .cloned()
            .ok_or_else(|| format!("missing result field: {resp}"))
    }
}

impl Drop for McpStdioClient {
    fn drop(&mut self) {
        let _ = self.call("shutdown", json!({}));
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Extract and parse the JSON payload from the first `type: text` content
/// entry.  mcp-core serialises all tool results as `{"type":"text","text":"…"}`
/// where the text is pretty-printed JSON; we parse the string back so callers
/// can traverse the value as before.
fn extract_value(tool_result: &Value) -> Value {
    let content = tool_result
        .get("content")
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| panic!("expected result.content array, got: {tool_result}"));

    for entry in content {
        if entry.get("type") == Some(&Value::String("text".to_string()))
            && let Some(text) = entry.get("text").and_then(|v| v.as_str())
        {
            return serde_json::from_str(text)
                .unwrap_or_else(|e| panic!("content text is not valid JSON: {e}\ntext: {text}"));
        }
    }

    panic!("no text content entry in: {tool_result}");
}

fn network_tests_enabled() -> bool {
    std::env::var("RUN_NETWORK_TESTS").ok().as_deref() == Some("1")
}

fn expect_err_contains<T>(res: Result<T, String>, needle: &str) {
    match res {
        Ok(_) => panic!("expected error containing '{needle}', but call succeeded"),
        Err(e) => {
            let lower = e.to_lowercase();
            assert!(
                lower.contains(&needle.to_lowercase()),
                "expected error containing '{needle}', got: {e}"
            );
        }
    }
}

// ── Protocol tests (no network) ───────────────────────────────────────────────

/// The server must respond to `initialize` with serverInfo and capabilities.
#[test]
fn test_initialize_response_shape() {
    let mut client = McpStdioClient::start();
    let resp = client
        .call(
            "initialize",
            json!({"protocolVersion":"2025-11-25","capabilities":{}}),
        )
        .expect("initialize");

    let result = resp.get("result").expect("result field");
    assert!(
        result.get("serverInfo").is_some(),
        "missing serverInfo: {result}"
    );
    let server_info = result.get("serverInfo").unwrap();
    assert_eq!(
        server_info.get("name").and_then(|v| v.as_str()),
        Some("openstreetmap-mcp"),
        "unexpected serverInfo.name"
    );
    assert!(
        result.get("capabilities").is_some(),
        "missing capabilities: {result}"
    );
}

/// The `initialize` response must carry a non-empty `instructions` string. The
/// daemon captures it as the server's searchable description for tool discovery,
/// so its absence blinds server-grain routing.
#[test]
fn test_initialize_response_includes_instructions() {
    let mut client = McpStdioClient::start();
    let resp = client
        .call(
            "initialize",
            json!({"protocolVersion":"2025-11-25","capabilities":{}}),
        )
        .expect("initialize");

    let result = resp.get("result").expect("result field");
    let instructions = result
        .get("instructions")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert!(
        !instructions.trim().is_empty(),
        "initialize result must include a non-empty instructions string, got: {result}"
    );
}

/// `tools/list` must return the expected set of tool names.
#[test]
fn test_tools_list_contains_expected_tools() {
    let mut client = McpStdioClient::start();
    client.initialize();

    let resp = client.call("tools/list", json!({})).expect("tools/list");
    let result = resp.get("result").expect("result field");

    let tools_val = result.get("tools").expect("tools field");
    let tools = tools_val.as_array().expect("tools is an array");

    // tools/list nests the tool array one level: { tools: [[ {..}, .. ]] }.
    let names: Vec<&str> = if tools.len() == 1 && tools[0].is_array() {
        tools[0]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t.get("name").and_then(|v| v.as_str()))
            .collect()
    } else {
        tools
            .iter()
            .filter_map(|t| t.get("name").and_then(|v| v.as_str()))
            .collect()
    };

    let expected = [
        "osm_search",
        "osm_reverse",
        "osm_lookup",
        "osm_nearby",
        "osm_route",
    ];

    for expected_name in &expected {
        assert!(
            names.contains(expected_name),
            "tool '{}' missing from tools/list. Got: {:?}",
            expected_name,
            names
        );
    }
}

/// Calling a tool before `initialize` must return an error.
#[test]
fn test_tool_call_before_initialize_returns_error() {
    let mut client = McpStdioClient::start();
    let result = client.tool_call("osm_search", json!({"query": "London"}));
    assert!(
        result.is_err(),
        "expected error when calling tool before initialize"
    );
}

/// An unknown tool name must surface as an `isError: true` tool result.
///
/// mcp-core maps `CallError::Tool` to spec-compliant `isError` content rather
/// than a JSON-RPC protocol error, so `tool_call` succeeds but the result
/// carries `isError: true`.
#[test]
fn test_unknown_tool_returns_error() {
    let mut client = McpStdioClient::start();
    client.initialize();
    let result = client
        .tool_call("nonexistent_tool", json!({}))
        .expect("mcp-core surfaces unknown tool as isError content, not a protocol error");
    assert_eq!(
        result.get("isError"),
        Some(&Value::Bool(true)),
        "expected isError:true for unknown tool, got: {result}"
    );
    // The content text should mention the tool name so models understand what failed.
    let content_text = result["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_lowercase();
    assert!(
        content_text.contains("unknown")
            || content_text.contains("not found")
            || content_text.contains("nonexistent"),
        "expected error message to reference the unknown tool, got: {content_text}"
    );
}

/// An unknown method must return a method-not-found error.
#[test]
fn test_unknown_method_returns_method_not_found() {
    let mut client = McpStdioClient::start();
    client.initialize();
    let result = client.call("unknownMethod/foobar", json!({}));
    assert!(result.is_err(), "expected error for unknown method");
}

/// Malformed JSON must return a parse error.
#[test]
fn test_malformed_json_returns_parse_error() {
    let mut client = McpStdioClient::start();

    client
        .stdin
        .write_all(b"this is not json at all\n")
        .and_then(|_| client.stdin.flush())
        .expect("write malformed json");

    let msg = client.read_msg();
    assert!(
        msg.get("error").is_some(),
        "expected error response for malformed json, got: {msg}"
    );
}

// ── Parameter validation tests (no network) ───────────────────────────────────

/// `osm_search` must reject a missing query.
#[test]
fn test_search_missing_query() {
    let mut client = McpStdioClient::start();
    client.initialize();
    let result = client.tool_call("osm_search", json!({"limit": 3}));
    expect_err_contains(result, "query");
}

/// `osm_reverse` must reject a missing coordinate.
#[test]
fn test_reverse_missing_coordinate() {
    let mut client = McpStdioClient::start();
    client.initialize();
    let result = client.tool_call("osm_reverse", json!({"latitude": 51.5}));
    expect_err_contains(result, "longitude");
}

/// `osm_lookup` must reject a malformed OSM id without hitting the network.
#[test]
fn test_lookup_rejects_malformed_id() {
    let mut client = McpStdioClient::start();
    client.initialize();
    let result = client.tool_call("osm_lookup", json!({"osm_ids": "X123"}));
    expect_err_contains(result, "invalid");
}

/// `osm_nearby` must reject a missing tag key.
#[test]
fn test_nearby_missing_key() {
    let mut client = McpStdioClient::start();
    client.initialize();
    let result = client.tool_call("osm_nearby", json!({"latitude": 51.5, "longitude": -0.12}));
    expect_err_contains(result, "key");
}

/// `osm_route` must reject fewer than two coordinates without hitting the network.
#[test]
fn test_route_requires_two_coordinates() {
    let mut client = McpStdioClient::start();
    client.initialize();
    let result = client.tool_call(
        "osm_route",
        json!({"coordinates": [{"latitude": 51.5, "longitude": -0.12}]}),
    );
    expect_err_contains(result, "two");
}

/// `osm_route` must reject an invalid profile without hitting the network.
#[test]
fn test_route_rejects_invalid_profile() {
    let mut client = McpStdioClient::start();
    client.initialize();
    let result = client.tool_call(
        "osm_route",
        json!({
            "coordinates": [
                {"latitude": 51.5, "longitude": -0.12},
                {"latitude": 48.85, "longitude": 2.35}
            ],
            "profile": "teleport"
        }),
    );
    expect_err_contains(result, "profile");
}

// ── Network integration tests (require RUN_NETWORK_TESTS=1) ──────────────────

/// Search "London" and verify we get a plausible UK result.
#[test]
fn test_search_london_network() {
    if !network_tests_enabled() {
        eprintln!("Skipping network test (set RUN_NETWORK_TESTS=1 to enable)");
        return;
    }

    let mut client = McpStdioClient::start();
    client.initialize();

    let result = client
        .tool_call("osm_search", json!({"query": "London", "limit": 3}))
        .expect("search London");

    let places = extract_value(&result);
    let arr = places.as_array().expect("expected array of places");
    assert!(!arr.is_empty(), "expected at least one search result");

    let first = &arr[0];
    let lat = first.get("latitude").and_then(|v| v.as_f64()).unwrap();
    let lon = first.get("longitude").and_then(|v| v.as_f64()).unwrap();
    assert!((lat - 51.5).abs() < 1.0, "unexpected latitude: {lat}");
    assert!((lon - (-0.12)).abs() < 1.0, "unexpected longitude: {lon}");
}

/// Search for an unknown location must return a NotFound error.
#[test]
fn test_search_nonexistent_location_network() {
    if !network_tests_enabled() {
        eprintln!("Skipping network test (set RUN_NETWORK_TESTS=1 to enable)");
        return;
    }

    let mut client = McpStdioClient::start();
    client.initialize();

    let result = client.tool_call(
        "osm_search",
        json!({"query": "xyzzy_nonexistent_place_00000"}),
    );
    assert!(result.is_err(), "expected error for nonexistent location");
}

/// Reverse geocode a coordinate in London and verify we get an address back.
#[test]
fn test_reverse_london_network() {
    if !network_tests_enabled() {
        eprintln!("Skipping network test (set RUN_NETWORK_TESTS=1 to enable)");
        return;
    }

    let mut client = McpStdioClient::start();
    client.initialize();

    let result = client
        .tool_call(
            "osm_reverse",
            json!({"latitude": 51.5074, "longitude": -0.1278}),
        )
        .expect("reverse London");

    let place = extract_value(&result);
    assert!(
        place.get("display_name").and_then(|v| v.as_str()).is_some(),
        "expected a display_name, got: {place}"
    );
}

/// Find cafes near a point in central London.
#[test]
fn test_nearby_cafes_network() {
    if !network_tests_enabled() {
        eprintln!("Skipping network test (set RUN_NETWORK_TESTS=1 to enable)");
        return;
    }

    let mut client = McpStdioClient::start();
    client.initialize();

    let result = client
        .tool_call(
            "osm_nearby",
            json!({
                "latitude": 51.5074,
                "longitude": -0.1278,
                "key": "amenity",
                "value": "cafe",
                "radius": 1000,
                "limit": 10
            }),
        )
        .expect("nearby cafes");

    let features = extract_value(&result);
    let arr = features.as_array().expect("expected array of features");
    assert!(!arr.is_empty(), "expected at least one cafe near London");
    // Results must be sorted nearest-first.
    let dists: Vec<f64> = arr
        .iter()
        .filter_map(|f| f.get("distance_meters").and_then(|v| v.as_f64()))
        .collect();
    assert!(
        dists.windows(2).all(|w| w[0] <= w[1]),
        "features not sorted by distance: {dists:?}"
    );
}

/// Route from London to Paris and verify a plausible driving distance.
#[test]
fn test_route_london_to_paris_network() {
    if !network_tests_enabled() {
        eprintln!("Skipping network test (set RUN_NETWORK_TESTS=1 to enable)");
        return;
    }

    let mut client = McpStdioClient::start();
    client.initialize();

    let result = client
        .tool_call(
            "osm_route",
            json!({
                "coordinates": [
                    {"latitude": 51.5074, "longitude": -0.1278},
                    {"latitude": 48.8566, "longitude": 2.3522}
                ],
                "profile": "driving"
            }),
        )
        .expect("route London to Paris");

    let route = extract_value(&result);
    let distance = route
        .get("distance_meters")
        .and_then(|v| v.as_f64())
        .unwrap();
    // Road distance London→Paris is ~450 km; allow a wide band.
    assert!(
        (300_000.0..700_000.0).contains(&distance),
        "unexpected route distance: {distance} m"
    );
}
