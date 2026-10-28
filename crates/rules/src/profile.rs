//! Aircraft type profiles: the manufacturer's limits for one ICAO type
//! (`profiles/<ICAO>.yaml`), read by rules as `profile.*`. Every value is
//! unit-checked at load and names the document it comes from. A flight whose
//! type has no profile is graded by its class alone: type-specific values
//! are unknown and the rules using them stay silent. A profile may also
//! move its class's phase boundaries (`phases:`). A flight is matched to a
//! profile by its type designator, then by a profile's aliases; without one,
//! the sim's own airframe facts and weights give its class (`Identified`).

use std::fmt;

use analyzer::{Flight, PhaseConfig};
use fdm_core::{AircraftId, Lfl, SignalId, SimFacts, Unit};
use serde::{Deserialize, Serialize};

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
    /// Maximum operating speed (VMO; VNE without one), kt IAS.
    Vmo,
    /// Maximum speed with the landing gear extended (VLE), kt.
    Vle,
    /// Maximum speed while the gear moves (VLO; the lower of extension and
    /// retraction), kt.
    Vlo,
    /// Maximum demonstrated crosswind for landing, kt.
    MaxCrosswind,
    /// Maximum tailwind for takeoff and landing, kt.
    MaxTailwind,
    /// Stalling speed in the landing configuration at maximum weight, kt
    /// IAS; gives the approach speed (1.3 VSO) where none is published.
    Vso,
    /// Distance between the main gear centrelines, ft.
    MainGearTrack,
    /// Vref at the gross weight and flap angle at that point, kt
    /// (`Profile::ref_speeds`); not a single value, so `value` and
    /// `sourced` leave it unknown.
    Vref,
    /// The approach speed the profile gives at that point (light types),
    /// kt; read as `Vref`.
    Vapp,
    /// Maximum landing weight (MLW), lb.
    Mlw,
    /// Maximum operating Mach number (MMO).
    Mmo,
    /// Height of the pilot's eye (or the glideslope antenna) above the main
    /// wheels on the approach, ft.
    EyeToWheel,
}

impl ProfileField {
    pub const ALL: [Self; 16] = [
        Self::Vfe,
        Self::TailTakeoffPitch,
        Self::TailLandingPitch,
        Self::HardLandingG,
        Self::Vmo,
        Self::Vle,
        Self::Vlo,
        Self::MaxCrosswind,
        Self::MaxTailwind,
        Self::Vso,
        Self::MainGearTrack,
        Self::Vref,
        Self::Vapp,
        Self::Mlw,
        Self::Mmo,
        Self::EyeToWheel,
    ];

    /// The field named `name` in `profile.<name>`.
    pub(crate) fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.name() == name)
    }

    /// Its `<name>` in `profile.<name>`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Vfe => "vfe",
            Self::TailTakeoffPitch => "tail_takeoff_pitch",
            Self::TailLandingPitch => "tail_landing_pitch",
            Self::HardLandingG => "hard_landing_g",
            Self::Vmo => "vmo",
            Self::Vle => "vle",
            Self::Vlo => "vlo",
            Self::MaxCrosswind => "max_crosswind",
            Self::MaxTailwind => "max_tailwind",
            Self::Vso => "vso",
            Self::MainGearTrack => "main_gear_track",
            Self::Vref => "vref",
            Self::Vapp => "vapp",
            Self::Mlw => "mlw",
            Self::Mmo => "mmo",
            Self::EyeToWheel => "eye_to_wheel",
        }
    }

    pub fn unit(self) -> Unit {
        match self {
            Self::Vfe
            | Self::Vmo
            | Self::Vle
            | Self::Vlo
            | Self::MaxCrosswind
            | Self::MaxTailwind
            | Self::Vso
            | Self::Vref
            | Self::Vapp => Unit::Knots,
            Self::TailTakeoffPitch | Self::TailLandingPitch => Unit::Degrees,
            Self::MainGearTrack | Self::EyeToWheel => Unit::Feet,
            Self::HardLandingG => Unit::G,
            Self::Mlw => Unit::Pounds,
            Self::Mmo => Unit::Mach,
        }
    }
}

/// A phase boundary of `PhaseConfig` that a profile may set (`phases:`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaseField {
    RollKt,
    RtoKt,
    ClimbEndFt,
    ApproachFt,
    FlareFt,
    GoAroundRiseFt,
    GoAroundDropFt,
}

impl PhaseField {
    pub const ALL: [Self; 7] = [
        Self::RollKt,
        Self::RtoKt,
        Self::ClimbEndFt,
        Self::ApproachFt,
        Self::FlareFt,
        Self::GoAroundRiseFt,
        Self::GoAroundDropFt,
    ];

    /// Its key under `phases:`, as the `PhaseConfig` field.
    pub fn name(self) -> &'static str {
        match self {
            Self::RollKt => "roll_kt",
            Self::RtoKt => "rto_kt",
            Self::ClimbEndFt => "climb_end_ft",
            Self::ApproachFt => "approach_ft",
            Self::FlareFt => "flare_ft",
            Self::GoAroundRiseFt => "go_around_rise_ft",
            Self::GoAroundDropFt => "go_around_drop_ft",
        }
    }

    pub fn unit(self) -> Unit {
        match self {
            Self::RollKt | Self::RtoKt => Unit::Knots,
            _ => Unit::Feet,
        }
    }

    fn of(self, c: &mut PhaseConfig) -> &mut f64 {
        match self {
            Self::RollKt => &mut c.roll_kt,
            Self::RtoKt => &mut c.rto_kt,
            Self::ClimbEndFt => &mut c.climb_end_ft,
            Self::ApproachFt => &mut c.approach_ft,
            Self::FlareFt => &mut c.flare_ft,
            Self::GoAroundRiseFt => &mut c.go_around_rise_ft,
            Self::GoAroundDropFt => &mut c.go_around_drop_ft,
        }
    }

    /// Its value in `c`.
    pub fn get(self, mut c: PhaseConfig) -> f64 {
        *self.of(&mut c)
    }
}

/// The phase boundaries of a class; transport where it is unknown, the
/// values every flight had before boundaries followed the aircraft.
pub fn class_phases(class: Option<Aircraft>) -> PhaseConfig {
    match class {
        Some(Aircraft::Light) => PhaseConfig::LIGHT,
        _ => PhaseConfig::TRANSPORT,
    }
}

/// CICTT ends the initial climb at 1000 ft or the pattern height,
/// whichever first.
const CLIMB_END_MAX_FT: f64 = 1000.0;

/// A profile number with the document it comes from.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Sourced {
    /// In the unit of its `ProfileField`.
    pub value: f64,
    pub source: String,
    /// Not the type's own published value but the closest one found (a
    /// sister variant, a secondary source); `note` says how it was derived.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub approximate: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
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

const LB_PER_KG: f64 = 2.204_622_621_8;
/// Approach speed from the stalling speed where none is published: 1.3
/// VSO (FAA-S-ACS-6C PA.IV.B.S7).
const VSO_FACTOR: f64 = 1.3;

/// The class a landing is scored in (docs/scoring.md §2.2); its bands and
/// leaderboards differ by class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ScoringClass {
    Light,
    /// Transport-category turboprop.
    Turboprop,
    Jet,
}

impl ScoringClass {
    /// The default of a rule class: light, else jet.
    pub fn of(class: Aircraft) -> Self {
        match class {
            Aircraft::Light => Self::Light,
            Aircraft::Transport => Self::Jet,
        }
    }
}

