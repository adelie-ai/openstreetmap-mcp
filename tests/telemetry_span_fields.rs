#![deny(warnings)]

// In-process proof of D10 for openstreetmap-mcp: whatever a tool handler does
// with a caller's query, coordinate, tag, or id, it never becomes a span
// field, at any level, and it never reaches an event at INFO or above.
//
// `tests/telemetry_stdio.rs` proves the same thing against the real,
// installed subscriber; this drives mcp-core's dispatch directly and reads
// back the spans and events it really emitted. A span field would not
// necessarily show up on an INFO-level *line* of console text (the fmt layer
// only renders a span's fields on a line when some event fires while that
// span is entered), so this checks span fields directly rather than relying
// on the console rendering to surface one.
//
// Table-driven over every tool `OsmService` advertises (mcp-core#40 lesson
// 8): a tool added later without a matching entry in
// `support::sentinel_tool_calls()` fails `support::assert_covers_every_tool`
// rather than shipping with an unguarded content-leak path.

mod support;

use openstreetmap_mcp::service::OsmService;
use tracing::Level;

use support::{capture_dispatch, closed_port_config, sentinel_tool_calls, tool_call};

fn dispatch_all_sentinel_calls() -> support::Recorded {
    let calls = sentinel_tool_calls();
    let mut messages = vec![serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}
    })];
    for (i, call) in calls.iter().enumerate() {
        messages.push(tool_call(2 + i as u64, call.tool, call.args.clone()));
    }
    let service = OsmService::with_config(closed_port_config());
    capture_dispatch(service, &messages)
}

/// AC (mcp-core#40, D10): for every advertised tool, called with a
/// sentinel-laden argument set, no span field anywhere carries any sentinel
/// value, and no INFO-or-louder event carries one.
#[test]
fn no_tool_call_leaks_content_into_any_span_field_or_info_event() {
    support::assert_covers_every_tool();
    let calls = sentinel_tool_calls();
    let recorded = dispatch_all_sentinel_calls();

    let all_sentinels: Vec<&str> = calls
        .iter()
        .flat_map(|call| call.sentinels.iter().map(String::as_str))
        .collect();

    for span in &recorded.spans {
        for (key, value) in &span.fields {
            for sentinel in &all_sentinels {
                assert!(
                    !value.contains(sentinel),
                    "sentinel {sentinel:?} reached span {:?} field {key:?}: {value:?}",
                    span.name
                );
            }
        }
    }

    for event in &recorded.events {
        if event.level > Level::INFO {
            continue;
        }
        for (key, value) in &event.fields {
            for sentinel in &all_sentinels {
                assert!(
                    !value.contains(sentinel),
                    "sentinel {sentinel:?} reached a {} line, field {key:?}: {value:?}",
                    event.level
                );
            }
        }
    }
}

/// AC (mcp-core#40): for every advertised tool, *this crate's own*
/// per-request `debug!` fires, carrying that tool's sentinel content.
///
/// This is a stronger, per-tool positive control than "the sentinel is
/// reachable at DEBUG somewhere": mcp-core's own dispatch already logs every
/// tool's raw arguments at DEBUG (inherited for free), so that alone would
/// pass even if this crate never logged its own outbound request. Matching
/// the exact message each `log_*_request` helper uses proves this server's
/// own logging exists, tool by tool, and a tool missing its own debug event
/// fails by name rather than being averaged away across the whole table.
#[test]
fn each_tool_call_logs_its_own_outbound_request_at_debug() {
    support::assert_covers_every_tool();
    let calls = sentinel_tool_calls();
    let recorded = dispatch_all_sentinel_calls();

    for call in &calls {
        let matching: Vec<_> = recorded
            .events
            .iter()
            .filter(|event| {
                event.level == Level::DEBUG
                    && event.fields.get("message").map(String::as_str) == Some(call.debug_message)
            })
            .collect();
        assert!(
            !matching.is_empty(),
            "{}: expected a DEBUG event with message {:?}; the DEBUG/TRACE events were {:?}",
            call.tool,
            call.debug_message,
            recorded
                .events
                .iter()
                .filter(|e| e.level >= Level::DEBUG)
                .map(|e| &e.fields)
                .collect::<Vec<_>>()
        );
        for sentinel in &call.sentinels {
            assert!(
                matching
                    .iter()
                    .any(|event| event.fields.values().any(|v| v.contains(sentinel))),
                "{}: the {:?} event must carry sentinel {sentinel:?}; matching events were {:?}",
                call.tool,
                call.debug_message,
                matching
            );
        }
    }
}

/// AC (mcp-core#40): every tool handler is instrumented -- a span opens for
/// each, nested under mcp-core's own `mcp.tools.call` span. Table-driven so a
/// new tool without its own span fails this test too, not only the checks
/// above.
#[test]
fn each_tool_handler_opens_its_own_span() {
    support::assert_covers_every_tool();
    let calls = sentinel_tool_calls();
    let recorded = dispatch_all_sentinel_calls();

    let expected_spans = [
        "call_search",
        "call_reverse",
        "call_lookup",
        "call_nearby",
        "call_route",
    ];
    assert_eq!(
        expected_spans.len(),
        calls.len(),
        "the expected-span list must stay in step with the tool table"
    );
    for expected in expected_spans {
        assert!(
            recorded.spans.iter().any(|span| span.name == expected),
            "expected a {expected:?} span; the spans were {:?}",
            recorded
                .spans
                .iter()
                .map(|span| span.name)
                .collect::<Vec<_>>()
        );
    }
}
