//! Aircraft type profiles: the manufacturer's limits for one ICAO type
//! (`profiles/<ICAO>.yaml`), read by rules as `profile.*`. Every value is
//! unit-checked at load and names the document it comes from. A flight whose
//! type has no profile is graded by its class alone: type-specific values
//! are unknown and the rules using them stay silent.

use std::fmt;

use analyzer::Flight;
use fdm_core::{AircraftId, Unit};
use serde::Deserialize;

use crate::engine::aircraft;
use crate::expr;
use crate::rule::{Aircraft, Scalar};

/// The shipped profiles, every `profiles/*.yaml` as (type, YAML), embedded
/// by the build script.
const SHIPPED: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/profiles.rs"));

/// A type-specific value, read in rules as `profile.<name>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileField {
    /// Maximum flap-extended speed for the flap extension at that point,
    /// kt; unknown with the flaps retracted.
    Vfe,
    /// Body pitch at which the tail touches the runway with the main gear
    /// struts extended (rotation), deg.
    TailTakeoffPitch,
    /// Body pitch at which the tail touches the runway with the struts
    /// compressed (touchdown), deg.
    TailLandingPitch,
    /// Peak normal load from which the manufacturer requires a hard-landing
    /// inspection, g.
    HardLandingG,
}

impl ProfileField {
    const ALL: [Self; 4] = [
        Self::Vfe,
        Self::TailTakeoffPitch,
        Self::TailLandingPitch,
        Self::HardLandingG,
    ];

    /// The field named `name` in `profile.<name>`.
    pub(crate) fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| {
            name == match f {
                Self::Vfe => "vfe",
                Self::TailTakeoffPitch => "tail_takeoff_pitch",
                Self::TailLandingPitch => "tail_landing_pitch",
                Self::HardLandingG => "hard_landing_g",
            }
        })
    }

    pub fn unit(self) -> Unit {
        match self {
            Self::Vfe => Unit::Knots,
            Self::TailTakeoffPitch | Self::TailLandingPitch => Unit::Degrees,
            Self::HardLandingG => Unit::G,
        }
    }
}

/// A profile number with the document it comes from.
#[derive(Debug, Clone, PartialEq)]
pub struct Sourced {
    /// In the unit of its `ProfileField`.
    pub value: f64,
    pub source: String,
    pub note: Option<String>,
}

/// Flap extensions sharing one VFE. Limits are defined by flap position and
/// a flight data recorder samples the surface, not the lever, whose detent
/// order the sim does not guarantee.
#[derive(Debug, Clone, PartialEq)]
pub struct FlapBand {
    /// Trailing-edge angle up to which the band applies, deg; `None` for
    /// the last band to cover any larger extension.
    pub up_to: Option<f64>,
    pub vfe: Sourced,
}

/// Slack on a band's `up_to` for flap travel and rounding, deg. Also the
/// angle up to which the flaps count as retracted.
pub const FLAP_TOLERANCE_DEG: f64 = 0.5;

/// The limits of one aircraft type.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    /// ICAO Doc 8643 type designator, e.g. `C172`.
    pub icao: String,
    pub name: String,
    pub class: Aircraft,
    pub tail_takeoff_pitch: Option<Sourced>,
    pub tail_landing_pitch: Option<Sourced>,
    pub hard_landing_g: Option<Sourced>,
    /// VFE bands by increasing flap angle.
    pub flaps: Vec<FlapBand>,
}

impl Profile {
    /// The value of `field`; `flap_angle` gives the trailing-edge flap angle
    /// in deg and is read for `Vfe` only: unknown when retracted, else the
    /// first band reaching the angle (within the tolerance). `None` where the
    /// profile lacks the value.
    pub fn value(
        &self,
        field: ProfileField,
        flap_angle: impl FnOnce() -> Option<f64>,
    ) -> Option<f64> {
        let sourced = match field {
            ProfileField::Vfe => {
                let angle = flap_angle().filter(|a| *a > FLAP_TOLERANCE_DEG)?;
                self.flaps
                    .iter()
                    .find(|b| {
                        b.up_to
                            .is_none_or(|up_to| angle <= up_to + FLAP_TOLERANCE_DEG)
                    })
                    .map(|b| &b.vfe)
            }
            ProfileField::TailTakeoffPitch => self.tail_takeoff_pitch.as_ref(),
            ProfileField::TailLandingPitch => self.tail_landing_pitch.as_ref(),
            ProfileField::HardLandingG => self.hard_landing_g.as_ref(),
        };
        sourced.map(|s| s.value)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProfileError {
    /// The type designator the file is named after.
    pub profile: String,
    pub message: String,
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "profile `{}`: {}", self.profile, self.message)
    }
}

