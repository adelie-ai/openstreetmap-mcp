#![deny(warnings)]

// Shared Nominatim response types and mapping.
//
// `search`, `reverse`, and `lookup` all return the same per-place object shape
// (Nominatim's `jsonv2` format), so the deserialization struct and the
// normalization into our public JSON shape live here once.

use serde::Deserialize;
use serde_json::{Value, json};

/// A single place as returned by Nominatim's `jsonv2` format.
///
/// Fields are all optional: Nominatim omits many of them depending on the
/// endpoint and the object, and we would rather degrade gracefully than fail
/// deserialization on a missing field.
#[derive(Debug, Deserialize)]
pub struct Place {
    pub place_id: Option<u64>,
    pub osm_type: Option<String>,
    pub osm_id: Option<u64>,
    pub lat: Option<String>,
    pub lon: Option<String>,
    pub category: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub place_rank: Option<u64>,
    pub importance: Option<f64>,
    pub addresstype: Option<String>,
    #[serde(default)]
    pub boundingbox: Option<Vec<String>>,
    #[serde(default)]
    pub address: Option<Value>,
}

/// Convert Nominatim's `[south, north, west, east]` string array into a named
/// object of floats, or `None` if it isn't the expected 4-element shape.
fn parse_boundingbox(bbox: &[String]) -> Option<Value> {
    if bbox.len() != 4 {
        return None;
    }
    let parse = |s: &String| s.parse::<f64>().ok();
    Some(json!({
        "south": parse(&bbox[0]),
        "north": parse(&bbox[1]),
        "west": parse(&bbox[2]),
        "east": parse(&bbox[3]),
    }))
}

/// Normalize a [`Place`] into the public JSON shape returned by the MCP tools.
///
/// Why: Nominatim returns `lat`/`lon` as strings and a positional bounding box;
/// we expose floats and a named bounding box so downstream agents don't have to
/// reparse strings or remember coordinate ordering.
pub fn place_to_json(p: Place) -> Value {
    let latitude = p.lat.as_deref().and_then(|s| s.parse::<f64>().ok());
    let longitude = p.lon.as_deref().and_then(|s| s.parse::<f64>().ok());
    let boundingbox = p.boundingbox.as_deref().and_then(parse_boundingbox);

    json!({
        "place_id": p.place_id,
        "osm_type": p.osm_type,
        "osm_id": p.osm_id,
        "latitude": latitude,
        "longitude": longitude,
        "category": p.category,
        "type": p.kind,
        "name": p.name,
        "display_name": p.display_name,
        "place_rank": p.place_rank,
        "importance": p.importance,
        "address_type": p.addresstype,
        "boundingbox": boundingbox,
        "address": p.address,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_boundingbox_in_south_north_west_east_order() {
        let bbox = vec![
            "51.0".to_string(),
            "52.0".to_string(),
            "-0.5".to_string(),
            "0.3".to_string(),
        ];
        let v = parse_boundingbox(&bbox).expect("4-element bbox parses");
        assert_eq!(v["south"], json!(51.0));
        assert_eq!(v["north"], json!(52.0));
        assert_eq!(v["west"], json!(-0.5));
        assert_eq!(v["east"], json!(0.3));
    }

    #[test]
    fn rejects_wrong_length_boundingbox() {
        assert!(parse_boundingbox(&["1.0".to_string()]).is_none());
    }

    #[test]
    fn maps_string_coords_to_floats() {
        let place = Place {
            place_id: Some(1),
            osm_type: Some("node".to_string()),
            osm_id: Some(42),
            lat: Some("51.5".to_string()),
            lon: Some("-0.12".to_string()),
            category: Some("place".to_string()),
            kind: Some("city".to_string()),
            name: Some("London".to_string()),
            display_name: Some("London, England".to_string()),
            place_rank: Some(16),
            importance: Some(0.9),
            addresstype: Some("city".to_string()),
            boundingbox: None,
            address: None,
        };
        let v = place_to_json(place);
        assert_eq!(v["latitude"], json!(51.5));
        assert_eq!(v["longitude"], json!(-0.12));
        assert_eq!(v["osm_type"], json!("node"));
    }
}
