#![deny(warnings)]

// Acceptance tests for the telemetry openstreetmap-mcp inherits from
// mcp-core's `run`, plus this server's own per-request debug logging: the
// stdio transport keeps stdout clean at any log level, and no tool argument
// (a query, a coordinate, a tag, an id) ever reaches an INFO line.
//
// Each test spawns the real binary, with the OSM endpoint env vars pointed
// at a closed local port so no request ever reaches a live OSM service --
// only a real process proves what reaches file descriptor 1 and what the
// installed subscriber really writes to stderr; an in-process capturing
// layer only proves what a test told a layer to do.
//
// Table-driven over every tool `OsmService` advertises (mcp-core#40 lesson
// 8), the same table `tests/telemetry_span_fields.rs` uses, so a tool
// missing from the table is missing from both nets, not silently covered by
// one and not the other.

mod support;

use serde_json::{Value, json};
use std::io::Write;
use std::process::{Child, Command, Output, Stdio};

use support::sentinel_tool_calls;

fn spawn_with_log_level(level: &str) -> Child {
    let exe = env!("CARGO_BIN_EXE_openstreetmap-mcp");
    Command::new(exe)
        .args(["serve", "--mode", "stdio"])
        .env("RUST_LOG", level)
        .env("OSM_NOMINATIM_URL", "http://127.0.0.1:1")
        .env("OSM_OVERPASS_URL", "http://127.0.0.1:1/interpreter")
        .env("OSM_OSRM_URL", "http://127.0.0.1:1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn openstreetmap-mcp serve --mode stdio")
}

fn run_requests(level: &str, requests: &[Value]) -> Output {
    let mut child = spawn_with_log_level(level);
    {
        let stdin = child.stdin.as_mut().expect("child has a piped stdin");
        for request in requests {
            writeln!(stdin, "{request}").expect("write jsonrpc line");
        }
    }
    drop(child.stdin.take());
    child.wait_with_output().expect("child must exit")
}

/// The level word `tracing_subscriber`'s default console formatter writes as
/// the second whitespace-separated token, right after the timestamp. Reading
/// it this way (rather than a substring search for "INFO") does not confuse
/// a level word for content that happens to contain the same letters.
fn line_level(line: &str) -> Option<&str> {
    line.split_whitespace()
        .nth(1)
        .filter(|token| matches!(*token, "ERROR" | "WARN" | "INFO" | "DEBUG" | "TRACE"))
}

fn all_requests() -> Vec<Value> {
    let mut requests = vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
    ];
    let mut id = 2;
    for call in sentinel_tool_calls() {
        requests.push(json!({
            "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": {"name": call.tool, "arguments": call.args},
        }));
        id += 1;
    }
    requests.push(json!({"jsonrpc":"2.0","id":id,"method":"shutdown","params":{}}));
    requests
}

#[test]
fn stdout_carries_only_jsonrpc_at_trace_level() {
    let requests = all_requests();
    let expected_replies = requests.iter().filter(|r| r.get("id").is_some()).count();

    let output = run_requests("trace", &requests);
    assert!(
        output.status.success(),
        "openstreetmap-mcp must exit cleanly, otherwise an empty stdout proves nothing: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    let mut replies = 0;
    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        let value: Value = serde_json::from_str(line).unwrap_or_else(|e| {
            panic!("every stdout line must be JSON-RPC, but {line:?} is not: {e}")
        });
        assert_eq!(
            value.get("jsonrpc").and_then(Value::as_str),
            Some("2.0"),
            "every stdout line must carry the JSON-RPC envelope: {line:?}"
        );
        replies += 1;
    }
    assert_eq!(
        replies, expected_replies,
        "expected one reply per request that carried an id"
    );

    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(
        stderr.contains("INFO") || stderr.contains("DEBUG") || stderr.contains("TRACE"),
        "at RUST_LOG=trace the subscriber must be installed and log to stderr; stderr was: \
         {stderr:?}"
    );
}

/// AC (mcp-core#40, D10): for every advertised tool, no sentinel value (a
/// query, a coordinate, a tag, an id) reaches an INFO-or-louder line on
/// stderr.
#[test]
fn no_sentinel_reaches_an_info_line_for_any_tool() {
    support::assert_covers_every_tool();

    let requests = all_requests();
    let output = run_requests("trace", &requests);
    assert!(
        output.status.success(),
        "openstreetmap-mcp must exit cleanly: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");

    let all_sentinels: Vec<String> = sentinel_tool_calls()
        .into_iter()
        .flat_map(|call| call.sentinels)
        .collect();

    for sentinel in &all_sentinels {
        for line in stderr.lines() {
            if !line.contains(sentinel.as_str()) {
                continue;
            }
            let level = line_level(line);
            assert!(
                matches!(level, Some("DEBUG") | Some("TRACE")),
                "sentinel {sentinel:?} reached a line at level {level:?}, at or above INFO: \
                 {line:?}"
            );
        }
    }
}

/// AC (mcp-core#40): for every advertised tool, *this crate's own*
/// per-request debug line appears on stderr at DEBUG, carrying that tool's
/// sentinel content.
///
/// This is a stronger, per-tool positive control than "the sentinel is
/// reachable at DEBUG somewhere": mcp-core's own dispatch already logs every
/// tool's raw arguments at DEBUG (inherited for free, in a `"tool call
/// arguments"` line), so that alone would pass even if this crate never
/// logged its own outbound request. Matching the exact message each
/// `log_*_request` helper uses proves this server's own logging exists, tool
/// by tool, on the real console output rather than only in an in-process
/// capture.
#[test]
fn each_tool_call_logs_its_own_outbound_request_line_at_debug() {
    support::assert_covers_every_tool();

    let requests = all_requests();
    let output = run_requests("debug", &requests);
    assert!(
        output.status.success(),
        "openstreetmap-mcp must exit cleanly: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");

    for call in sentinel_tool_calls() {
        let matching: Vec<&str> = stderr
            .lines()
            .filter(|line| line_level(line) == Some("DEBUG") && line.contains(call.debug_message))
            .collect();
        assert!(
            !matching.is_empty(),
            "{}: expected a DEBUG line containing {:?}; stderr was {stderr:?}",
            call.tool,
            call.debug_message
        );
        for sentinel in &call.sentinels {
            assert!(
                matching.iter().any(|line| line.contains(sentinel.as_str())),
                "{}: the {:?} line must carry sentinel {sentinel:?}; matching lines were {:?}",
                call.tool,
                call.debug_message,
                matching
            );
        }
    }
}
