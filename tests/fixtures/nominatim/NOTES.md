# Nominatim fixtures

Used by `tests/telemetry_metrics.rs` to drive the upstream-failure metric
tests against a local mock server, so no test reaches a live OpenStreetMap
service.

`search_success.json` mirrors the shape of a live Nominatim `jsonv2`
`/search` response, captured on 2026-08-07 against
`https://nominatim.openstreetmap.org/search` (one real result), then rebuilt
with fictional values. No real place name, coordinate, id, or address
appears in this file. Nominatim does not publish a version number on its
response; the endpoint observed was the public instance's `jsonv2` format.

`search_empty.json` is Nominatim's real empty-result shape (a bare `[]`),
confirmed live on 2026-08-07 against a nonsense query.

The rate-limited (429) and server-error (500) cases used by the same test
file do not need a fixture file: a live capture on 2026-08-07 showed
Nominatim's `/reverse` error path returns a small JSON object,
`{"error": "Unable to geocode"}`, and its non-2xx statuses carry a plain-text
body, not JSON, so the tests build those two directly as short inline
strings rather than files.
