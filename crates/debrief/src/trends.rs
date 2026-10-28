//! The Trends screen's numbers (docs/trends.md): the flight summaries of one
//! pilot, filtered and aggregated the way an airline reads its FOQA
//! programme, in rates per flight rather than bare counts.

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use assess::DOMAINS;
use rules::{Domain, Level};
use serde::{Deserialize, Serialize};

use crate::assessment::{domain_short, domain_title};
use crate::{FlightSummary, LandingFacts, SkillsSummary, Station, iso_utc};

/// A comparison of the newest half with the older half needs this many in
/// each, else it is `None`.
const MIN_HALF: usize = 5;
const DAY_S: i64 = 86_400;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrendFilter {
    /// ICAO type designator.
    pub aircraft: Option<String>,
    /// A flight's departure or arrival, a landing's runway airport.
    pub airport: Option<String>,
    /// Flights that started within the last N days.
    pub days: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Trends {
    pub filter: TrendFilter,
    /// What the filters can be set to, from all flights.
    pub options: Options,
    pub flights: usize,
    pub hours: f64,
    pub landing_count: usize,
    /// Full-stop landings with a score: what the means are over.
    pub scored: usize,
    pub score: ScoreTrend,
    /// Oldest first.
    pub landings: Vec<TrendLanding>,
    pub domains: Vec<DomainTrend>,
    pub events: Vec<EventTrend>,
    pub by_aircraft: Vec<AircraftTrend>,
    pub by_airport: Vec<AirportTrend>,
    pub skills: SkillsTrend,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Options {
    pub aircraft: Vec<AircraftOption>,
    pub airports: Vec<AirportOption>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AircraftOption {
    pub icao: String,
    pub flights: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AirportOption {
    pub icao: String,
    pub landings: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScoreTrend {
    pub mean: Option<f64>,
    pub median: Option<f64>,
    pub recent: Option<f64>,
    pub earlier: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TrendLanding {
    pub flight_id: String,
    pub started_utc: Option<String>,
    pub aircraft: Option<String>,
    #[serde(flatten)]
    pub facts: LandingFacts,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DomainTrend {
    pub id: Domain,
    pub title: &'static str,
    pub short: &'static str,
    /// Landings in which the domain applied.
    pub graded: usize,
    pub mean_level: Option<f64>,
    pub level_2_plus: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EventTrend {
    pub rule: String,
    pub title: String,
    /// Its worst level in any flight.
    pub level: Level,
    /// Flights it fired in.
    pub flights: usize,
    /// Times it fired, repeats in one flight included.
    pub count: usize,
    /// Flights it fired in, per 100 flights: a rule that fires seven times
    /// in one flight is one flight with the event, as in sector rates.
    pub per_100: f64,
    pub recent_per_100: Option<f64>,
    pub earlier_per_100: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AircraftTrend {
    pub icao: String,
    pub flights: usize,
    pub landings: usize,
    pub mean_score: Option<f64>,
    pub events_per_flight: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AirportTrend {
    pub icao: String,
    pub landings: usize,
    pub mean_score: Option<f64>,
    /// Of the landings with a touchdown zone.
    pub in_tdz: Option<f64>,
    /// Signed: the average offset from the centreline, right positive.
    pub mean_cross_m: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SkillsTrend {
    /// Mean of the flights' autopilot shares of their airborne time.
    pub ap_share: Option<f64>,
    /// Over hand-flown landings: an autoland has no disconnect, so it is
    /// counted in `autolands` instead.
    pub ap_off_ft_median: Option<f64>,
    pub autolands: usize,
    pub vertical_rms_median: Option<f64>,
    pub descent_level_s_median: Option<f64>,
    /// Per type, by ICAO: fuel flow is not comparable across types.
    pub fuel_kg_h: Vec<FuelTrend>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FuelTrend {
    pub icao: String,
    pub median: f64,
}

/// A landing with the flight it belongs to.
type Row<'a> = (&'a FlightSummary, &'a LandingFacts);

/// The summaries `filter` selects, aggregated; `station` names the rules.
pub fn trends(summaries: &[FlightSummary], filter: &TrendFilter, station: &Station) -> Trends {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
    build(summaries, filter, station, now)
}

fn build(summaries: &[FlightSummary], filter: &TrendFilter, station: &Station, now: i64) -> Trends {
    let same = |a: &str, b: &Option<String>| b.as_deref().is_none_or(|b| a.eq_ignore_ascii_case(b));
    // ISO times in one format order as text.
    let since = filter
        .days
        .map(|d| iso_utc((now - i64::from(d) * DAY_S) * 1_000_000, false));
    let mut flights: Vec<&FlightSummary> = summaries
        .iter()
        .filter(|f| same(icao(f).unwrap_or(""), &filter.aircraft))
        .filter(|f| {
            filter.airport.is_none()
                || [&f.departure, &f.arrival]
                    .into_iter()
                    .flatten()
                    .any(|a| same(a, &filter.airport))
        })
        .filter(|f| {
            since
                .as_ref()
                .is_none_or(|s| f.started_utc.as_ref() >= Some(s))
        })
        .collect();
    flights.sort_by(|a, b| (&a.started_utc, &a.id).cmp(&(&b.started_utc, &b.id)));
    let rows: Vec<Row> = flights
        .iter()
        .flat_map(|&f| f.landings_detail.iter().map(move |l| (f, l)))
        .filter(|(_, l)| {
            filter.airport.is_none() || same(l.icao.as_deref().unwrap_or(""), &filter.airport)
        })
        .collect();
    let scored: Vec<Row> = rows
        .iter()
        .filter(|(_, l)| l.full_stop && l.score.is_some())
        .copied()
        .collect();
    let scores: Vec<f64> = scored.iter().filter_map(|(_, l)| l.score).collect();
    let halves = halves(&scores);

    Trends {
        filter: filter.clone(),
        options: options(summaries),
        flights: flights.len(),
        hours: r1(flights.iter().map(|f| f.duration_s).sum::<f64>() / 3600.0),
        landing_count: rows.len(),
        scored: scored.len(),
        score: ScoreTrend {
            mean: mean(&scores).map(r1),
            median: median(&scores).map(r1),
            recent: halves.and_then(|h| mean(h.1)).map(r1),
            earlier: halves.and_then(|h| mean(h.0)).map(r1),
        },
        landings: rows
            .iter()
            .map(|&(f, l)| TrendLanding {
                flight_id: f.id.clone(),
                started_utc: f.started_utc.clone(),
                aircraft: icao(f).map(str::to_owned),
                facts: l.clone(),
            })
            .collect(),
        domains: DOMAINS
            .map(|id| {
                let levels: Vec<f64> = scored
                    .iter()
                    .filter_map(|(_, l)| l.domains.get(&id).copied().flatten())
                    .map(f64::from)
                    .collect();
                DomainTrend {
                    id,
                    title: domain_title(id),
                    short: domain_short(id),
                    graded: levels.len(),
                    mean_level: mean(&levels).map(r2),
                    level_2_plus: levels.iter().filter(|&&l| l >= 2.0).count(),
                }
            })
            .into(),
        events: events(&flights, station),
        by_aircraft: by_aircraft(&flights, &rows, &scored),
        by_airport: by_airport(&rows),
        skills: skills_trend(&flights),
    }
}

/// The aircraft types and airports there are flights and landings for.
fn options(summaries: &[FlightSummary]) -> Options {
    let count = |keys: Vec<&str>| {
        let mut n: BTreeMap<&str, usize> = BTreeMap::new();
        for k in keys {
            *n.entry(k).or_default() += 1;
        }
        let mut n: Vec<_> = n.into_iter().map(|(k, c)| (k.to_owned(), c)).collect();
        n.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        n
    };
    Options {
        aircraft: count(summaries.iter().filter_map(|f| icao(f)).collect())
            .into_iter()
            .map(|(icao, flights)| AircraftOption { icao, flights })
            .collect(),
        airports: count(
            summaries
                .iter()
                .flat_map(|f| &f.landings_detail)
                .filter_map(|l| l.icao.as_deref())
                .collect(),
        )
        .into_iter()
        .map(|(icao, landings)| AirportOption { icao, landings })
        .collect(),
    }
}

/// Each rule's worst level, the flights it fired in and its total count.
fn rule_totals<'a>(flights: &[&'a FlightSummary]) -> BTreeMap<&'a str, (Level, usize, usize)> {
    let mut totals: BTreeMap<&str, (Level, usize, usize)> = BTreeMap::new();
    for (rule, c) in flights.iter().flat_map(|f| &f.rule_counts) {
        let t = totals.entry(rule).or_insert((c.level, 0, 0));
        *t = (t.0.max(c.level), t.1 + 1, t.2 + c.count);
    }
    totals
}

/// Flights with each event per 100 flights, by the newest and the older half of `flights`.
fn events(flights: &[&FlightSummary], station: &Station) -> Vec<EventTrend> {
    let per_100 = |count: usize, n: usize| r1(count as f64 * 100.0 / n as f64);
    let halves = halves(flights).map(|(earlier, recent)| {
        (
            rule_totals(earlier),
            earlier.len(),
            rule_totals(recent),
            recent.len(),
        )
    });
    let mut events: Vec<EventTrend> = rule_totals(flights)
        .into_iter()
        .map(|(rule, (level, in_flights, count))| {
            let half = |totals: &BTreeMap<&str, _>, n| {
                let count = totals.get(rule).map_or(0, |t: &(Level, usize, usize)| t.1);
                per_100(count, n)
            };
            EventTrend {
                title: station
                    .rules()
                    .iter()
                    .find(|r| r.id == rule)
                    .map_or(rule, |r| &r.title)
                    .to_owned(),
                rule: rule.to_owned(),
                level,
                flights: in_flights,
                count,
                per_100: per_100(in_flights, flights.len()),
                recent_per_100: halves.as_ref().map(|h| half(&h.2, h.3)),
                earlier_per_100: halves.as_ref().map(|h| half(&h.0, h.1)),
            }
        })
        .collect();
    events.sort_by(|a, b| {
        (b.flights, b.level, b.count, &a.rule).cmp(&(a.flights, a.level, a.count, &b.rule))
    });
    events
}

/// Landings and scores from the filtered rows, so an airport filter holds here too.
fn by_aircraft(flights: &[&FlightSummary], rows: &[Row], scored: &[Row]) -> Vec<AircraftTrend> {
    let mut types: BTreeMap<&str, Vec<&FlightSummary>> = BTreeMap::new();
    for f in flights {
        if let Some(icao) = icao(f) {
            types.entry(icao).or_default().push(f);
        }
    }
    let mut out: Vec<AircraftTrend> = types
        .into_iter()
        .map(|(icao, fs)| {
            let scores: Vec<f64> = scored
                .iter()
                .filter(|(f, _)| fs.iter().any(|x| x.id == f.id))
                .filter_map(|(_, l)| l.score)
                .collect();
            let events: usize = fs
                .iter()
                .flat_map(|f| f.rule_counts.values())
                .map(|c| c.count)
                .sum();
            AircraftTrend {
                icao: icao.to_owned(),
                flights: fs.len(),
                landings: rows
                    .iter()
                    .filter(|(f, _)| fs.iter().any(|x| x.id == f.id))
                    .count(),
                mean_score: mean(&scores).map(r1),
                events_per_flight: r2(events as f64 / fs.len() as f64),
            }
        })
        .collect();
    out.sort_by(|a, b| b.flights.cmp(&a.flights).then_with(|| a.icao.cmp(&b.icao)));
    out
}

fn by_airport(rows: &[Row]) -> Vec<AirportTrend> {
    let mut airports: BTreeMap<&str, Vec<&LandingFacts>> = BTreeMap::new();
    for (_, l) in rows {
        if let Some(icao) = &l.icao {
            airports.entry(icao).or_default().push(l);
        }
    }
    let mut out: Vec<AirportTrend> = airports
        .into_iter()
        .map(|(icao, ls)| {
            let scores: Vec<f64> = ls
                .iter()
                .filter(|l| l.full_stop)
                .filter_map(|l| l.score)
                .collect();
            let zones: Vec<f64> = ls
                .iter()
                .filter_map(|l| l.zone.as_deref())
                .map(|z| f64::from(u8::from(z == "In TDZ")))
                .collect();
            let cross: Vec<f64> = ls.iter().filter_map(|l| l.cross_m).collect();
            AirportTrend {
                icao: icao.to_owned(),
                landings: ls.len(),
                mean_score: mean(&scores).map(r1),
                in_tdz: mean(&zones).map(r2),
                mean_cross_m: mean(&cross).map(r1),
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.landings
            .cmp(&a.landings)
            .then_with(|| a.icao.cmp(&b.icao))
    });
    out
}

/// The flying skills of the flights (docs/skills.md): medians, so one
/// turbulent or hand-flown sector does not move them.
fn skills_trend(flights: &[&FlightSummary]) -> SkillsTrend {
    let of = |get: fn(&SkillsSummary) -> Option<f64>| -> Vec<f64> {
        flights.iter().filter_map(|f| get(&f.skills)).collect()
    };
    let mut types: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
    for f in flights {
        if let (Some(icao), Some(kg_h)) = (icao(f), f.skills.fuel_kg_h) {
            types.entry(icao).or_default().push(kg_h);
        }
    }
    SkillsTrend {
        ap_share: mean(&of(|s| s.ap_share)).map(r2),
        ap_off_ft_median: median(&of(|s| s.ap_off_ft)).map(r1),
        autolands: flights.iter().filter(|f| f.skills.autoland).count(),
        vertical_rms_median: median(&of(|s| s.vertical_rms)).map(r2),
        descent_level_s_median: median(&of(|s| s.descent_level_s)).map(r1),
        fuel_kg_h: types
            .into_iter()
            .filter_map(|(icao, v)| Some((icao, median(&v)?)))
            .map(|(icao, m)| FuelTrend {
                icao: icao.to_owned(),
                median: r1(m),
            })
            .collect(),
    }
}

fn icao(f: &FlightSummary) -> Option<&str> {
    f.aircraft.as_ref()?.icao.as_deref()
}

/// `(earlier, recent)`: `v` in time order split in two, `None` unless each
/// half has `MIN_HALF`.
fn halves<T>(v: &[T]) -> Option<(&[T], &[T])> {
    let (earlier, recent) = v.split_at(v.len() / 2);
    (earlier.len() >= MIN_HALF && recent.len() >= MIN_HALF).then_some((earlier, recent))
}

pub(crate) fn mean(v: &[f64]) -> Option<f64> {
    (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
}

pub(crate) fn median(v: &[f64]) -> Option<f64> {
    let mut v = v.to_vec();
    v.sort_by(f64::total_cmp);
    let mid = v.len() / 2;
    match v.len() {
        0 => None,
        n if n % 2 == 1 => Some(v[mid]),
        _ => Some((v[mid - 1] + v[mid]) / 2.0),
    }
}

pub(crate) fn r1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

pub(crate) fn r2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use super::*;
    use crate::{Aircraft, Assessment, Grade, RuleCount};

    const NOW: i64 = 1_800_000_000;

    fn station() -> &'static Station {
        static STATION: OnceLock<Station> = OnceLock::new();
        STATION.get_or_init(|| Station::core().unwrap())
    }

    /// A landing at `icao` scored `score` with the given zone and offset.
    fn landing(icao: &str, score: Option<f64>, zone: &str, cross_m: f64) -> LandingFacts {
        LandingFacts {
            t_s: 100.0,
            full_stop: true,
            icao: Some(icao.into()),
            runway: Some("09".into()),
            score,
            grade: score.map(|_| Grade::Good),
            confidence: None,
            domains: score
                .map(|_| [(Domain::Stability, Some(1)), (Domain::Rollout, None)].into())
                .unwrap_or_default(),
            sink_fpm: 120.0,
            peak_g: 1.2,
            along_m: Some(300.0),
            cross_m: Some(cross_m),
            zone: Some(zone.into()),
            tdz_m: Some(900.0),
            aiming_point_m: Some(400.0),
            lda_m: Some(3000.0),
            crosswind_kt: None,
            tailwind_kt: None,
            bounces: 0,
        }
    }

    /// A flight `age` days before `NOW` (none: no start time).
    fn flight(
        n: usize,
        age: Option<i64>,
        aircraft: &str,
        landings: Vec<LandingFacts>,
    ) -> FlightSummary {
        FlightSummary {
            id: format!("flight-{n:03}"),
            started_utc: age.map(|d| iso_utc((NOW - d * DAY_S) * 1_000_000, false)),
            duration_s: 3600.0,
            aircraft: Some(Aircraft {
                icao: Some(aircraft.into()),
                title: aircraft.into(),
            }),
            departure: Some("LTBJ".into()),
            arrival: landings.last().and_then(|l| l.icao.clone()),
            landings: landings.len(),
            worst_level: None,
            event_counts: BTreeMap::new(),
            assessment: Assessment::of(&[]),
            landings_detail: landings,
            skills: SkillsSummary::default(),
            rule_counts: BTreeMap::new(),
        }
    }

    fn fires(mut f: FlightSummary, rule: &str, count: usize) -> FlightSummary {
        let level = Level::Medium;
        f.rule_counts
            .insert(rule.into(), RuleCount { level, count });
        f
    }

    fn all(summaries: &[FlightSummary]) -> Trends {
        build(summaries, &TrendFilter::default(), station(), NOW)
    }

    /// `n` flights, the oldest first, scored 70, 71, ...
    fn series(n: usize) -> Vec<FlightSummary> {
        (0..n)
            .map(|i| {
                let l = landing("LTBJ", Some(70.0 + i as f64), "In TDZ", 1.0);
                flight(i, Some((n - i) as i64), "B738", vec![l])
            })
            .collect()
    }

    #[test]
    fn empty_input_is_all_zero_and_null() {
        let t = all(&[]);
        assert_eq!((t.flights, t.landing_count, t.scored), (0, 0, 0));
        assert_eq!((t.score.mean, t.score.median), (None, None));
        assert!(t.events.is_empty() && t.by_aircraft.is_empty() && t.by_airport.is_empty());
        assert_eq!(t.domains.len(), 6);
        assert!(
            t.domains
                .iter()
                .all(|d| d.graded == 0 && d.mean_level.is_none())
        );
        serde_json::to_value(&t).unwrap();
    }

    #[test]
    fn filters_pick_type_airport_and_days() {
        let mut s = series(4);
        s.push(flight(
            9,
            Some(500),
            "C172",
            vec![landing("LTFE", Some(90.0), "Long", 0.0)],
        ));
        let by = |f: TrendFilter| build(&s, &f, station(), NOW);
        assert_eq!(by(TrendFilter::default()).flights, 5);
        let t = by(TrendFilter {
            aircraft: Some("c172".into()),
            ..Default::default()
        });
        assert_eq!((t.flights, t.landing_count), (1, 1));
        // The options come from every flight, whatever the filter.
        assert_eq!(t.options.aircraft.len(), 2);
        assert_eq!(t.options.airports.len(), 2);
        let t = by(TrendFilter {
            airport: Some("LTFE".into()),
            ..Default::default()
        });
        assert_eq!((t.flights, t.landing_count), (1, 1));
        // A flight departing from the airport counts, its landings elsewhere do not (5 flights, 4 landings).
        let t = by(TrendFilter {
            airport: Some("LTBJ".into()),
            ..Default::default()
        });
        assert_eq!((t.flights, t.landing_count), (5, 4));
        let per_type: usize = t.by_aircraft.iter().map(|a| a.landings).sum();
        assert_eq!(
            per_type, 4,
            "by aircraft counts the airport's landings only"
        );
        let t = by(TrendFilter {
            days: Some(3),
            ..Default::default()
        });
        assert_eq!(t.flights, 3);
        let t = by(TrendFilter {
            days: Some(365),
            ..Default::default()
        });
        assert_eq!(t.flights, 4);
    }

    #[test]
    fn unscored_landings_count_but_stay_out_of_the_means() {
        let s = [
            flight(
                1,
                Some(2),
                "B738",
                vec![landing("LTBJ", Some(80.0), "In TDZ", 2.0)],
            ),
            flight(2, Some(1), "B738", vec![landing("LTBJ", None, "Long", 4.0)]),
        ];
        let t = all(&s);
        assert_eq!((t.landing_count, t.scored), (2, 1));
        assert_eq!((t.score.mean, t.score.median), (Some(80.0), Some(80.0)));
        let a = &t.by_airport[0];
        assert_eq!((a.landings, a.mean_score), (2, Some(80.0)));
        // The footprint takes both.
        assert_eq!((a.in_tdz, a.mean_cross_m), (Some(0.5), Some(3.0)));
        assert_eq!(t.landings.len(), 2);
        let b = &t.by_aircraft[0];
        assert_eq!((b.flights, b.landings, b.mean_score), (2, 2, Some(80.0)));
    }

    #[test]
    fn not_applicable_domains_are_not_graded() {
        let t = all(&series(3));
        let get = |id| t.domains.iter().find(|d| d.id == id).unwrap();
        let stability = get(Domain::Stability);
        assert_eq!(
            (
                stability.graded,
                stability.mean_level,
                stability.level_2_plus
            ),
            (3, Some(1.0), 0)
        );
        assert_eq!(stability.short, "Approach");
        assert_eq!(get(Domain::Rollout).graded, 0);
        assert_eq!(get(Domain::Rollout).mean_level, None);
    }

    #[test]
    fn median_and_mean_of_the_scored_landings() {
        let t = all(&series(4));
        assert_eq!((t.score.mean, t.score.median), (Some(71.5), Some(71.5)));
        assert_eq!(t.hours, 4.0);
    }

    #[test]
    fn a_half_needs_five_flights_for_a_comparison() {
        let nine = all(&series(9));
        assert_eq!((nine.score.recent, nine.score.earlier), (None, None));
        let ten = all(&series(10));
        // Scores 70..79: the older five average 72, the newer five 77.
        assert_eq!(
            (ten.score.recent, ten.score.earlier),
            (Some(77.0), Some(72.0))
        );
    }

    #[test]
    fn events_are_rates_per_100_flights_with_halves() {
        let rule = "unstable-speed-above-500";
        let s: Vec<_> = series(10)
            .into_iter()
            .enumerate()
            .map(|(i, f)| match i {
                // Two older flights fired it, one newer, the last one twice.
                0 | 1 => fires(f, rule, 1),
                9 => fires(f, rule, 2),
                _ => f,
            })
            .collect();
        let t = all(&s);
        let e = &t.events[0];
        assert_eq!(
            (e.rule.as_str(), e.level, e.flights, e.count),
            (rule, Level::Medium, 3, 4)
        );
        // Three flights of ten; the repeat in the last one counts once.
        assert_eq!(e.per_100, 30.0);
        assert_eq!(
            (e.earlier_per_100, e.recent_per_100),
            (Some(40.0), Some(20.0))
        );
        assert_ne!(e.title, rule, "the station's rule title");
        let few = all(&s[..9]);
        assert_eq!(few.events[0].recent_per_100, None);
        assert_eq!(t.by_aircraft[0].events_per_flight, 0.4);
    }

    #[test]
    fn flights_without_a_start_time_sort_first() {
        let s = [
            flight(
                1,
                Some(1),
                "B738",
                vec![landing("LTBJ", Some(90.0), "In TDZ", 0.0)],
            ),
            flight(
                2,
                None,
                "B738",
                vec![landing("LTBJ", Some(60.0), "In TDZ", 0.0)],
            ),
        ];
        let t = all(&s);
        assert_eq!(t.landings[0].flight_id, "flight-002");
        assert_eq!(t.flights, 2);
        // With a days limit its age is unknown, so it is left out.
        let t = build(
            &s,
            &TrendFilter {
                days: Some(30),
                ..Default::default()
            },
            station(),
            NOW,
        );
        assert_eq!(t.flights, 1);
    }

    #[test]
    fn the_json_is_the_documented_shape() {
        let v = serde_json::to_value(all(&series(2))).unwrap();
        assert_eq!(v["landings"][0]["flight_id"], "flight-000");
        assert_eq!(v["landings"][0]["score"], 70.0);
        assert_eq!(v["domains"][0]["id"], "stability");
    }

    #[test]
    fn skills_are_medians_with_fuel_per_type() {
        let with = |mut f: FlightSummary, ap: f64, off: Option<f64>, rms: f64, kg_h: f64| {
            f.skills = SkillsSummary {
                ap_share: Some(ap),
                ap_off_ft: off,
                vertical_rms: Some(rms),
                fuel_kg_h: Some(kg_h),
                ..Default::default()
            };
            f
        };
        let mut s = [
            with(flight(1, Some(3), "B738", vec![]), 0.5, None, 0.2, 2400.0),
            with(
                flight(2, Some(2), "B738", vec![]),
                0.7,
                Some(300.0),
                0.4,
                2600.0,
            ),
            with(flight(3, Some(1), "C172", vec![]), 0.3, None, 0.3, 30.0),
            // No skills at all (an old summary) leaves every figure out.
            flight(4, Some(1), "C172", vec![]),
        ];
        // The first landed on the autopilot: no disconnect height.
        s[0].skills.autoland = true;
        let k = all(&s).skills;
        assert_eq!(k.ap_share, Some(0.5));
        assert_eq!(
            (k.ap_off_ft_median, k.autolands, k.vertical_rms_median),
            (Some(300.0), 1, Some(0.3))
        );
        assert_eq!(k.descent_level_s_median, None);
        let fuel: Vec<_> = k
            .fuel_kg_h
            .iter()
            .map(|f| (f.icao.as_str(), f.median))
            .collect();
        assert_eq!(fuel, [("B738", 2500.0), ("C172", 30.0)]);
        assert_eq!(all(&[]).skills.ap_share, None);
        let old: FlightSummary = serde_json::from_value(
            serde_json::to_value(flight(5, None, "C172", vec![]))
                .map(|mut v| {
                    v.as_object_mut().unwrap().remove("skills");
                    v
                })
                .unwrap(),
        )
        .unwrap();
        assert_eq!(old.skills, SkillsSummary::default());
    }
}
