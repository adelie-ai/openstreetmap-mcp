// Routing between coordinates via OSRM.
// http://project-osrm.org/docs/v5.24.0/api/

use crate::config::OsmConfig;
use crate::error::{OsmError, Result};
use serde::Deserialize;
use serde_json::{Value, json};

/// Travel profiles supported by the OSRM HTTP API path segment.
const VALID_PROFILES: [&str; 3] = ["driving", "walking", "cycling"];

#[derive(Debug, Deserialize)]
struct OsrmResponse {
    code: String,
    message: Option<String>,
    #[serde(default)]
    routes: Vec<OsrmRoute>,
    #[serde(default)]
    waypoints: Vec<OsrmWaypoint>,
}

#[derive(Debug, Deserialize)]
struct OsrmRoute {
    distance: f64,
    duration: f64,
    geometry: Option<Value>,
    #[serde(default)]
    legs: Vec<OsrmLeg>,
}

#[derive(Debug, Deserialize)]
struct OsrmLeg {
    distance: f64,
    duration: f64,
    summary: Option<String>,
    #[serde(default)]
    steps: Vec<OsrmStep>,
}

#[derive(Debug, Deserialize)]
struct OsrmStep {
    distance: f64,
    duration: f64,
    name: Option<String>,
    maneuver: Option<OsrmManeuver>,
}

#[derive(Debug, Deserialize)]
struct OsrmManeuver {
    #[serde(rename = "type")]
    kind: Option<String>,
    modifier: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OsrmWaypoint {
    name: Option<String>,
    location: Option<Vec<f64>>,
}

/// Compute a route through an ordered list of `(latitude, longitude)`
/// waypoints (at least two).
///
/// `profile` must be one of `driving`, `walking`, `cycling`. When `steps` is
/// true, turn-by-turn step instructions are included in each leg.
pub async fn route(
    client: &reqwest::Client,
    config: &OsmConfig,
    coordinates: &[(f64, f64)],
    profile: &str,
    steps: bool,
) -> Result<Value> {
    if coordinates.len() < 2 {
        return Err(OsmError::InvalidParameters(
            "route requires at least two coordinates".to_string(),
        )
        .into());
    }
    if !VALID_PROFILES.contains(&profile) {
        return Err(OsmError::InvalidParameters(format!(
            "invalid profile '{}': expected one of {}",
            profile,
            VALID_PROFILES.join(", ")
        ))
        .into());
    }

    // OSRM expects coordinates as `lon,lat` pairs separated by `;`.
    let coord_path = coordinates
        .iter()
        .map(|(lat, lon)| format!("{},{}", lon, lat))
        .collect::<Vec<_>>()
        .join(";");

    let url = format!("{}/route/v1/{}/{}", config.osrm_base(), profile, coord_path);

    let resp = client
        .get(&url)
        .query(&[
            ("overview", "full"),
            ("geometries", "geojson"),
            ("steps", if steps { "true" } else { "false" }),
            ("annotations", "false"),
        ])
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        return Err(OsmError::ApiError(format!("OSRM returned HTTP {}", status)).into());
    }

    let body: OsrmResponse = resp.json().await?;
    if body.code != "Ok" {
        let detail = body.message.unwrap_or_else(|| body.code.clone());
        // `NoRoute` / `NoSegment` mean the points couldn't be connected; treat
        // those as a not-found rather than a generic API failure.
        return Err(
            OsmError::NotFound(format!("No route found ({}): {}", body.code, detail)).into(),
        );
    }

    let route = body
        .routes
        .into_iter()
        .next()
        .ok_or_else(|| OsmError::NotFound("OSRM returned no routes".to_string()))?;

    let legs: Vec<Value> = route
        .legs
        .into_iter()
        .map(|leg| {
            let steps_json: Vec<Value> = leg
                .steps
                .into_iter()
                .map(|s| {
                    let (kind, modifier) = s
                        .maneuver
                        .map(|m| (m.kind, m.modifier))
                        .unwrap_or((None, None));
                    json!({
                        "name": s.name,
                        "distance_meters": s.distance,
                        "duration_seconds": s.duration,
                        "maneuver": kind,
                        "modifier": modifier,
                    })
                })
                .collect();
            json!({
                "distance_meters": leg.distance,
                "duration_seconds": leg.duration,
                "summary": leg.summary,
                "steps": steps_json,
            })
        })
        .collect();

    let waypoints: Vec<Value> = body
        .waypoints
        .into_iter()
        .map(|w| {
            // OSRM waypoint location is [lon, lat]; expose named fields.
            let (latitude, longitude) = match w.location.as_deref() {
                Some([lon, lat, ..]) => (Some(*lat), Some(*lon)),
                _ => (None, None),
            };
            json!({
                "name": w.name,
                "latitude": latitude,
                "longitude": longitude,
            })
        })
        .collect();

    Ok(json!({
        "distance_meters": route.distance,
        "duration_seconds": route.duration,
        "geometry": route.geometry,
        "waypoints": waypoints,
        "legs": legs,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::test_capture::capture_events;

    /// mcp-core#40: waypoint coordinates and the profile are tool arguments,
    /// so the per-request log must stay at DEBUG.
    #[test]
    fn log_route_request_puts_the_waypoints_at_debug_only() {
        const SENTINEL_LATITUDE: f64 = 12.34908675;
        const SENTINEL_LONGITUDE: f64 = -56.78091234;
        let waypoints = [(SENTINEL_LATITUDE, SENTINEL_LONGITUDE), (0.0, 0.0)];
        let events = capture_events(|| {
            super::log_route_request(
                "https://example.com/route/v1/driving/...",
                "driving",
                &waypoints,
            )
        });

        assert_eq!(
            events.len(),
            1,
            "querying osrm route must log exactly one event: {events:?}"
        );
        let event = &events[0];
        assert_eq!(
            event.level,
            tracing::Level::DEBUG,
            "the outbound route request must log at DEBUG, so it stays off the INFO band"
        );
        assert_eq!(
            event.fields.get("profile").map(String::as_str),
            Some("driving"),
            "the event must carry the travel profile: {event:?}"
        );
        let waypoints_field = event
            .fields
            .get("waypoints")
            .expect("the event must carry the waypoints field");
        assert!(
            waypoints_field.contains(&SENTINEL_LATITUDE.to_string())
                && waypoints_field.contains(&SENTINEL_LONGITUDE.to_string()),
            "the event must carry the coordinates that were routed: {event:?}"
        );
    }

    #[tokio::test]
    async fn rejects_single_coordinate() {
        let client = reqwest::Client::new();
        let config = OsmConfig::default();
        let res = route(&client, &config, &[(51.5, -0.12)], "driving", false).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn rejects_invalid_profile() {
        let client = reqwest::Client::new();
        let config = OsmConfig::default();
        let res = route(
            &client,
            &config,
            &[(51.5, -0.12), (48.85, 2.35)],
            "teleport",
            false,
        )
        .await;
        assert!(res.is_err());
    }
}