/// What flies: the landing score and the runway rules are for airplanes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Airplane,
    Helicopter,
    Glider,
    SeaplaneAmphibian,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    Piston,
    Turboprop,
    Jet,
    Electric,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Gear {
    Fixed,
    Retractable,
    Floats,
    Skids,
}

/// Facts about the airframe (not limits, so unsourced), each `None` where
/// unknown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Airframe {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<Kind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine: Option<Engine>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engines: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gear: Option<Gear>,
}

impl Airframe {
    /// More engines than any aircraft has (An-225: six).
    const MAX_ENGINES: u8 = 8;

    /// Field by field, `self` where known, else `other`.
    fn or(self, other: Self) -> Self {
        Self {
            kind: self.kind.or(other.kind),
            engine: self.engine.or(other.engine),
            engines: self.engines.or(other.engines),
            gear: self.gear.or(other.gear),
        }
    }

    /// From the sim's SimVars: `CATEGORY` and the gear say helicopter and
    /// seaplane; an airplane with no engine (`ENGINE TYPE` none, no
    /// engines) is a glider. A helicopter turbine, "unsupported" and
    /// unknown values leave the engine unknown.
    pub fn from_sim(f: &SimFacts) -> Self {
        let kind = match f.category.as_deref() {
            Some("Helicopter") => Some(Kind::Helicopter),
            _ if f.floats == Some(true) => Some(Kind::SeaplaneAmphibian),
            Some("Airplane") if f.engine_type == Some(2) && f.engines == Some(0) => {
                Some(Kind::Glider)
            }
            Some("Airplane") => Some(Kind::Airplane),
            _ => None,
        };
        let gear = if f.floats == Some(true) {
            Some(Gear::Floats)
        } else if f.skids == Some(true) {
            Some(Gear::Skids)
        } else {
            f.retractable
                .map(|r| if r { Gear::Retractable } else { Gear::Fixed })
        };
        Self {
            kind,
            engine: match f.engine_type {
                Some(0) => Some(Engine::Piston),
                Some(1) => Some(Engine::Jet),
                Some(5) => Some(Engine::Turboprop),
                _ => None,
            },
            engines: f.engines,
            gear,
        }
    }

    /// Lands on a runway the way the landing score and the runway rules
    /// grade it; an unknown kind is taken for an airplane.
    pub fn runway_landing(&self) -> bool {
        !matches!(self.kind, Some(Kind::Helicopter | Kind::Glider))
    }
}

/// The document a profile table comes from.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Cited {
    pub source: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub approximate: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Published final-approach speed ranges by flap angle (POH).
#[derive(Debug, Clone, PartialEq)]
pub struct ApproachSpeeds {
    pub cited: Cited,
    /// (flap angle deg, [lowest, highest] kt IAS), by increasing angle.
    pub flaps: Vec<(f64, [f64; 2])>,
}

