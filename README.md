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
cargo test                       # unit + protocol/validation integration tests (no network)
just test-network                # additionally run the live OSM integration tests
```

Network-dependent integration tests are gated behind `RUN_NETWORK_TESTS=1` so the
default suite is deterministic and offline.

For local "CI", `just check` runs `fmt-check`, `lint`, `build`, and `test`;
`just install-hooks` wires it into a pre-push git hook.

## License

Apache-2.0. See `LICENSE-APACHE` and `NOTICE`.
