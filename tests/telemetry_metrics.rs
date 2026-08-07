#![deny(warnings)]

// Acceptance tests for openstreetmap-mcp's upstream-failure metric
// (mcp-core#40). Every test drives a real dispatch through either a local
// mock server (httpmock) or a closed local port -- never a live OSM service
// -- and reads the metric back from mcp-core's process-global registry.
//
// The registry is process-global and cargo test runs a file's tests
// concurrently by default, so every test here is guarded by METRICS_LOCK
// (adelie-telemetry#6 / mcp-core#40 lesson 6): two tests recording or
// reading the same instrument at once would race and flake.

mod support;

use httpmock::MockServer;
use mcp_core::telemetry::metrics::{self, Label};
use openstreetmap_mcp::config::OsmConfig;
use openstreetmap_mcp::service::OsmService;
use serde_json::json;

use support::{capture_dispatch, closed_port_config, tool_call};

const SEARCH_SUCCESS_FIXTURE: &str = include_str!("fixtures/nominatim/search_success.json");
const SEARCH_EMPTY_FIXTURE: &str = include_str!("fixtures/nominatim/search_empty.json");

/// Guards every test in this file so they run one at a time relative to each
/// other; it holds no data of its own.
static METRICS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock_metrics() -> std::sync::MutexGuard<'static, ()> {
    METRICS_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn service_pointed_at(server: &MockServer) -> OsmService {
    OsmService::with_config(OsmConfig {
        nominatim_url: server.base_url(),
        overpass_url: format!("{}/interpreter", server.base_url()),
        osrm_url: server.base_url(),
        user_agent: "openstreetmap-mcp-test/0.0".to_string(),
    })
}

fn search_dispatch(service: OsmService) {
    capture_dispatch(
        service,
        &[
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
            tool_call(2, "osm_search", json!({"query": "fixture query"})),
        ],
    );
}

#[test]
fn successful_search_does_not_increment_upstream_failures() {
    let _guard = lock_metrics();
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(httpmock::Method::GET).path("/search");
        then.status(200)
            .header("content-type", "application/json")
            .body(SEARCH_SUCCESS_FIXTURE);
    });

    let before = total_across_all_reasons("osm_search");
    search_dispatch(service_pointed_at(&server));
    mock.assert();

    assert_eq!(
        total_across_all_reasons("osm_search"),
        before,
        "a successful search must not increment osm.upstream_failures for any reason"
    );
}

#[test]
fn empty_search_result_does_not_increment_upstream_failures() {
    let _guard = lock_metrics();
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(httpmock::Method::GET).path("/search");
        then.status(200)
            .header("content-type", "application/json")
            .body(SEARCH_EMPTY_FIXTURE);
    });

    let before = total_across_all_reasons("osm_search");
    search_dispatch(service_pointed_at(&server));
    mock.assert();

    assert_eq!(
        total_across_all_reasons("osm_search"),
        before,
        "an empty (zero-result) search is a valid answer, not a fault, and must not increment \
         osm.upstream_failures"
    );
}

#[test]
fn rate_limited_search_increments_upstream_failures_as_api_error() {
    let _guard = lock_metrics();
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(httpmock::Method::GET).path("/search");
        then.status(429).body("Too Many Requests");
    });

    let labels = [
        Label::new("tool", "osm_search"),
        Label::new("reason", "api_error"),
    ];
    let before = counter_total("osm.upstream_failures", &labels);
    search_dispatch(service_pointed_at(&server));
    mock.assert();

    assert_eq!(
        counter_total("osm.upstream_failures", &labels),
        before + 1,
        "a 429 rate-limit response must increment osm.upstream_failures, tool=osm_search \
         reason=api_error"
    );
}

#[test]
fn server_error_search_increments_upstream_failures_as_api_error() {
    let _guard = lock_metrics();
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(httpmock::Method::GET).path("/search");
        then.status(500).body("internal server error");
    });

    let labels = [
        Label::new("tool", "osm_search"),
        Label::new("reason", "api_error"),
    ];
    let before = counter_total("osm.upstream_failures", &labels);
    search_dispatch(service_pointed_at(&server));
    mock.assert();

    assert_eq!(
        counter_total("osm.upstream_failures", &labels),
        before + 1,
        "a 500 response must increment osm.upstream_failures, tool=osm_search reason=api_error"
    );
}

#[test]
fn connection_failure_increments_upstream_failures_as_http_error() {
    let _guard = lock_metrics();
    let labels = [
        Label::new("tool", "osm_search"),
        Label::new("reason", "http_error"),
    ];
    let before = counter_total("osm.upstream_failures", &labels);

    search_dispatch(OsmService::with_config(closed_port_config()));

    assert_eq!(
        counter_total("osm.upstream_failures", &labels),
        before + 1,
        "a connection failure (no upstream reachable at all) must increment \
         osm.upstream_failures, tool=osm_search reason=http_error"
    );
}

/// The lifetime total of one counter series, or zero when it has never been
/// recorded. The registry is process-wide, so every assertion here is a
/// delta.
fn counter_total(name: &str, labels: &[Label]) -> u64 {
    metrics::global()
        .snapshot()
        .counters
        .iter()
        .find(|counter| counter.name == name && same_labels(&counter.labels, labels))
        .map_or(0, |counter| counter.total)
}

/// The lifetime total of `osm.upstream_failures` for `tool`, summed across
/// every `reason` -- used by the "must not increment for any reason" tests,
/// so a future reason bucket cannot silently hide a regression.
fn total_across_all_reasons(tool: &str) -> u64 {
    metrics::global()
        .snapshot()
        .counters
        .iter()
        .filter(|counter| {
            counter.name == "osm.upstream_failures"
                && counter
                    .labels
                    .iter()
                    .any(|l| l.key() == "tool" && l.value() == tool)
        })
        .map(|counter| counter.total)
        .sum()
}

fn same_labels(recorded: &[Label], wanted: &[Label]) -> bool {
    recorded.len() == wanted.len()
        && wanted.iter().all(|want| {
            recorded
                .iter()
                .any(|have| have.key() == want.key() && have.value() == want.value())
        })
}