/// Vref by gross weight for each landing flap (FCOM/QRH).
#[derive(Debug, Clone, PartialEq)]
pub struct VrefTable {
    pub cited: Cited,
    /// By increasing angle.
    pub flaps: Vec<VrefCurve>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VrefCurve {
    /// Landing flap angle, deg.
    pub flap: f64,
    /// Strictly increasing gross weights, lb, with their Vref in `kt`.
    pub weights_lb: Vec<f64>,
    pub kt: Vec<f64>,
}

/// The entry of `entries` whose flap angle (`flap`) is nearest `angle`,
/// and whether it is off by more than the flap tolerance.
fn nearest<T>(entries: &[T], flap: impl Fn(&T) -> f64, angle: f64) -> Option<(&T, bool)> {
    let off = |e: &T| (flap(e) - angle).abs();
    entries
        .iter()
        .min_by(|a, b| off(a).total_cmp(&off(b)))
        .map(|e| (e, off(e) > FLAP_TOLERANCE_DEG))
}

/// Piecewise-linear through the points (`xs` increasing, `ys`), held at
/// both ends; `NaN` without points.
pub fn interpolate(xs: &[f64], ys: &[f64], x: f64) -> f64 {
    let next = xs.partition_point(|&k| k <= x);
    match (next.checked_sub(1), xs.get(next)) {
        (Some(i), Some(&x1)) => ys[i] + (x - xs[i]) / (x1 - xs[i]) * (ys[next] - ys[i]),
        (Some(i), None) => ys[i],
        (None, Some(_)) => ys[0],
        (None, None) => f64::NAN,
    }
}

/// A landing's reference speeds (docs/scoring.md §5) from the profile.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RefSpeeds {
    /// kt IAS.
    pub vref: f64,
    /// Target approach speed where the profile gives it (light types: the
    /// middle of the published range, or 1.3 VSO); `None` for a Vref table,
    /// whose approach speed adds a wind correction.
    pub vapp: Option<f64>,
    /// From the table's or value's own approximation, or read outside the
    /// table (weight beyond its ends, a flap angle it lacks).
    pub approximate: bool,
    pub source: String,
}

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
    pub vmo: Option<Sourced>,
    pub vle: Option<Sourced>,
    pub vlo: Option<Sourced>,
    pub max_crosswind: Option<Sourced>,
    pub max_tailwind: Option<Sourced>,
    pub vso: Option<Sourced>,
    pub main_gear_track: Option<Sourced>,
    /// The profile's `scoring_class`, else the default of `class`.
    pub scoring_class: ScoringClass,
    pub approach_speed: Option<ApproachSpeeds>,
    pub vref: Option<VrefTable>,
    /// VFE bands by increasing flap angle.
    pub flaps: Vec<FlapBand>,
    /// The class's phase boundaries this type moves.
    pub phases: Vec<(PhaseField, Sourced)>,
    pub mlw: Option<Sourced>,
    pub mmo: Option<Sourced>,
    pub eye_to_wheel: Option<Sourced>,
    pub airframe: Airframe,
    /// `ATC MODEL` values and `TITLE` substrings that mean this type.
    pub aliases: Vec<String>,
    /// Signals known to be wrong on this type (an add-on that does not
    /// drive the standard SimVar), read as no data (`Selection::mask`).
    pub unreliable: Vec<SignalId>,
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
        self.sourced(field, flap_angle).map(|s| s.value)
    }

    /// `value` with its source.
    pub fn sourced(
        &self,
        field: ProfileField,
        flap_angle: impl FnOnce() -> Option<f64>,
    ) -> Option<&Sourced> {
        match field {
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
            ProfileField::Vmo => self.vmo.as_ref(),
            ProfileField::Vle => self.vle.as_ref(),
            ProfileField::Vlo => self.vlo.as_ref(),
            ProfileField::MaxCrosswind => self.max_crosswind.as_ref(),
            ProfileField::MaxTailwind => self.max_tailwind.as_ref(),
            ProfileField::Vso => self.vso.as_ref(),
            ProfileField::MainGearTrack => self.main_gear_track.as_ref(),
            ProfileField::Mlw => self.mlw.as_ref(),
            ProfileField::Mmo => self.mmo.as_ref(),
            ProfileField::EyeToWheel => self.eye_to_wheel.as_ref(),
            ProfileField::Vref | ProfileField::Vapp => None,
        }
    }

    /// Whether the profile gives `field` at all (some flap band, some
    /// reference speed source).
    pub fn has(&self, field: ProfileField) -> bool {
        match field {
            ProfileField::Vfe => !self.flaps.is_empty(),
            ProfileField::Vref | ProfileField::Vapp => {
                self.vref.is_some() || self.approach_speed.is_some() || self.vso.is_some()
            }
            f => self.sourced(f, || None).is_some(),
        }
    }

    /// Vref and the approach speed at a gross weight and landing flap
    /// angle, from the first source that has them, as Data4Safety orders
    /// them: the Vref table (by weight, linear and held at its ends, on the
    /// nearest flap angle), the published approach speeds (Vref their lowest,
    /// the approach speed their middle), 1.3 VSO. `None` without any.
    pub fn ref_speeds(&self, weight_lb: Option<f64>, flap_deg: Option<f64>) -> Option<RefSpeeds> {
        let speeds = |vref, vapp, approximate, source: &str| RefSpeeds {
            vref,
            vapp,
            approximate,
            source: source.to_owned(),
        };
        let table = self.vref.as_ref().zip(weight_lb.zip(flap_deg));
        if let Some((t, (w, flap))) = table
            && let Some((c, off)) = nearest(&t.flaps, |c| c.flap, flap)
        {
            let (lightest, heaviest) = (c.weights_lb[0], c.weights_lb[c.weights_lb.len() - 1]);
            let outside = off || !(lightest..=heaviest).contains(&w);
            let kt = interpolate(&c.weights_lb, &c.kt, w);
            return Some(speeds(
                kt,
                None,
                t.cited.approximate || outside,
                &t.cited.source,
            ));
        }
        let published = self.approach_speed.as_ref().zip(flap_deg);
        if let Some((a, flap)) = published
            && let Some(((_, [low, high]), off)) = nearest(&a.flaps, |e| e.0, flap)
        {
            let vapp = Some((low + high) / 2.0);
            return Some(speeds(
                *low,
                vapp,
                a.cited.approximate || off,
                &a.cited.source,
            ));
        }
        let vso = self.vso.as_ref()?;
        let vapp = VSO_FACTOR * vso.value;
        Some(speeds(
            vapp,
            Some(vapp),
            vso.approximate,
            &format!("1.3 VSO, {}", vso.source),
        ))
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
    vmo: Option<SourcedDoc>,
    vle: Option<SourcedDoc>,
    vlo: Option<SourcedDoc>,
    max_crosswind: Option<SourcedDoc>,
    max_tailwind: Option<SourcedDoc>,
    vso: Option<SourcedDoc>,
    main_gear_track: Option<SourcedDoc>,
    scoring_class: Option<ScoringClass>,
    approach_speed: Option<TableDoc<ApproachDoc>>,
    vref: Option<TableDoc<VrefDoc>>,
    #[serde(default)]
    flaps: Vec<FlapDoc>,
    #[serde(default)]
    phases: PhasesDoc,
    mlw: Option<SourcedDoc>,
    mmo: Option<SourcedDoc>,
    eye_to_wheel: Option<SourcedDoc>,
    #[serde(default)]
    airframe: Airframe,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    unreliable: Vec<String>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct PhasesDoc {
    roll_kt: Option<SourcedDoc>,
    rto_kt: Option<SourcedDoc>,
    climb_end_ft: Option<SourcedDoc>,
    approach_ft: Option<SourcedDoc>,
    flare_ft: Option<SourcedDoc>,
    go_around_rise_ft: Option<SourcedDoc>,
    go_around_drop_ft: Option<SourcedDoc>,
}

impl PhasesDoc {
    fn take(&mut self, f: PhaseField) -> Option<SourcedDoc> {
        match f {
            PhaseField::RollKt => self.roll_kt.take(),
            PhaseField::RtoKt => self.rto_kt.take(),
            PhaseField::ClimbEndFt => self.climb_end_ft.take(),
            PhaseField::ApproachFt => self.approach_ft.take(),
            PhaseField::FlareFt => self.flare_ft.take(),
            PhaseField::GoAroundRiseFt => self.go_around_rise_ft.take(),
            PhaseField::GoAroundDropFt => self.go_around_drop_ft.take(),
        }
    }
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
    #[serde(default)]
    approximate: bool,
    note: Option<String>,
}

/// A table with one source, one entry per flap angle.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TableDoc<E> {
    source: String,
    #[serde(default)]
    approximate: bool,
    note: Option<String>,
    flaps: Vec<E>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApproachDoc {
    flap: Scalar,
    ias: [Scalar; 2],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VrefDoc {
    flap: Scalar,
    weights_kg: Vec<f64>,
    ias: Vec<Scalar>,
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

    fn sourced(&self, path: &str, unit: Unit, doc: SourcedDoc) -> Result<Sourced, ProfileError> {
        let value = self.quantity(path, unit, &doc.value)?;
        let c = self.cited(path, doc.source, doc.approximate, doc.note)?;
        Ok(Sourced {
            value,
            source: c.source,
            approximate: c.approximate,
            note: c.note,
        })
    }

    /// A non-empty source, and a note on an approximate value.
    fn cited(
        &self,
        path: &str,
        source: String,
        approximate: bool,
        note: Option<String>,
    ) -> Result<Cited, ProfileError> {
        if source.trim().is_empty() {
            return Err(self.err(format!("`{path}` needs a source")));
        }
        if approximate && note.as_deref().is_none_or(|n| n.trim().is_empty()) {
            return Err(self.err(format!("`{path}` is approximate: its note must say how")));
        }
        Ok(Cited {
            source,
            approximate,
            note,
        })
    }

    /// A flap table: its citation and its entries (`entry` compiles one
    /// from its path and flap angle), at strictly increasing angles.
    fn table<E, T>(
        &self,
        path: &str,
        doc: TableDoc<E>,
        flap: impl Fn(&E) -> &Scalar,
        entry: impl Fn(&str, f64, E) -> Result<T, ProfileError>,
    ) -> Result<(Cited, Vec<T>), ProfileError> {
        let cited = self.cited(path, doc.source, doc.approximate, doc.note)?;
        if doc.flaps.is_empty() {
            return Err(self.err(format!("`{path}.flaps` needs at least one flap angle")));
        }
        let mut previous = f64::NEG_INFINITY;
        let entries = doc
            .flaps
            .into_iter()
            .enumerate()
            .map(|(i, e)| {
                let at = format!("{path}.flaps[{i}]");
                let angle = self.quantity(&format!("{at}.flap"), Unit::Degrees, flap(&e))?;
                if angle <= previous {
                    let message = format!("`{at}.flap`: flap angles must be in increasing order");
                    return Err(self.err(message));
                }
                previous = angle;
                entry(&at, angle, e)
            })
            .collect::<Result<_, _>>()?;
        Ok((cited, entries))
    }

    fn approach_speed(&self, doc: TableDoc<ApproachDoc>) -> Result<ApproachSpeeds, ProfileError> {
        let entry = |at: &str, flap, e: ApproachDoc| {
            let ias = |s| self.quantity(&format!("{at}.ias"), Unit::Knots, s);
            let [low, high] = [ias(&e.ias[0])?, ias(&e.ias[1])?];
            if low > high {
                return Err(self.err(format!("`{at}.ias`: lowest speed first")));
            }
            Ok((flap, [low, high]))
        };
        let (cited, flaps) = self.table("approach_speed", doc, |e| &e.flap, entry)?;
        Ok(ApproachSpeeds { cited, flaps })
    }

    fn vref(&self, doc: TableDoc<VrefDoc>) -> Result<VrefTable, ProfileError> {
        let entry = |at: &str, flap, e: VrefDoc| {
            if e.weights_kg.is_empty() || e.weights_kg.len() != e.ias.len() {
                let message = format!("`{at}`: needs one `ias` per weight, and at least one");
                return Err(self.err(message));
            }
            if e.weights_kg.windows(2).any(|w| w[1] <= w[0]) {
                let message = format!("`{at}.weights_kg`: weights must be strictly increasing");
                return Err(self.err(message));
            }
            let kt = e
                .ias
                .iter()
                .map(|s| self.quantity(&format!("{at}.ias"), Unit::Knots, s))
                .collect::<Result<_, _>>()?;
            Ok(VrefCurve {
                flap,
                weights_lb: e.weights_kg.iter().map(|kg| kg * LB_PER_KG).collect(),
                kt,
            })
        };
        let (cited, flaps) = self.table("vref", doc, |e| &e.flap, entry)?;
        Ok(VrefTable { cited, flaps })
    }

    fn optional(
        &self,
        path: &str,
        field: ProfileField,
        doc: Option<SourcedDoc>,
    ) -> Result<Option<Sourced>, ProfileError> {
        doc.map(|d| self.sourced(path, field.unit(), d)).transpose()
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
                let vfe = self.sourced(&format!("flaps[{i}].vfe"), Unit::Knots, band.vfe)?;
                Ok(FlapBand { up_to, vfe })
            })
            .collect()
    }

    /// The `phases:` overrides, checked merged with the class's defaults.
    fn phases(
        &self,
        class: Aircraft,
        mut doc: PhasesDoc,
    ) -> Result<Vec<(PhaseField, Sourced)>, ProfileError> {
        let mut set = Vec::new();
        let mut c = class_phases(Some(class));
        for f in PhaseField::ALL {
            if let Some(d) = doc.take(f) {
                let s = self.sourced(&format!("phases.{}", f.name()), f.unit(), d)?;
                *f.of(&mut c) = s.value;
                set.push((f, s));
            }
        }
        let wrong = if c.rto_kt < c.roll_kt {
            "`rto_kt` below `roll_kt`"
        } else if c.climb_end_ft > CLIMB_END_MAX_FT {
            "`climb_end_ft` above 1000 ft, where CICTT ends the initial climb"
        } else if c.approach_ft < c.climb_end_ft {
            "`approach_ft` below `climb_end_ft`"
        } else if c.flare_ft >= c.approach_ft {
            "`flare_ft` not below `approach_ft`"
        } else {
            return Ok(set);
        };
        Err(self.err(format!("`phases`: {wrong}")))
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
        let a = doc.airframe;
        if a.engines.is_some_and(|n| n > Airframe::MAX_ENGINES)
            || (a.engines == Some(0) && a.engine.is_some())
        {
            return Err(self.err(format!(
                "`airframe.engines`: 1 to {} with an `engine`, 0 only without one",
                Airframe::MAX_ENGINES
            )));
        }
        if let Some(bad) = doc.aliases.iter().find(|a| key(a).len() < MIN_ALIAS_LEN) {
            return Err(self.err(format!(
                "`aliases`: `{bad}` needs at least {MIN_ALIAS_LEN} letters or digits"
            )));
        }
        let unreliable = doc
            .unreliable
            .iter()
            .map(|name| {
                Lfl::V1
                    .by_name(name)
                    .map(|d| d.id)
                    .ok_or_else(|| self.err(format!("`unreliable`: unknown signal `{name}`")))
            })
            .collect::<Result<_, _>>()?;
        Ok(Profile {
            mlw: self.optional("mlw", ProfileField::Mlw, doc.mlw)?,
            mmo: self.optional("mmo", ProfileField::Mmo, doc.mmo)?,
            eye_to_wheel: self.optional(
                "eye_to_wheel",
                ProfileField::EyeToWheel,
                doc.eye_to_wheel,
            )?,
            airframe: a,
            aliases: doc.aliases,
            unreliable,
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
            vmo: self.optional("vmo", ProfileField::Vmo, doc.vmo)?,
            vle: self.optional("vle", ProfileField::Vle, doc.vle)?,
            vlo: self.optional("vlo", ProfileField::Vlo, doc.vlo)?,
            max_crosswind: self.optional(
                "max_crosswind",
                ProfileField::MaxCrosswind,
                doc.max_crosswind,
            )?,
            max_tailwind: self.optional(
                "max_tailwind",
                ProfileField::MaxTailwind,
                doc.max_tailwind,
            )?,
            vso: self.optional("vso", ProfileField::Vso, doc.vso)?,
            main_gear_track: self.optional(
                "main_gear_track",
                ProfileField::MainGearTrack,
                doc.main_gear_track,
            )?,
            scoring_class: doc.scoring_class.unwrap_or(ScoringClass::of(doc.class)),
            approach_speed: doc
                .approach_speed
                .map(|d| self.approach_speed(d))
                .transpose()?,
            vref: doc.vref.map(|d| self.vref(d)).transpose()?,
            flaps: self.flaps(doc.flaps)?,
            phases: self.phases(doc.class, doc.phases)?,
            icao: doc.icao,
            name: doc.name,
            class: doc.class,
        })
    }
}

