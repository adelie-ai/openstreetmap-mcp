//! A capturing `tracing` layer, and a driver that runs openstreetmap-mcp's
//! real service under it.
//!
//! The telemetry criteria are about what the dispatch path emits, so a test
//! has to read the spans and events back rather than assert a constant
//! against itself. Each test file that needs it declares `mod support;`, so
//! not every item is reached from every file.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::{Arc, Mutex};

use mcp_core::{McpService, ServerCore, Session};
use openstreetmap_mcp::service::OsmService;
use serde_json::{Value, json};
use tracing::Level;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;

/// One span, as the subscriber saw it. A span whose fields are recorded after
/// creation appears a second time, carrying only what was recorded then.
#[derive(Clone, Debug)]
pub struct RecordedSpan {
    /// The span's name.
    pub name: &'static str,
    /// Field name to its rendered value.
    pub fields: BTreeMap<String, String>,
}

/// One event, as the subscriber saw it.
#[derive(Clone, Debug)]
pub struct RecordedEvent {
    /// The level the event was emitted at.
    pub level: Level,
    /// Field name to its rendered value. The message is the `message` field.
    pub fields: BTreeMap<String, String>,
}

/// Everything one captured run produced.
#[derive(Clone, Debug, Default)]
pub struct Recorded {
    /// Spans, in the order they opened.
    pub spans: Vec<RecordedSpan>,
    /// Events, in the order they were emitted.
    pub events: Vec<RecordedEvent>,
}

impl Recorded {
    /// A short rendering for an assertion message.
    pub fn span_summary(&self) -> Vec<String> {
        self.spans
            .iter()
            .map(|span| format!("{}{:?}", span.name, span.fields))
            .collect()
    }

    /// A short rendering for an assertion message.
    pub fn event_summary(&self) -> Vec<String> {
        self.events
            .iter()
            .map(|event| format!("{}{:?}", event.level, event.fields))
            .collect()
    }
}

/// Run `body` with a capturing subscriber installed on this thread, and
/// return what it emitted.
pub fn capture<F, Fut>(body: F) -> Recorded
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = ()>,
{
    let capture = Capture::default();
    let subscriber = tracing_subscriber::registry().with(capture.clone());
    tracing::subscriber::with_default(subscriber, || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a current-thread runtime");
        runtime.block_on(body());
    });
    capture.take()
}

/// Drive `messages` through one session over `service`, capturing what the
/// dispatch path emitted. `service` is caller-supplied (rather than the
/// crate's built-in default) so a test can point it at a local mock server.
pub fn capture_dispatch(service: OsmService, messages: &[Value]) -> Recorded {
    let messages = messages.to_vec();
    capture(|| async move {
        let core = ServerCore::new(openstreetmap_mcp::server_config(), Arc::new(service));
        let mut session = Session::new(core);
        for message in messages {
            session.handle_message(message).await;
        }
    })
}

/// The `initialize` / `initialized` pair every dispatch needs before a tool
/// call is accepted.
pub fn init_messages() -> Vec<Value> {
    vec![serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}})]
}

/// A `tools/call` JSON-RPC request.
pub fn tool_call(id: u64, name: &str, arguments: Value) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {"name": name, "arguments": arguments},
    })
}

// ── Content sentinels, shared by every leak test (mcp-core#40 lesson 8) ────
//
// A single tool-call table drives both content tests (the in-process
// span-field capture and the real-process console/stdout test), so a tool
// that is missing from it is missing from *both* nets, not silently covered
// by one and not the other. `assert_covers_every_tool` cross-checks the
// table against the server's own advertised tool list, so adding a tool
// without adding it here fails loudly instead of shipping unguarded.

/// The value every "place query" sentinel test hunts for: unbounded caller
/// content (a free-form search query), never an id.
pub const SENTINEL_QUERY: &str = "MARKER-osm-query-9f3d1c2a";
/// A coordinate precise enough that its rendered digits cannot appear by
/// accident anywhere else in a captured field.
pub const SENTINEL_LATITUDE: f64 = 12.34908675;
pub const SENTINEL_LONGITUDE: f64 = -56.78091234;
/// An OSM tag key/value pair, as `osm_nearby` takes them.
pub const SENTINEL_KEY: &str = "MARKER-osm-nearby-key-9f3d1c2a";
pub const SENTINEL_VALUE: &str = "MARKER-osm-nearby-value-9f3d1c2a";
/// A syntactically valid OSM id (`osm_lookup` only accepts an N/W/R prefix
/// plus digits, so there is no room for a text marker) built from digits
/// distinctive enough not to collide by accident.
pub const SENTINEL_OSM_IDS: &str = "N90019002";

/// One entry in [`sentinel_tool_calls`]: a tool, a sentinel-laden argument
/// set, the rendered sentinel substrings that call must make reachable at
/// DEBUG, and the exact message of *this crate's own* per-request debug log
/// for that call.
///
/// `debug_message` matters beyond "some event mentions the sentinel":
/// mcp-core's own dispatch already logs every tool's raw arguments at DEBUG
/// (`server.rs`'s `"tool call arguments"` event), inherited for free by every
/// server. A positive control that only checked "the sentinel is reachable
/// at DEBUG somewhere" would pass from that inherited event alone, even if
/// this crate's own outbound-request `debug!` were never added. Requiring
/// the specific message this crate's own `log_*_request` functions use is
/// what proves *this* server's own logging exists, not only mcp-core's.
pub struct SentinelCall {
    pub tool: &'static str,
    pub args: Value,
    pub sentinels: Vec<String>,
    pub debug_message: &'static str,
}