impl std::error::Error for ProfileError {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Doc {
    #[serde(rename = "type")]
    icao: String,
    name: String,
    class: Aircraft,
    tail_contact: Option<TailDoc>,
    hard_landing_g: Option<SourcedDoc>,
    #[serde(default)]
    flaps: Vec<FlapDoc>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TailDoc {
    takeoff: Option<SourcedDoc>,
    landing: Option<SourcedDoc>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourcedDoc {
    value: Scalar,
    source: String,
    note: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FlapDoc {
    up_to: Option<Scalar>,
    vfe: SourcedDoc,
}

/// Validates one file, attributing every error to its profile.
struct Compiler<'a> {
    profile: &'a str,
}

impl Compiler<'_> {
    fn err(&self, message: impl Into<String>) -> ProfileError {
        ProfileError {
            profile: self.profile.to_owned(),
            message: message.into(),
        }
    }

    fn quantity(&self, path: &str, unit: Unit, value: &Scalar) -> Result<f64, ProfileError> {
        expr::quantity(&value.to_string(), Some(unit), "value", "the field")
            .map_err(|m| self.err(format!("`{path}`:\n{m}")))
    }

    fn sourced(
        &self,
        path: &str,
        field: ProfileField,
        doc: SourcedDoc,
    ) -> Result<Sourced, ProfileError> {
        let value = self.quantity(path, field.unit(), &doc.value)?;
        if doc.source.trim().is_empty() {
            return Err(self.err(format!("`{path}` needs a source")));
        }
        Ok(Sourced {
            value,
            source: doc.source,
            note: doc.note,
        })
    }

    fn optional(
        &self,
        path: &str,
        field: ProfileField,
        doc: Option<SourcedDoc>,
    ) -> Result<Option<Sourced>, ProfileError> {
        doc.map(|d| self.sourced(path, field, d)).transpose()
    }

    fn flaps(&self, flaps: Vec<FlapDoc>) -> Result<Vec<FlapBand>, ProfileError> {
        let count = flaps.len();
        let mut previous = f64::NEG_INFINITY;
        flaps
            .into_iter()
            .enumerate()
            .map(|(i, band)| {
                let up_to = match band.up_to {
                    Some(angle) => {
                        let path = format!("flaps[{i}].up_to");
                        let angle = self.quantity(&path, Unit::Degrees, &angle)?;
                        if angle <= previous {
                            return Err(self.err(format!(
                                "`{path}`: bands must be in increasing order of angle"
                            )));
                        }
                        previous = angle;
                        Some(angle)
                    }
                    None if i + 1 == count => None,
                    None => {
                        return Err(
                            self.err(format!("`flaps[{i}]`: only the last band may omit `up_to`"))
                        );
                    }
                };
                let vfe = self.sourced(&format!("flaps[{i}].vfe"), ProfileField::Vfe, band.vfe)?;
                Ok(FlapBand { up_to, vfe })
            })
            .collect()
    }

    fn compile(&self, doc: Doc) -> Result<Profile, ProfileError> {
        if doc.icao != self.profile {
            return Err(self.err(format!(
                "`type: {}` does not match the file name `{}.yaml`",
                doc.icao, self.profile
            )));
        }
        let (takeoff, landing) = doc
            .tail_contact
            .map_or((None, None), |t| (t.takeoff, t.landing));
        Ok(Profile {
            tail_takeoff_pitch: self.optional(
                "tail_contact.takeoff",
                ProfileField::TailTakeoffPitch,
                takeoff,
            )?,
            tail_landing_pitch: self.optional(
                "tail_contact.landing",
                ProfileField::TailLandingPitch,
                landing,
            )?,
            hard_landing_g: self.optional(
                "hard_landing_g",
                ProfileField::HardLandingG,
                doc.hard_landing_g,
            )?,
            flaps: self.flaps(doc.flaps)?,
            icao: doc.icao,
            name: doc.name,
            class: doc.class,
        })
    }
}

/// The aircraft a flight is graded as.
#[derive(Debug, Clone)]
pub struct Selection<'a> {
    /// The recorded ICAO type designator; `None` without an identity.
    pub icao: Option<String>,
    /// `None` when the type has no profile.
    pub profile: Option<&'a Profile>,
    /// The profile's class, else the class by weight (`aircraft`).
    pub class: Option<Aircraft>,
}

impl fmt::Display for Selection<'_> {
    /// `C172 profile`, `transport class default (no B744 profile)`,
    /// `no aircraft identity, unknown class`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let missing = self
            .icao
            .as_ref()
            .map_or("no aircraft identity".to_owned(), |t| {
                format!("no {t} profile")
            });
        match (self.profile, self.class) {
            (Some(p), _) => write!(f, "{} profile", p.icao),
            (None, Some(class)) => write!(f, "{class} class default ({missing})"),
            (None, None) => write!(f, "{missing}, unknown class"),
        }
    }
}