/// Shorter aliases would match inside unrelated titles.
const MIN_ALIAS_LEN: usize = 3;

/// A name as aliases are compared: letters and digits only, upper case, so
/// `B737-800`, `b737 800` and `B737800` are one.
fn key(name: &str) -> String {
    name.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// How a flight's profile was found, or what its class comes from without
/// one: the first step of the chain that gave an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Identified {
    /// The `ATC MODEL` designator has a profile.
    Type,
    /// A profile alias is the `ATC MODEL`.
    Model,
    /// A profile alias is part of the `TITLE`.
    Title,
    /// No profile: the class and airframe from the sim's own facts (`MAX
    /// GROSS WEIGHT`, `ENGINE TYPE`, `CATEGORY`, gear).
    Sim,
    /// No profile, no sim facts: the class by the recorded gross weight.
    Weight,
    /// Nothing known: the transport defaults.
    Unknown,
}

/// The checks that need a profile value, in words, with the values they
/// read; `Selection::unchecked` lists those that cannot run.
const CHECKS: [(&str, &[ProfileField]); 9] = [
    ("flap speeds (Vfe)", &[ProfileField::Vfe]),
    ("maximum operating speed (Vmo)", &[ProfileField::Vmo]),
    (MMO_CHECK, &[ProfileField::Mmo]),
    (GEAR_CHECK, &[ProfileField::Vle, ProfileField::Vlo]),
    (
        "tail strike",
        &[
            ProfileField::TailTakeoffPitch,
            ProfileField::TailLandingPitch,
        ],
    ),
    ("hard-landing inspection", &[ProfileField::HardLandingG]),
    ("overweight landing", &[ProfileField::Mlw]),
    ("approach speed against Vref", &[ProfileField::Vref]),
    (
        "crosswind and tailwind limits",
        &[ProfileField::MaxCrosswind, ProfileField::MaxTailwind],
    ),
];
const MMO_CHECK: &str = "maximum Mach (Mmo)";
const GEAR_CHECK: &str = "gear speeds (Vle, Vlo)";

