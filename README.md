# openstreetmap-mcp

A small, fast Rust **MCP server** (plus library) that exposes **OpenStreetMap**
services — geocoding, feature search, and routing — over a simple JSON-RPC
transport. It is intended for use by **LLM agents** and other automated clients
that need location intelligence without managing API keys.

It wraps three free, public OpenStreetMap-powered services:

- **[Nominatim](https://nominatim.org/)** — forward/reverse geocoding and id lookup.
- **[Overpass API](https://wiki.openstreetmap.org/wiki/Overpass_API)** — querying map features (POIs, amenities) by tag.
- **[OSRM](http://project-osrm.org/)** — routing and turn-by-turn directions.

## Tools

| Tool | Service | Purpose |
| --- | --- | --- |
| `osm_search` | Nominatim `/search` | Forward geocode a free-form query → places with coordinates, OSM ids, structured address, bounding box. |
| `osm_reverse` | Nominatim `/reverse` | Reverse geocode a coordinate → nearest addressable place. |
| `osm_lookup` | Nominatim `/lookup` | Resolve specific OSM ids (e.g. `R146656,W104393803,N240109189`) → details. |
| `osm_nearby` | Overpass | Find features tagged `key` (optionally `key=value`) within a radius of a point, sorted nearest-first. |
| `osm_route` | OSRM | Route through ≥2 waypoints → distance, duration, GeoJSON geometry, per-leg steps. |

All tools return their payload in the MCP `content` envelope as a `type: "json"`
entry. See [`docs/result_shapes.md`](docs/result_shapes.md) for the exact shapes.

## Build & run

Requires a Rust toolchain (pinned in `rust-toolchain.toml`).

```bash
cargo build --release

# stdio transport (recommended for local/editor usage)
./target/release/openstreetmap-mcp serve --mode stdio

# WebSocket transport (recommended for hosted services)
./target/release/openstreetmap-mcp serve --mode websocket --host 0.0.0.0 --port 8080
```

### Configuration

The public OSM endpoints enforce usage policies and rate limits. Each service
URL and the `User-Agent` can be overridden (CLI flag or environment variable) so
you can point at a self-hosted or commercial instance:

| Flag | Env var | Default |
| --- | --- | --- |
| `--nominatim-url` | `OSM_NOMINATIM_URL` | `https://nominatim.openstreetmap.org` |
| `--overpass-url` | `OSM_OVERPASS_URL` | `https://overpass-api.de/api/interpreter` |
| `--osrm-url` | `OSM_OSRM_URL` | `https://router.project-osrm.org` |
| `--user-agent` | `OSM_USER_AGENT` | `openstreetmap-mcp/<version>` |

> **Note:** The [Nominatim usage policy](https://operations.osmfoundation.org/policies/nominatim/)
> requires a descriptive, contactful `User-Agent` and limits you to ~1 request/second
> when using the public endpoint. For production load, run your own instances and
> point the flags at them.

## Logging

`mcp-core`'s `run` installs the process subscriber; this crate calls nothing
to get it. Logs go to stderr, never stdout — the stdio transport frames
JSON-RPC on stdout, and one log line there would corrupt the protocol
stream. `RUST_LOG` sets the level (default `info`); see `mcp-core`'s own
README for the full level contract, the request/tool-call spans, and the
standard `OTEL_*` environment variables.

What this server adds on top of what it inherits:

- A `debug!` line each time it starts an outbound request to Nominatim,
  Overpass, or OSRM — the one network call each tool makes. The query,
  coordinate, tag, or id in that request is a tool argument, so it stays at
  DEBUG and is never attached to a span; `RUST_LOG=debug` is what it takes
  to see it.
- `osm.upstream_failures`, a counter labelled `tool` and `reason`
  (`api_error`, `timeout`, `http_error`, `bad_response`, or `io_error`), for
  a fault reaching outward to one of the three upstream services. A "not
  found" result, a rejected coordinate, or any other decline is not counted
  here (rule 8.2) — only a genuine fault is.
- `mcp-core` already records a tool-call counter and a latency histogram by
  tool and outcome (`mcp.tools.call`, `mcp.tools.call.duration`); this server
  does not duplicate them.

### The `otel` feature

Off by default. A pure passthrough —
`openstreetmap-mcp -> mcp-core -> adelie-telemetry` — so this crate takes no
direct dependency on `adelie-telemetry` or on any opentelemetry crate. With
the feature off, `cargo tree` resolves no opentelemetry crate at all.

```bash
cargo build --features otel
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318 ./target/debug/openstreetmap-mcp serve --mode stdio
```

## Architecture

The crate mirrors the structure of its sibling MCP servers in this monorepo
(`fileio-mcp`, `geocode-mcp`):

- `src/main.rs` — CLI entrypoint, JSON-RPC loop, stdio + WebSocket transports.
- `src/server.rs` — MCP lifecycle (`initialize` / `tools/list` / `tools/call` / `shutdown`).
- `src/tools.rs` — tool schemas, argument parsing, and dispatch to operations.
- `src/config.rs` — OSM endpoint + identification configuration.
- `src/operations/` — one module per upstream call, each normalizing the response.
- `src/error.rs` — structured error types (`thiserror`).
- `src/transport.rs` — newline- and `Content-Length`-framed stdio transport.

## Testing

```bash
just check                       # default features: fmt, lint, build, test
just check-otel                  # the same, built with --features otel
just test-network                # additionally run the live OSM integration tests
```

Network-dependent integration tests are gated behind `RUN_NETWORK_TESTS=1` so the
default suite is deterministic and offline. The `tests/telemetry_*.rs` files are
the telemetry acceptance suite: that stdout carries only JSON-RPC at
`RUST_LOG=trace`; that no query, coordinate, tag, or id reaches an INFO line or a
span field, for every tool the server advertises; that `cargo tree` resolves no
opentelemetry crate by default; and that `osm.upstream_failures` is recorded
correctly against a local mock server (`tests/fixtures/nominatim/`), never a live
OSM service.

`just install-hooks` wires `check` into a pre-push git hook.

## License

Apache-2.0. See `LICENSE-APACHE` and `NOTICE`.
