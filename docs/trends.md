# Trends

An airline's FOQA programme is about trends, not single flights: event rates per sector,
touchdown dispersion per airport, which landing area keeps costing points. The Trends screen
does the same for one pilot's flights.

## Where the numbers come from

Every number is aggregated from the flight list's summaries (`FlightSummary`,
`docs/debrief-api.md`), which the index caches (`crates/debrief/src/index.rs`). A summary is
valid only for the `Station` that made it (rules, profiles, scoring method, analysis code), so
after an update every flight is analysed again from its recording, as a ground station
reprocesses its archive. Trends therefore never mix rule or score versions.

`debrief::trends(&summaries, &filter, &station)` is the only aggregation; the app
(`trends` command) and the CLI (`fdm trends <dir> [--aircraft ICAO] [--airport ICAO]
[--days N] [--json]`) both call it.

## Summary additions

`FlightSummary` gains two fields (both `#[serde(default)]`, empty when absent):

- `landings_detail: [LandingFacts]`, one per landing (bounces grouped with their landing, as on
  the debrief), in time order:

  | field | type | |
  |---|---|---|
  | `t_s` | number | first touchdown |
  | `full_stop` | bool | `false` for a touch-and-go |
  | `icao`, `runway` | string? | matched runway |
  | `score` | number? | landing-2, 0-100 |
  | `grade` | `good`/`fair`/`poor`? | |
  | `confidence` | `full`/`partial`? | |
  | `domains` | `{domain id: level 0-3 or null}` | null: n/a |
  | `sink_fpm`, `peak_g` | number | first contact |
  | `along_m`, `cross_m` | number? | past the threshold, right of the centreline positive |
  | `zone` | `In TDZ`/`Long`/`Short`? | |
  | `tdz_m`, `aiming_point_m`, `lda_m` | number? | the matched runway's touchdown zone, aiming point and landing distance |
  | `crosswind_kt`, `tailwind_kt` | number? | at contact, on the runway heading; right crosswind positive, a headwind is a negative tailwind |
  | `bounces` | integer | |

- `rule_counts: {rule id: {level, count}}`: each rule that fired at low or above, its worst
  level and how often.

## `Trends`

```jsonc
{
  "filter": { "aircraft": "B738", "airport": null, "days": 90 },
  "options": {                         // what the filters can be set to, from all flights
    "aircraft": [{ "icao": "B738", "flights": 12 }],
    "airports": [{ "icao": "LTBJ", "landings": 7 }]
  },
  "flights": 12, "hours": 21.4, "landing_count": 14, "scored": 13,   // scored: full-stop landings with a score
  "score": {                           // scored full-stop landings
    "mean": 78.2, "median": 81.0,
    "recent": 82.5, "earlier": 74.0     // newest half vs older half; null under 5 in each
  },
  "landings": [                        // the rows, as `landing_count` counts them; oldest first: the chart and the footprint
    { "flight_id": "flight-…", "started_utc": "…", "aircraft": "B738",
      /* the LandingFacts fields */ }
  ],
  "domains": [                         // landing-2 order
    { "id": "stability", "title": "Approach stability", "short": "Approach",
      "graded": 13, "mean_level": 0.6, "level_2_plus": 2 }
  ],
  "events": [                          // most frequent first, then by level
    { "rule": "unstable-speed-above-500", "title": "…", "level": "medium",
      "flights": 4, "count": 6, "per_100": 33.3,
      "recent_per_100": 16.7, "earlier_per_100": 50.0 }   // null under 5 flights in each half
  ],
  "by_aircraft": [{ "icao": "B738", "flights": 12, "landings": 14, "mean_score": 78.2,
                    "events_per_flight": 1.4 }],
  "by_airport": [{ "icao": "LTBJ", "landings": 7, "mean_score": 80.1, "in_tdz": 0.71,
                   "mean_cross_m": 1.2 }]
}
```

Rules:

- A flight counts when it matches the filter: `aircraft` its type, `airport` its departure or
  arrival (a landing's runway for landing rows), `days` its start within the last N days.
- Rates are flights with the event per 100 flights, the way an airline quotes events per 1,000
  sectors: a rule that fires seven times in one flight is one flight with the event (`count`
  keeps the firings). A count alone says nothing without its denominator.
- "Recent" and "earlier" split the matching flights by start time into halves. With fewer than
  5 in either half the comparison is `null`: the screen says there is not enough data instead
  of drawing a trend from two flights.
- Unscored landings (insufficient data) count as landings and in the footprint, never in score
  means; n/a domains are left out of that domain's `graded`.
- Flights without a start time sort first and fall in "earlier"; a `days` filter leaves them
  out (their age is unknown).
- The halves split at the middle, the older one taking the smaller half of an odd count. A score
  comparison halves the scored landings, an event rate the flights.
- `mean_cross_m` is signed (the average offset, right positive); `in_tdz` is the share of the
  airport's landings with a touchdown zone. Mean scores are over full-stop landings.

`skills` (`docs/skills.md`) holds `{ap_share, ap_off_ft_median, autolands, vertical_rms_median,
descent_level_s_median, fuel_kg_h: [{icao, median}]}` over the matching flights: the flights'
autopilot shares averaged, the others medians (an autoland counts as 0 ft), fuel per type by ICAO.
A flight without a measure is left out of it; with none the value is `null`.
