# openstreetmap-mcp Result Shapes

Every tool returns its payload inside the MCP content envelope as a single
`type: "json"` entry:

```json
{ "content": [ { "type": "json", "value": <payload> } ] }
```

The `<payload>` shapes are described below. Coordinates are decimal degrees
(WGS84). On failure (no match, bad parameters, upstream error) the tool returns a
JSON-RPC error instead of a result.

## `osm_search` / `osm_lookup` → array of Place

```json
[
  {
    "place_id": 97582011,
    "osm_type": "way",
    "osm_id": 5013364,
    "latitude": 48.8582599,
    "longitude": 2.2945006,
    "category": "man_made",
    "type": "tower",
    "name": "Tour Eiffel",
    "display_name": "Tour Eiffel, 5, Avenue Anatole France, …, France",
    "place_rank": 30,
    "importance": 0.62,
    "address_type": "man_made",
    "boundingbox": { "south": 48.8574753, "north": 48.8590453, "west": 2.2933119, "east": 2.2956897 },
    "address": { "road": "Avenue Anatole France", "city": "Paris", "country": "France", "country_code": "fr", "postcode": "75007" }
  }
]
```

`osm_search` returns relevance-ordered matches; `osm_lookup` returns one entry
per requested id. `boundingbox` and `address` may be `null` for sparse objects.

## `osm_reverse` → single Place

Same Place object as above (not wrapped in an array), for the place nearest the
queried coordinate.

## `osm_nearby` → array of Feature (nearest first)

```json
[
  {
    "osm_type": "node",
    "osm_id": 1234567890,
    "name": "Monmouth Coffee",
    "latitude": 51.5142,
    "longitude": -0.1265,
    "distance_meters": 87.0,
    "tags": { "amenity": "cafe", "name": "Monmouth Coffee", "cuisine": "coffee_shop" }
  }
]
```

Sorted ascending by `distance_meters` from the query point. An empty array means
no matching features were found (this is a success, not an error). `name` is
`null` when the feature has no `name` tag.

## `osm_route` → Route

```json
{
  "distance_meters": 459123.4,
  "duration_seconds": 17640.2,
  "geometry": { "type": "LineString", "coordinates": [[-0.1278, 51.5074], …] },
  "waypoints": [
    { "name": "Strand", "latitude": 51.5074, "longitude": -0.1278 },
    { "name": "Rue de Rivoli", "latitude": 48.8566, "longitude": 2.3522 }
  ],
  "legs": [
    {
      "distance_meters": 459123.4,
      "duration_seconds": 17640.2,
      "summary": "A20, A28",
      "steps": [
        { "name": "Strand", "distance_meters": 120.0, "duration_seconds": 25.0, "maneuver": "depart", "modifier": null }
      ]
    }
  ]
}
```

`geometry` is GeoJSON (`overview=full`). `steps` is populated only when the call
sets `steps: true`; otherwise it is an empty array. `waypoints` are the input
points snapped to the road network.
