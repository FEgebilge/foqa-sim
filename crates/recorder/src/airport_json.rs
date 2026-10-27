//! Airport snapshots as JSON, the same encoding in the journal (event text)
//! and in the raw capture, so a replay reproduces the journal exactly.

use fdm_core::{Airport, Designator, Runway, RunwayEnd};
use serde_json::{Value, json};

pub(crate) fn to_json(a: &Airport) -> Value {
    let end = |e: &RunwayEnd| json!({ "number": e.number, "designator": e.designator.code(), "displaced_m": e.displaced_m });
    let runways: Vec<Value> = a
        .runways
        .iter()
        .map(|r| {
            json!({
                "lat_deg": r.lat_deg, "lon_deg": r.lon_deg, "elevation_m": r.elevation_m,
                "heading_deg": r.heading_deg, "length_m": r.length_m, "width_m": r.width_m,
                "primary": end(&r.primary), "secondary": end(&r.secondary),
            })
        })
        .collect();
    json!({
        "icao": a.icao, "region": a.region, "lat_deg": a.lat_deg, "lon_deg": a.lon_deg,
        "elevation_m": a.elevation_m, "magvar_deg": a.magvar_deg, "runways": runways,
    })
}

/// `None` if any field is missing or of the wrong type.
pub(crate) fn from_json(v: &Value) -> Option<Airport> {
    let num = |v: &Value, key| v.get(key)?.as_f64();
    let text = |key| Some(v.get(key)?.as_str()?.to_owned());
    let end = |e: &Value| {
        Some(RunwayEnd {
            number: u8::try_from(e.get("number")?.as_u64()?).ok()?,
            designator: Designator::from_code(i32::try_from(e.get("designator")?.as_i64()?).ok()?)?,
            displaced_m: num(e, "displaced_m")?,
        })
    };
    let runway = |r: &Value| {
        Some(Runway {
            lat_deg: num(r, "lat_deg")?,
            lon_deg: num(r, "lon_deg")?,
            elevation_m: num(r, "elevation_m")?,
            heading_deg: num(r, "heading_deg")?,
            length_m: num(r, "length_m")?,
            width_m: num(r, "width_m")?,
            primary: end(r.get("primary")?)?,
            secondary: end(r.get("secondary")?)?,
        })
    };
    Some(Airport {
        icao: text("icao")?,
        region: text("region")?,
        lat_deg: num(v, "lat_deg")?,
        lon_deg: num(v, "lon_deg")?,
        elevation_m: num(v, "elevation_m")?,
        magvar_deg: num(v, "magvar_deg")?,
        runways: v
            .get("runways")?
            .as_array()?
            .iter()
            .map(runway)
            .collect::<Option<_>>()?,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// LTBJ 16L/34R as MSFS reports it (verified live 2026-10-26).
    pub(crate) fn ltbj() -> Airport {
        let end = |number, designator| RunwayEnd {
            number,
            designator,
            displaced_m: 0.0,
        };
        Airport {
            icao: "LTBJ".into(),
            region: "LT".into(),
            lat_deg: 38.289167,
            lon_deg: 27.155,
            elevation_m: 117.0,
            magvar_deg: 354.4,
            runways: vec![Runway {
                lat_deg: 38.292379,
                lon_deg: 27.156957,
                elevation_m: 117.0,
                heading_deg: 166.74,
                length_m: 3240.0,
                width_m: 50.0,
                primary: end(16, Designator::Left),
                secondary: RunwayEnd {
                    displaced_m: 150.0,
                    ..end(34, Designator::Right)
                },
            }],
        }
    }

    #[test]
    fn round_trips_through_json() {
        assert_eq!(from_json(&to_json(&ltbj())), Some(ltbj()));
    }

    #[test]
    fn a_missing_field_is_rejected() {
        let mut v = to_json(&ltbj());
        v["runways"][0]["primary"]
            .as_object_mut()
            .unwrap()
            .remove("designator");
        assert_eq!(from_json(&v), None);
    }
}