/// Type profiles by ICAO designator.
#[derive(Debug, Clone)]
pub struct ProfileSet(Vec<Profile>);

impl ProfileSet {
    /// Loads (type designator, YAML) pairs, one per `<ICAO>.yaml` file.
    pub fn load<'a>(
        files: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<Self, ProfileError> {
        files
            .into_iter()
            .map(|(icao, yaml)| {
                let c = Compiler { profile: icao };
                let doc = serde_yaml_ng::from_str(yaml).map_err(|e| c.err(e.to_string()))?;
                c.compile(doc)
            })
            .collect::<Result<_, _>>()
            .map(Self)
    }

    /// The profiles shipped in `profiles/`.
    pub fn shipped() -> Result<Self, ProfileError> {
        Self::load(SHIPPED.iter().copied())
    }

    /// The profile of the flight's recorded type, with the class fallback.
    pub fn select(&self, flight: &Flight) -> Selection<'_> {
        let icao = flight.aircraft().and_then(AircraftId::icao_type);
        self.select_type(icao, || aircraft(flight))
    }

    /// The profile of type `icao`; without one, the class from `class`.
    pub fn select_type(
        &self,
        icao: Option<String>,
        class: impl FnOnce() -> Option<Aircraft>,
    ) -> Selection<'_> {
        let profile = icao
            .as_deref()
            .and_then(|t| self.0.iter().find(|p| p.icao == t));
        Selection {
            class: profile.map_or_else(class, |p| Some(p.class)),
            icao,
            profile,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROFILE: &str = r#"
type: TEST
name: Test aircraft
class: light
tail_contact:
  takeoff: { value: 11 deg, source: "doc A", note: "struts extended" }
hard_landing_g: { value: 2.2 g, source: "doc B" }
flaps:
  - { up_to: 10 deg, vfe: { value: 110 kt, source: "doc C" } }
  - { up_to: 20 deg, vfe: { value: 85 kt, source: "doc C" } }
"#;

    fn load(yaml: &str) -> Result<Profile, String> {
        ProfileSet::load([("TEST", yaml)])
            .map(|set| set.0[0].clone())
            .map_err(|e| e.to_string())
    }

    #[test]
    fn vfe_follows_the_flap_angle_bands() {
        let p = load(PROFILE).unwrap();
        let vfe = |angle: f64| p.value(ProfileField::Vfe, || Some(angle));
        // Retracted, within the tolerance: no VFE.
        assert_eq!(vfe(0.0), None);
        assert_eq!(vfe(0.5), None);
        assert_eq!(vfe(-3.0), None);
        // Inside each band, and the tolerance edge above `up_to`.
        assert_eq!(vfe(5.0), Some(110.0));
        assert_eq!(vfe(10.5), Some(110.0));
        assert_eq!(vfe(10.6), Some(85.0));
        assert_eq!(vfe(20.5), Some(85.0));
        // Beyond a closed last band, or no flap data: unknown.
        assert_eq!(vfe(20.6), None);
        assert_eq!(p.value(ProfileField::Vfe, || None), None);
        assert_eq!(p.value(ProfileField::TailTakeoffPitch, || None), Some(11.0));
        assert_eq!(p.value(ProfileField::TailLandingPitch, || None), None);
        assert_eq!(p.hard_landing_g.unwrap().source, "doc B");
    }

    #[test]
    fn the_open_last_band_covers_any_larger_extension() {
        let p = load(&PROFILE.replace("up_to: 20 deg, ", "")).unwrap();
        let vfe = |angle: f64| p.value(ProfileField::Vfe, || Some(angle));
        assert_eq!(vfe(10.6), Some(85.0));
        assert_eq!(vfe(40.0), Some(85.0));
        assert_eq!(vfe(0.0), None);
    }

    #[test]
    fn rejects_a_wrong_unit() {
        let err = load(&PROFILE.replace("110 kt", "110 ft")).unwrap_err();
        assert_eq!(
            err,
            "profile `TEST`: `flaps[0].vfe`:\nvalue `110 ft` is ft, but the field is kt"
        );
        assert!(
            load(&PROFILE.replace("2.2 g", "2.2"))
                .unwrap_err()
                .contains("is a plain number")
        );
    }

    #[test]
    fn every_value_needs_a_source() {
        let err = load(&PROFILE.replace(", source: \"doc B\"", "")).unwrap_err();
        assert!(err.contains("missing field `source`"), "{err}");
        let err = load(&PROFILE.replace("doc B", " ")).unwrap_err();
        assert_eq!(err, "profile `TEST`: `hard_landing_g` needs a source");
    }

    #[test]
    fn the_type_must_match_the_file_name() {
        let err = ProfileSet::load([("C172", PROFILE)]).unwrap_err();
        assert_eq!(
            err.to_string(),
            "profile `C172`: `type: TEST` does not match the file name `C172.yaml`"
        );
    }

    #[test]
    fn bands_must_increase_and_only_the_last_may_be_open() {
        let err = load(&PROFILE.replace("up_to: 20 deg", "up_to: 10 deg")).unwrap_err();
        assert_eq!(
            err,
            "profile `TEST`: `flaps[1].up_to`: bands must be in increasing order of angle"
        );
        let err = load(&PROFILE.replace("up_to: 10 deg, ", "")).unwrap_err();
        assert_eq!(
            err,
            "profile `TEST`: `flaps[0]`: only the last band may omit `up_to`"
        );
        let err = load(&PROFILE.replace("up_to: 10 deg", "up_to: 10 kt")).unwrap_err();
        assert!(err.contains("`flaps[0].up_to`"), "{err}");
    }

    #[test]
    fn rejects_unknown_fields() {
        let err = load(&format!("{PROFILE}vmo: 163 kt\n")).unwrap_err();
        assert!(err.contains("unknown field `vmo`"), "{err}");
    }

    #[test]
    fn shipped_profiles_load() {
        let set = ProfileSet::shipped().unwrap_or_else(|e| panic!("{e}"));
        let find = |icao: &str| set.0.iter().find(|p| p.icao == icao).unwrap();
        let classes: Vec<_> = set.0.iter().map(|p| (p.icao.as_str(), p.class)).collect();
        assert_eq!(
            classes,
            [
                ("B38M", Aircraft::Transport),
                ("B738", Aircraft::Transport),
                ("C172", Aircraft::Light),
                ("C25C", Aircraft::Transport),
                ("C700", Aircraft::Transport),
            ]
        );
        let vfe = |icao: &str| -> Vec<_> {
            [0.0, 10.0, 15.0, 30.0]
                .map(|a| find(icao).value(ProfileField::Vfe, || Some(a)))
                .to_vec()
        };
        assert_eq!(vfe("C172"), [None, Some(110.0), Some(85.0), Some(85.0)]);
        assert_eq!(vfe("C25C"), [None, Some(200.0), Some(200.0), Some(160.0)]);
        assert_eq!(vfe("C700"), [None, Some(230.0), Some(230.0), Some(180.0)]);
        assert_eq!(vfe("B738"), [None; 4]);
    }
}