/// Every tool this server advertises, paired with a sentinel-laden argument
/// set. Cross-check with [`assert_covers_every_tool`] before using this
/// list, so a tool this table has fallen behind on is caught rather than
/// silently skipped.
pub fn sentinel_tool_calls() -> Vec<SentinelCall> {
    vec![
        SentinelCall {
            tool: "osm_search",
            args: json!({"query": SENTINEL_QUERY}),
            sentinels: vec![SENTINEL_QUERY.to_string()],
            debug_message: "querying nominatim search",
        },
        SentinelCall {
            tool: "osm_reverse",
            args: json!({"latitude": SENTINEL_LATITUDE, "longitude": SENTINEL_LONGITUDE}),
            sentinels: vec![
                SENTINEL_LATITUDE.to_string(),
                SENTINEL_LONGITUDE.to_string(),
            ],
            debug_message: "querying nominatim reverse",
        },
        SentinelCall {
            tool: "osm_lookup",
            args: json!({"osm_ids": SENTINEL_OSM_IDS}),
            sentinels: vec![SENTINEL_OSM_IDS.to_string()],
            debug_message: "querying nominatim lookup",
        },
        SentinelCall {
            tool: "osm_nearby",
            args: json!({
                "latitude": SENTINEL_LATITUDE,
                "longitude": SENTINEL_LONGITUDE,
                "key": SENTINEL_KEY,
                "value": SENTINEL_VALUE,
            }),
            sentinels: vec![SENTINEL_KEY.to_string(), SENTINEL_VALUE.to_string()],
            debug_message: "querying overpass",
        },
        SentinelCall {
            tool: "osm_route",
            args: json!({
                "coordinates": [
                    {"latitude": SENTINEL_LATITUDE, "longitude": SENTINEL_LONGITUDE},
                    {"latitude": 0.0, "longitude": 0.0},
                ],
            }),
            sentinels: vec![
                SENTINEL_LATITUDE.to_string(),
                SENTINEL_LONGITUDE.to_string(),
            ],
            debug_message: "querying osrm route",
        },
    ]
}

/// Fail loudly if [`sentinel_tool_calls`] has fallen out of sync with the
/// server's own advertised tool list. This is the guard mcp-core#40 lesson 8
/// asks for: a tool added to `OsmService` without a matching entry here must
/// fail this check, not ship with an unchecked content-leak path.
pub fn assert_covers_every_tool() {
    use std::collections::BTreeSet;
    let advertised: BTreeSet<String> = OsmService::new()
        .tools()
        .into_iter()
        .map(|t| t.name)
        .collect();
    let tested: BTreeSet<String> = sentinel_tool_calls()
        .into_iter()
        .map(|call| call.tool.to_string())
        .collect();
    assert_eq!(
        tested, advertised,
        "sentinel_tool_calls() must cover exactly the tools OsmService advertises, or a new \
         tool ships without a content-leak check (mcp-core#40 lesson 8)"
    );
}

/// A config that fails every outbound request fast (connection refused on a
/// closed local port) without ever reaching a live OSM service or any real
/// network host. Used by the content-leak tests, which only need the
/// request to *start* -- the per-request `debug!` fires, and the tool
/// handler's span opens, before the (failing) send.
pub fn closed_port_config() -> openstreetmap_mcp::config::OsmConfig {
    openstreetmap_mcp::config::OsmConfig {
        nominatim_url: "http://127.0.0.1:1".to_string(),
        overpass_url: "http://127.0.0.1:1/interpreter".to_string(),
        osrm_url: "http://127.0.0.1:1".to_string(),
        user_agent: "openstreetmap-mcp-test/0.0".to_string(),
    }
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Recorded>>);

impl Capture {
    fn take(self) -> Recorded {
        self.0
            .lock()
            .expect("the capture lock is only held to push one record")
            .clone()
    }
}

impl<S> Layer<S> for Capture
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, _id: &Id, _ctx: Context<'_, S>) {
        let mut fields = BTreeMap::new();
        attrs.record(&mut Collector(&mut fields));
        self.0
            .lock()
            .expect("the capture lock is only held to push one record")
            .spans
            .push(RecordedSpan {
                name: attrs.metadata().name(),
                fields,
            });
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let name = ctx.span(id).map_or("<closed>", |span| span.name());
        let mut fields = BTreeMap::new();
        values.record(&mut Collector(&mut fields));
        self.0
            .lock()
            .expect("the capture lock is only held to push one record")
            .spans
            .push(RecordedSpan { name, fields });
    }

    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let mut fields = BTreeMap::new();
        event.record(&mut Collector(&mut fields));
        self.0
            .lock()
            .expect("the capture lock is only held to push one record")
            .events
            .push(RecordedEvent {
                level: *event.metadata().level(),
                fields,
            });
    }
}

struct Collector<'a>(&'a mut BTreeMap<String, String>);

impl Visit for Collector<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name().to_string(), value.to_string());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0
            .insert(field.name().to_string(), format!("{value:?}"));
    }
}