/// The aircraft a flight is graded as.
#[derive(Debug, Clone)]
pub struct Selection<'a> {
    /// The ICAO type designator: the profile's, else the recorded one;
    /// `None` without either.
    pub icao: Option<String>,
    /// `None` when the type has no profile.
    pub profile: Option<&'a Profile>,
    /// The profile's class, else the class by the sim's maximum gross
    /// weight or by the recorded weight (`aircraft`).
    pub class: Option<Aircraft>,
    /// The class's phase boundaries with the profile's overrides.
    pub phases: PhaseConfig,
    pub identified: Identified,
    /// The profile's airframe, the sim's facts where it says nothing.
    pub airframe: Airframe,
}

impl Selection<'_> {
    /// The profile's scoring class; without one the class's default, a
    /// transport turboprop scored as one (transport where unknown, as the
    /// rules grade it).
    pub fn scoring_class(&self) -> ScoringClass {
        match (self.profile, self.class.unwrap_or(Aircraft::Transport)) {
            (Some(p), _) => p.scoring_class,
            (None, Aircraft::Transport) if self.airframe.engine == Some(Engine::Turboprop) => {
                ScoringClass::Turboprop
            }
            (None, class) => ScoringClass::of(class),
        }
    }

    /// The checks of `CHECKS` that cannot run for want of a profile value;
    /// the gear speeds only for retractable or unknown gear, Mmo only for a
    /// jet or an unknown engine.
    pub fn unchecked(&self) -> Vec<&'static str> {
        let a = self.airframe;
        let moot = |what: &str| match what {
            GEAR_CHECK => a.gear.is_some_and(|g| g != Gear::Retractable),
            MMO_CHECK => a.engine.is_some_and(|e| e != Engine::Jet),
            _ => false,
        };
        let given = |f: &ProfileField| self.profile.is_some_and(|p| p.has(*f));
        CHECKS
            .iter()
            .filter(|(what, fields)| !moot(what) && !fields.iter().any(given))
            .map(|(what, _)| *what)
            .collect()
    }

    /// Marks the profile's unreliable signals failed in `flight`, so the
    /// rules, the assessment and the charts read them as no data.
    pub fn mask(&self, flight: &mut Flight) {
        for &id in self.profile.iter().flat_map(|p| &p.unreliable) {
            flight.invalidate(id);
        }
    }

    /// The phase boundary `f` in effect and where it comes from: the
    /// profile and its document (`PC12 profile, FAA AIM 4-3-3`) or the
    /// class default (`light class default`).
    pub fn phase_value(&self, f: PhaseField) -> Sourced {
        let set = self
            .profile
            .and_then(|p| Some((p, &p.phases.iter().find(|(g, _)| *g == f)?.1)));
        match set {
            Some((p, s)) => Sourced {
                source: format!("{} profile, {}", p.icao, s.source),
                ..s.clone()
            },
            None => Sourced {
                value: f.get(self.phases),
                source: match self.class {
                    Some(class) => format!("{class} class default"),
                    None => "transport class default (unknown class)".to_owned(),
                },
                approximate: false,
                note: None,
            },
        }
    }
}

