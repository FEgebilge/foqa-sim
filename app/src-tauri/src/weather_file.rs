//! The weather file a challenge's clean start names: what the sim's own
//! missions do (a `Weather.WPR` beside the flight, `UseWeatherFile=True`). A
//! preset of the sim's own has its wind from 270 whatever the runway, so the
//! file is written here, its layers those of the sim's preset of that name
//! (`fs-base-spb/WeatherPresets`) and its wind the challenge's.

use std::fmt::Write;

use challenge::{Sky, WindBand};

/// One cloud layer: coverage (0-1), base and top (m above ground),
/// scattering and density (0-1).
type Cloud = (f64, f64, f64, f64, f64);

struct Preset {
    /// mm/h.
    precipitation: f64,
    thunderstorms: f64,
    aerosol: f64,
    snow_cover: f64,
    /// Knots, from 270, when the challenge names no wind.
    wind_kt: f64,
    clouds: [Cloud; 3],
}

fn preset(sky: Sky) -> Preset {
    let (precipitation, thunderstorms, aerosol, snow_cover, wind_kt, clouds) = match sky {
        Sky::Clear => (
            0.0,
            0.0,
            1.0,
            0.0,
            2.0,
            [
                (0.0, 2500.0, 2500.0, 0.0, 0.0),
                (0.0, 7000.0, 7000.0, 0.0, 0.0),
                (0.0, 10000.0, 10250.0, 0.5, 0.0),
            ],
        ),
        Sky::Few => (
            0.0,
            0.0,
            1.0,
            0.0,
            4.0,
            [
                (0.15, 500.0, 3000.0, 0.9, 0.1),
                (0.1, 5000.0, 6559.0, 0.5, 0.1),
                (0.0, 10000.0, 10250.0, 0.5, 0.0),
            ],
        ),
        Sky::Scattered => (
            0.0,
            0.0,
            1.5,
            0.0,
            5.0,
            [
                (0.4, 457.2, 2133.6, 0.9, 1.0),
                (0.45, 3962.4, 5181.6, 0.25, 0.5),
                (0.3, 9144.0, 10058.4, 0.5, 0.5),
            ],
        ),
        Sky::Broken => (
            0.0,
            0.0,
            2.0,
            0.0,
            12.0,
            [
                (0.5, 457.2, 1828.8, 1.0, 0.5),
                (0.75, 914.4, 3657.6, 0.25, 0.5),
                (0.75, 3657.6, 6097.0, 0.25, 0.0),
            ],
        ),
        Sky::High => (
            0.0,
            0.0,
            1.0,
            0.0,
            6.0,
            [
                (0.0, 2500.0, 2500.0, 0.9, 1.0),
                (0.0, 8000.0, 8000.0, 0.7, 1.0),
                (0.9, 9144.0, 10058.4, 0.5, 0.1),
            ],
        ),
        Sky::Overcast => (
            0.0,
            0.0,
            2.5,
            0.0,
            8.0,
            [
                (0.5, 457.2, 914.4, 1.0, 0.5),
                (0.95, 1219.0, 2438.4, 0.01, 1.0),
                (0.75, 3048.0, 4876.8, 0.0, 0.5),
            ],
        ),
        Sky::Rain => (
            10.0,
            0.0,
            3.0,
            0.0,
            10.0,
            [
                (0.7, 457.2, 1524.0, 0.9, 0.5),
                (0.5, 914.4, 4572.0, 0.5, 1.0),
                (0.0, 3048.0, 5486.4, 0.6, 0.5),
            ],
        ),
        Sky::Storm => (
            30.0,
            0.1,
            3.5,
            0.0,
            30.0,
            [
                (0.5, 457.2, 3048.0, 0.0, 1.0),
                (0.35, 914.4, 15240.0, 0.0, 1.0),
                (0.05, 6096.0, 12192.0, 0.0, 0.5),
            ],
        ),
        Sky::Snow => (
            15.0,
            0.0,
            3.0,
            0.5,
            5.0,
            [
                (0.7, 535.0, 1282.0, 0.0, 1.0),
                (0.5, 1439.0, 2848.0, 0.8, 0.5),
                (0.0, 10000.0, 10250.0, 0.5, 0.0),
            ],
        ),
    };
    Preset {
        precipitation,
        thunderstorms,
        aerosol,
        snow_cover,
        wind_kt,
        clouds,
    }
}

/// The `.WPR` of `sky` with `wind` blowing, or the preset's own light wind.
pub fn wpr(sky: Sky, wind: Option<&WindBand>) -> String {
    let p = preset(sky);
    let (from_deg, speed_kt) = wind.map_or((270.0, p.wind_kt), |w| (w.from_deg, w.speed_kt));
    let mut x = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <SimBase.Document Type=\"WeatherPreset\" version=\"1,3\">\n\
         <Descr>AceXML Document</Descr>\n\
         <WeatherPreset.Preset>\n\
         <Name>FOQA-Sim challenge</Name>\n\
         <IsAltitudeAMGL>True</IsAltitudeAMGL>\n",
    );
    let value = |x: &mut String, tag: &str, v: f64, unit: &str| {
        let _ = writeln!(x, "<{tag} Value=\"{v:.3}\" Unit=\"{unit}\"></{tag}>");
    };
    value(&mut x, "Precipitations", p.precipitation, "mm/h");
    value(&mut x, "ThunderstormIntensity", p.thunderstorms, "(0 - 1)");
    value(&mut x, "AerosolDensity", p.aerosol, "m");
    value(&mut x, "SnowCover", p.snow_cover, "(0 - 1)");
    value(&mut x, "MSLTemperature", 288.15, "k");
    value(&mut x, "MSLPressure", 101325.0, "pa");
    for (coverage, base, top, scattering, density) in p.clouds {
        x.push_str("<CloudLayer>\n");
        value(&mut x, "CloudLayerCoverage", coverage, "(0 - 1)");
        value(&mut x, "CloudLayerAltitudeBot", base, "m");
        value(&mut x, "CloudLayerAltitudeTop", top, "m");
        value(&mut x, "CloudLayerScattering", scattering, "(0 - 1)");
        value(&mut x, "CloudLayerDensity", density, "(0 - 1)");
        x.push_str("</CloudLayer>\n");
    }
    x.push_str("<WindLayer>\n");
    value(&mut x, "WindLayerAltitude", 0.0, "m");
    value(&mut x, "WindLayerAngle", from_deg, "degrees");
    value(&mut x, "WindLayerSpeed", speed_kt, "knts");
    x.push_str("</WindLayer>\n</WeatherPreset.Preset>\n</SimBase.Document>\n");
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_challenges_wind_replaces_the_presets() {
        let wind = WindBand {
            from_deg: 40.0,
            speed_kt: 12.0,
            tolerance_deg: 20.0,
            tolerance_kt: 4.0,
        };
        let set = wpr(Sky::Rain, Some(&wind));
        assert!(set.contains(r#"<WindLayerAngle Value="40.000""#));
        assert!(set.contains(r#"<WindLayerSpeed Value="12.000""#));
        assert!(set.contains(r#"<Precipitations Value="10.000""#));
        let own = wpr(Sky::Storm, None);
        assert!(own.contains(r#"<WindLayerAngle Value="270.000""#));
        assert!(own.contains(r#"<WindLayerSpeed Value="30.000""#));
        assert!(own.contains(r#"<ThunderstormIntensity Value="0.100""#));
        assert_eq!(own.matches("<CloudLayer>").count(), 3);
    }
}