impl fmt::Display for Selection<'_> {
    /// `C172 profile`, `B738 profile (by its ATC model)`,
    /// `transport class default (no B744 profile)`,
    /// `no aircraft identity, unknown class`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let missing = self
            .icao
            .as_ref()
            .map_or("no aircraft identity".to_owned(), |t| {
                format!("no {t} profile")
            });
        match (self.profile, self.class) {
            (Some(p), _) if self.identified == Identified::Model => {
                write!(f, "{} profile (by its ATC model)", p.icao)
            }
            (Some(p), _) if self.identified == Identified::Title => {
                write!(f, "{} profile (by its title)", p.icao)
            }
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
    /// Loads (type designator, YAML) pairs, one per `<ICAO>.yaml` file. An
    /// alias may name one type only, and not another profile's designator.
    pub fn load<'a>(
        files: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<Self, ProfileError> {
        let set: Vec<Profile> = files
            .into_iter()
            .map(|(icao, yaml)| {
                let c = Compiler { profile: icao };
                let doc = serde_yaml_ng::from_str(yaml).map_err(|e| c.err(e.to_string()))?;
                c.compile(doc)
            })
            .collect::<Result<_, _>>()?;
        let mut names: Vec<(String, &str)> = set.iter().map(|p| (key(&p.icao), &*p.icao)).collect();
        for p in &set {
            for alias in &p.aliases {
                let k = key(alias);
                if let Some((_, other)) = names.iter().find(|(n, _)| *n == k) {
                    return Err(ProfileError {
                        profile: p.icao.clone(),
                        message: format!("`aliases`: `{alias}` already names {other}"),
                    });
                }
                names.push((k, &p.icao));
            }
        }
        Ok(Self(set))
    }

    /// The profiles shipped in `profiles/`.
    pub fn shipped() -> Result<Self, ProfileError> {
        Self::load(SHIPPED.iter().copied())
    }

    /// The profile of the flight's recorded aircraft, with the fallbacks
    /// of `identify`.
    pub fn select(&self, flight: &Flight) -> Selection<'_> {
        self.identify(flight.aircraft(), || aircraft(flight))
    }

    /// The profile of `id`, tried in the order of `Identified`: its type
    /// designator, an alias equal to its `ATC MODEL`, the longest alias in
    /// its `TITLE`. Without one the class comes from the sim's maximum gross
    /// weight, else from `by_weight` (the recorded weight).
    pub fn identify(
        &self,
        id: Option<&AircraftId>,
        by_weight: impl FnOnce() -> Option<Aircraft>,
    ) -> Selection<'_> {
        let icao = id.and_then(AircraftId::icao_type);
        let aliased = |matches: &dyn Fn(&str) -> bool| {
            let found = self.0.iter().flat_map(|p| {
                p.aliases
                    .iter()
                    .map(|a| key(a))
                    .filter(|k| matches(k))
                    .map(move |k| (k.len(), p))
            });
            found.max_by_key(|(len, _)| *len).map(|(_, p)| p)
        };
        let found = icao
            .as_deref()
            .and_then(|t| self.by_type(t))
            .map(|p| (p, Identified::Type))
            .or_else(|| {
                let model = key(id?.model());
                Some((aliased(&|k| k == model)?, Identified::Model))
            })
            .or_else(|| {
                let title = key(&id?.title);
                Some((aliased(&|k| title.contains(k))?, Identified::Title))
            });
        let facts = id.map(|i| &i.facts);
        let sim = facts.map(Airframe::from_sim).unwrap_or_default();
        if let Some((p, identified)) = found {
            return self.selection(Some(p), Some(p.class), identified, sim);
        }
        let mtow = facts.and_then(|f| f.max_gross_lb).map(f64::from);
        let class = mtow.map(Aircraft::by_weight).or_else(by_weight);
        let identified = if mtow.is_some() || sim != Airframe::default() {
            Identified::Sim
        } else if class.is_some() {
            Identified::Weight
        } else {
            Identified::Unknown
        };
        Selection {
            icao,
            ..self.selection(None, class, identified, sim)
        }
    }

    /// The profile of type `icao`; without one, the class from `class`.
    pub fn select_type(
        &self,
        icao: Option<String>,
        class: impl FnOnce() -> Option<Aircraft>,
    ) -> Selection<'_> {
        let profile = icao.as_deref().and_then(|t| self.by_type(t));
        let class = profile.map_or_else(class, |p| Some(p.class));
        let identified = match (profile, class) {
            (Some(_), _) => Identified::Type,
            (None, Some(_)) => Identified::Weight,
            (None, None) => Identified::Unknown,
        };
        Selection {
            icao,
            ..self.selection(profile, class, identified, Airframe::default())
        }
    }

    fn by_type(&self, icao: &str) -> Option<&Profile> {
        self.0.iter().find(|p| p.icao == icao)
    }

    /// The selection of `profile` (in its class) or of `class`, the
    /// profile's airframe completed by the `sim`'s; a profile's turboprop
    /// scoring class says its engine.
    fn selection<'a>(
        &self,
        profile: Option<&'a Profile>,
        class: Option<Aircraft>,
        identified: Identified,
        sim: Airframe,
    ) -> Selection<'a> {
        let mut phases = class_phases(class);
        for (f, s) in profile.iter().flat_map(|p| &p.phases) {
            *f.of(&mut phases) = s.value;
        }
        let stated = profile.map_or_else(Airframe::default, |p| {
            let turboprop = p.scoring_class == ScoringClass::Turboprop;
            p.airframe.or(Airframe {
                engine: turboprop.then_some(Engine::Turboprop),
                ..Airframe::default()
            })
        });
        Selection {
            icao: profile.map(|p| p.icao.clone()),
            profile,
            class,
            phases,
            identified,
            airframe: stated.or(sim),
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
    fn an_approximate_value_says_how_it_was_derived() {
        let vmo = |extra: &str| {
            load(&format!(
                "{PROFILE}vmo: {{ value: 163 kt, source: \"doc D\", approximate: true{extra} }}\n"
            ))
        };
        let p = vmo(", note: \"from the 172R\"").unwrap();
        let v = p.sourced(ProfileField::Vmo, || None).unwrap();
        assert_eq!((v.value, v.approximate), (163.0, true));
        assert!(!p.hard_landing_g.unwrap().approximate);
        assert_eq!(
            vmo("").unwrap_err(),
            "profile `TEST`: `vmo` is approximate: its note must say how"
        );
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
        let err = load(&format!("{PROFILE}vne: 163 kt\n")).unwrap_err();
        assert!(err.contains("unknown field `vne`"), "{err}");
    }

    #[test]
    fn phases_override_the_class_defaults_and_are_checked() {
        let with = |phases: &str| {
            let yaml = format!("{PROFILE}phases:\n{phases}");
            ProfileSet::load([("TEST", yaml.as_str())]).map_err(|e| e.to_string())
        };
        let set = with("  approach_ft: { value: 1500 ft, source: AIM }\n").unwrap();
        let s = set.select_type(Some("TEST".into()), || None);
        let light = PhaseConfig::LIGHT;
        assert_eq!(
            s.phases,
            PhaseConfig {
                approach_ft: 1500.0,
                ..light
            }
        );
        let approach = s.phase_value(PhaseField::ApproachFt);
        assert_eq!(
            (approach.value, approach.source.as_str()),
            (1500.0, "TEST profile, AIM")
        );
        let flare = s.phase_value(PhaseField::FlareFt);
        assert_eq!(
            (flare.value, flare.source.as_str()),
            (20.0, "light class default")
        );

        let err = |phases: &str| with(phases).unwrap_err();
        assert!(err("  climb_end_ft: { value: 1500 ft, source: x }\n").contains("above 1000 ft"));
        assert!(
            err("  approach_ft: { value: 800 ft, source: x }\n").contains("below `climb_end_ft`")
        );
        assert!(
            err("  flare_ft: { value: 1000 ft, source: x }\n").contains("`flare_ft` not below")
        );
        assert!(
            err("  roll_kt: { value: 30 ft, source: x }\n").contains("is ft, but the field is kt")
        );
        assert!(
            err("  rto_kt: { value: 20 kt, source: x }\n").contains("`rto_kt` below `roll_kt`")
        );
        assert!(err("  flare: { value: 30 ft, source: x }\n").contains("unknown field `flare`"));
    }

    #[test]
    fn an_unknown_class_keeps_the_transport_boundaries() {
        let set = ProfileSet::load([]).unwrap();
        let s = set.select_type(None, || None);
        assert_eq!(s.phases, PhaseConfig::TRANSPORT);
        let source = s.phase_value(PhaseField::FlareFt).source;
        assert_eq!(source, "transport class default (unknown class)");
        let s = set.select_type(None, || Some(Aircraft::Light));
        assert_eq!(s.phases, PhaseConfig::LIGHT);
    }

    /// `PROFILE` as a transport type with a Vref table: flaps 30 from 40 t
    /// (120 kt) to 60 t (140 kt), flaps 40 at 50 t (125 kt).
    fn with_vref(table: &str) -> Result<Profile, String> {
        load(&format!(
            "{}vref:\n  source: \"doc E\"\n  flaps:\n{table}",
            PROFILE.replace("class: light", "class: transport")
        ))
    }

    const VREF: &str = "    - { flap: 30 deg, weights_kg: [40000, 60000], ias: [120 kt, 140 kt] }
    - { flap: 40 deg, weights_kg: [50000], ias: [125 kt] }
";

    #[test]
    fn vref_interpolates_by_weight_and_holds_at_the_ends() {
        let p = with_vref(VREF).unwrap();
        let at = |kg: f64, flap: f64| {
            let s = p.ref_speeds(Some(kg * LB_PER_KG), Some(flap)).unwrap();
            ((s.vref * 1e6).round() / 1e6, s.vapp, s.approximate)
        };
        assert_eq!(at(50_000.0, 30.0), (130.0, None, false));
        assert_eq!(at(60_000.0, 30.0), (140.0, None, false));
        // Beyond the table: held at its end, marked approximate.
        assert_eq!(at(30_000.0, 30.0), (120.0, None, true));
        assert_eq!(at(70_000.0, 30.0), (140.0, None, true));
        // A flap angle the table lacks reads the nearest, approximate.
        assert_eq!(at(50_000.0, 40.0), (125.0, None, false));
        assert_eq!(at(50_000.0, 37.0), (125.0, None, true));
        assert_eq!(p.ref_speeds(None, Some(30.0)), None);
        assert_eq!(p.ref_speeds(Some(1e5), Some(30.0)).unwrap().source, "doc E");
    }

    #[test]
    fn vref_tables_are_checked() {
        let err = |table: &str| with_vref(table).unwrap_err();
        assert_eq!(
            err("    - { flap: 30 deg, weights_kg: [60000, 40000], ias: [120 kt, 140 kt] }\n"),
            "profile `TEST`: `vref.flaps[0].weights_kg`: weights must be strictly increasing"
        );
        assert!(
            err("    - { flap: 30 deg, weights_kg: [40000, 60000], ias: [120 kt] }\n")
                .contains("needs one `ias` per weight")
        );
        assert!(err(&VREF.replace("30 deg", "45 deg")).contains("increasing order"));
        assert!(err(&VREF.replace("125 kt", "125 ft")).contains("`vref.flaps[1].ias`"));
        let yaml = format!("{PROFILE}vref:\n  source: \" \"\n  flaps:\n{VREF}");
        assert_eq!(
            load(&yaml).unwrap_err(),
            "profile `TEST`: `vref` needs a source"
        );
        let yaml = format!("{PROFILE}vref:\n  flaps:\n{VREF}");
        assert!(load(&yaml).unwrap_err().contains("missing field `source`"));
    }

    #[test]
    fn light_speeds_come_from_the_published_range_then_vso() {
        let speeds = "approach_speed:\n  source: POH\n  flaps:\n    \
                      - { flap: 0 deg, ias: [65 kt, 75 kt] }\n    \
                      - { flap: 30 deg, ias: [60 kt, 70 kt] }\nvso: { value: 50 kt, source: POH2 }\n";
        let p = load(&format!("{PROFILE}{speeds}")).unwrap();
        let at = |flap: f64| {
            let s = p.ref_speeds(None, Some(flap)).unwrap();
            (s.vref, s.vapp, s.approximate)
        };
        assert_eq!(at(30.0), (60.0, Some(65.0), false));
        assert_eq!(at(0.0), (65.0, Some(70.0), false));
        assert_eq!(at(20.0), (60.0, Some(65.0), true));
        // Without the flaps the range is unknown: 1.3 VSO.
        let s = p.ref_speeds(None, None).unwrap();
        assert_eq!((s.vref, s.vapp), (65.0, Some(65.0)));
        assert_eq!(s.source, "1.3 VSO, POH2");
        assert_eq!(
            load(PROFILE).unwrap().ref_speeds(Some(2000.0), Some(30.0)),
            None
        );
        let err = load(&format!(
            "{PROFILE}{}",
            speeds.replace("[65 kt, 75 kt]", "[75 kt, 65 kt]")
        ));
        assert!(err.unwrap_err().contains("lowest speed first"));
    }

    #[test]
    fn the_scoring_class_defaults_from_the_class() {
        assert_eq!(load(PROFILE).unwrap().scoring_class, ScoringClass::Light);
        let transport = PROFILE.replace("class: light", "class: transport");
        assert_eq!(load(&transport).unwrap().scoring_class, ScoringClass::Jet);
        let turboprop = format!("{transport}scoring_class: turboprop\n");
        assert_eq!(
            load(&turboprop).unwrap().scoring_class,
            ScoringClass::Turboprop
        );
        let err = load(&format!("{PROFILE}scoring_class: piston\n")).unwrap_err();
        assert!(err.contains("unknown variant `piston`"), "{err}");
    }

    const AIRFRAME: &str =
        "airframe: { kind: airplane, engine: jet, engines: 2, gear: retractable }
mlw: { value: 146300 lb, source: \"TCDS\" }
mmo: { value: 0.82 M, source: \"AFM\" }
eye_to_wheel: { value: 18 ft, source: \"FCTM\" }
aliases: [\"B737-800\", \"Boeing 737-800\"]
unreliable: [ap.engaged, sel.speed]
";

    #[test]
    fn airframe_weights_aliases_and_unreliable_signals_load() {
        let p = load(&format!("{PROFILE}{AIRFRAME}")).unwrap();
        assert_eq!(
            p.airframe,
            Airframe {
                kind: Some(Kind::Airplane),
                engine: Some(Engine::Jet),
                engines: Some(2),
                gear: Some(Gear::Retractable),
            }
        );
        let value = |f| p.value(f, || None);
        assert_eq!(value(ProfileField::Mlw), Some(146_300.0));
        assert_eq!(value(ProfileField::Mmo), Some(0.82));
        assert_eq!(value(ProfileField::EyeToWheel), Some(18.0));
        let names: Vec<_> = p
            .unreliable
            .iter()
            .map(|&id| Lfl::V1.get(id).unwrap().name)
            .collect();
        assert_eq!(names, ["ap.engaged", "sel.speed"]);
        // Every block is optional: an airframe says only what is known.
        assert_eq!(load(PROFILE).unwrap().airframe, Airframe::default());
        let gear = load(&format!("{PROFILE}airframe: {{ gear: floats }}\n")).unwrap();
        assert_eq!(gear.airframe.gear, Some(Gear::Floats));
    }

    #[test]
    fn airframe_weights_aliases_and_unreliable_signals_are_checked() {
        let err = |from: &str, to: &str| {
            load(&format!("{PROFILE}{}", AIRFRAME.replace(from, to))).unwrap_err()
        };
        assert!(err("airplane", "blimp").contains("unknown variant `blimp`"));
        assert!(err("engines: 2", "engines: 9").contains("`airframe.engines`: 1 to 8"));
        assert!(err("engines: 2", "engines: 0").contains("0 only without one"));
        assert!(err("gear: retractable", "wheels: 3").contains("unknown field `wheels`"));
        assert!(err("146300 lb", "146300 kg").contains("`mlw`"));
        assert!(err("0.82 M", "0.82").contains("is a plain number, but the field is M"));
        assert!(err("18 ft", "18 kt").contains("`eye_to_wheel`"));
        assert!(err("ap.engaged", "ap.on").contains("`unreliable`: unknown signal `ap.on`"));
        assert!(err("\"B737-800\"", "\"7-8\"").contains("`7-8` needs at least 3"));
        let err = err(", source: \"AFM\"", "");
        assert!(err.contains("missing field `source`"), "{err}");
    }

    #[test]
    fn an_alias_names_one_type() {
        let other = PROFILE.replace("type: TEST", "type: B738");
        let with = |aliases: &str| {
            let yaml = format!("{PROFILE}aliases: [{aliases}]\n");
            ProfileSet::load([("TEST", yaml.as_str()), ("B738", other.as_str())])
                .map_err(|e| e.to_string())
        };
        assert!(with("\"B737-800\"").is_ok());
        assert_eq!(
            with("\"b-738\"").unwrap_err(),
            "profile `TEST`: `aliases`: `b-738` already names B738"
        );
        let twice = format!("{other}aliases: [B737-800]\n");
        let both = format!("{PROFILE}aliases: [\"B737 800\"]\n");
        let err =
            ProfileSet::load([("TEST", both.as_str()), ("B738", twice.as_str())]).unwrap_err();
        assert_eq!(
            err.to_string(),
            "profile `B738`: `aliases`: `B737-800` already names TEST"
        );
    }

    fn id(title: &str, model: &str, facts: SimFacts) -> AircraftId {
        AircraftId {
            title: title.into(),
            atc_model: model.into(),
            facts,
            ..AircraftId::default()
        }
    }

    #[test]
    fn identification_tries_the_type_the_model_the_title_then_the_sim() {
        let b738 = format!("{}{AIRFRAME}", PROFILE.replace("type: TEST", "type: B738"));
        let a20n = format!(
            "{}aliases: [A320, A320neo]\n",
            PROFILE.replace("type: TEST", "type: A20N")
        );
        let set = ProfileSet::load([("B738", b738.as_str()), ("A20N", a20n.as_str())]).unwrap();
        let none = SimFacts::default;
        let pick = |id: &AircraftId| {
            let s = set.identify(Some(id), || Some(Aircraft::Light));
            (s.icao.clone(), s.identified, s.to_string())
        };
        let t = |icao: &str| Some(icao.to_owned());
        assert_eq!(
            pick(&id("", "ATCCOM.AC_MODEL B738.0.text", none())),
            (t("B738"), Identified::Type, "B738 profile".into())
        );
        assert_eq!(
            pick(&id("", "b737 800", none())),
            (
                t("B738"),
                Identified::Model,
                "B738 profile (by its ATC model)".into()
            )
        );
        // The longest alias in the title wins.
        assert_eq!(
            pick(&id("Airbus A320neo Lufthansa", "", none())),
            (
                t("A20N"),
                Identified::Title,
                "A20N profile (by its title)".into()
            )
        );
        // No profile: the sim's maximum weight outranks the recorded one.
        let heavy = SimFacts {
            max_gross_lb: Some(15_000),
            engine_type: Some(5),
            ..none()
        };
        let s = set.identify(Some(&id("King Air 350", "BE35", heavy)), || {
            Some(Aircraft::Light)
        });
        assert_eq!(
            (s.identified, s.class),
            (Identified::Sim, Some(Aircraft::Transport))
        );
        assert_eq!(s.to_string(), "transport class default (no BE35 profile)");
        assert_eq!(s.scoring_class(), ScoringClass::Turboprop);
        let s = set.identify(Some(&id("", "", none())), || Some(Aircraft::Light));
        assert_eq!(
            (s.identified, s.class),
            (Identified::Weight, Some(Aircraft::Light))
        );
        let s = set.identify(None, || None);
        assert_eq!(
            (s.identified, s.class, &s.icao),
            (Identified::Unknown, None, &None)
        );
        assert_eq!(s.scoring_class(), ScoringClass::Jet);
    }

    #[test]
    fn the_profile_airframe_comes_before_the_sim_facts() {
        let p = format!("{PROFILE}airframe: {{ gear: fixed }}\nscoring_class: turboprop\n");
        let set = ProfileSet::load([("TEST", p.as_str())]).unwrap();
        let facts = SimFacts {
            category: Some("Airplane".into()),
            engine_type: Some(0),
            retractable: Some(true),
            engines: Some(1),
            ..SimFacts::default()
        };
        let s = set.identify(Some(&id("", "TEST", facts)), || None);
        // The scoring class says turboprop; the sim fills only the unknowns.
        assert_eq!(
            s.airframe,
            Airframe {
                kind: Some(Kind::Airplane),
                engine: Some(Engine::Turboprop),
                engines: Some(1),
                gear: Some(Gear::Fixed),
            }
        );
    }

    #[test]
    fn sim_facts_give_the_kind_engine_and_gear() {
        let f = |category: &str, engine_type, engines, floats, skids| {
            Airframe::from_sim(&SimFacts {
                category: Some(category.into()),
                engine_type: Some(engine_type),
                engines: Some(engines),
                retractable: Some(false),
                floats: Some(floats),
                skids: Some(skids),
                max_gross_lb: None,
            })
        };
        let heli = f("Helicopter", 3, 1, false, true);
        assert_eq!(
            (heli.kind, heli.engine, heli.gear),
            (Some(Kind::Helicopter), None, Some(Gear::Skids))
        );
        assert!(!heli.runway_landing());
        let glider = f("Airplane", 2, 0, false, false);
        assert_eq!(
            (glider.kind, glider.gear),
            (Some(Kind::Glider), Some(Gear::Fixed))
        );
        assert!(!glider.runway_landing());
        let float = f("Airplane", 0, 1, true, false);
        assert_eq!(
            (float.kind, float.engine, float.gear),
            (
                Some(Kind::SeaplaneAmphibian),
                Some(Engine::Piston),
                Some(Gear::Floats)
            )
        );
        assert!(float.runway_landing());
        assert_eq!(f("Airplane", 1, 2, false, false).engine, Some(Engine::Jet));
        assert_eq!(f("Airplane", 4, 1, false, false).engine, None);
        assert_eq!(
            Airframe::from_sim(&SimFacts::default()),
            Airframe::default()
        );
        assert!(Airframe::default().runway_landing());
    }

    #[test]
    fn unchecked_lists_what_a_missing_profile_turns_off() {
        let set = ProfileSet::load([("TEST", PROFILE)]).unwrap();
        let s = set.select_type(Some("NONE".into()), || None);
        assert_eq!(s.unchecked().len(), CHECKS.len());
        let s = set.select_type(Some("TEST".into()), || None);
        assert_eq!(
            s.unchecked(),
            [
                "maximum operating speed (Vmo)",
                MMO_CHECK,
                GEAR_CHECK,
                "overweight landing",
                "approach speed against Vref",
                "crosswind and tailwind limits",
            ]
        );
        // Fixed gear has no gear speeds, a piston no Mmo.
        let s = Selection {
            airframe: Airframe {
                engine: Some(Engine::Piston),
                gear: Some(Gear::Fixed),
                ..Airframe::default()
            },
            ..s
        };
        assert!(
            !s.unchecked()
                .iter()
                .any(|c| [MMO_CHECK, GEAR_CHECK].contains(c))
        );
    }

    #[test]
    fn interpolation_holds_at_the_ends() {
        let (xs, ys) = ([1.0, 3.0], [10.0, 30.0]);
        assert_eq!(interpolate(&xs, &ys, 2.0), 20.0);
        assert_eq!(interpolate(&xs, &ys, 0.0), 10.0);
        assert_eq!(interpolate(&xs, &ys, 9.0), 30.0);
        assert!(interpolate(&[], &[], 1.0).is_nan());
    }

    #[test]
    fn shipped_profiles_load() {
        let set = ProfileSet::shipped().unwrap_or_else(|e| panic!("{e}"));
        let find = |icao: &str| set.0.iter().find(|p| p.icao == icao).unwrap();
        let classes: Vec<_> = set.0.iter().map(|p| (p.icao.as_str(), p.class)).collect();
        assert_eq!(
            classes,
            [
                ("A20N", Aircraft::Transport),
                ("A21N", Aircraft::Transport),
                ("A310", Aircraft::Transport),
                ("A320", Aircraft::Transport),
                ("A333", Aircraft::Transport),
                ("AT76", Aircraft::Transport),
                ("B350", Aircraft::Transport),
                ("B38M", Aircraft::Transport),
                ("B738", Aircraft::Transport),
                ("B748", Aircraft::Transport),
                ("B772", Aircraft::Transport),
                ("B77W", Aircraft::Transport),
                ("B78X", Aircraft::Transport),
                ("BE36", Aircraft::Light),
                ("BE58", Aircraft::Light),
                ("C152", Aircraft::Light),
                ("C172", Aircraft::Light),
                ("C208", Aircraft::Light),
                ("C25C", Aircraft::Transport),
                ("C700", Aircraft::Transport),
                ("DA62", Aircraft::Light),
                ("P28A", Aircraft::Light),
                ("PC12", Aircraft::Light),
                ("PC24", Aircraft::Transport),
                ("SF34", Aircraft::Transport),
                ("SF50", Aircraft::Transport),
                ("SR22", Aircraft::Light),
                ("TBM9", Aircraft::Light),
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
        assert_eq!(vfe("B738"), [None, Some(210.0), Some(200.0), Some(175.0)]);
        let approach = |icao: &str| {
            let s = set.select_type(Some(icao.into()), || None);
            s.phases.approach_ft
        };
        // Turbine patterns at 1500 ft (FAA AIM 4-3-3).
        let turbines = ["PC12", "TBM9", "C208", "C172"].map(approach);
        assert_eq!(turbines, [1500.0, 1500.0, 1500.0, 1000.0]);
        let scoring = ["C172", "AT76", "SF34", "B738"].map(|t| find(t).scoring_class);
        use ScoringClass::*;
        assert_eq!(scoring, [Light, Turboprop, Turboprop, Jet]);
        // C172S POH: flaps FULL 60-70 KIAS; VSO 40 KIAS.
        let c172 = find("C172").ref_speeds(None, Some(30.0)).unwrap();
        assert_eq!((c172.vref, c172.vapp), (60.0, Some(65.0)));
        assert_eq!(find("C172").value(ProfileField::Vso, || None), Some(40.0));
    }
}
