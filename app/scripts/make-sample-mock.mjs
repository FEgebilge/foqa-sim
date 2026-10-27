// Writes a synthetic sample flight list and debriefs to public/mock/sample-*.json in the shape `fdm debrief --json` writes
// (docs/debrief-api.md); event texts and details come from rules/core.yaml. Only for developing the UI in a browser before
// real data exists; the API layer prefers flights.json / debrief-<id>.json.
// A C172 flies two left-hand circuits on LTFA runway 35: a touch-and-go that bounces once (two contacts, one landing), then a long full-stop landing.
import { mkdirSync, readFileSync, writeFileSync } from "node:fs"
import { join } from "node:path"

const OUT = join(import.meta.dirname, "../public/mock")
/** title, description, why_it_matters, advice and detail of each rule, by name. */
const RULES = Object.fromEntries(readFileSync(join(import.meta.dirname, "../../rules/core.yaml"), "utf8").replace(/\r/g, "").split(/^---$/m).map((doc) => {
  const field = (key) => doc.match(new RegExp(`^${key}: >\\n((?:  .*\\n?)+)`, "m"))?.[1].trim().split(/\s*\n\s*/).join(" ")
    ?? doc.match(new RegExp(`^${key}: "?(.*?)"?$`, "m"))?.[1]
  return [field("rule"), Object.fromEntries(["title", "description", "why_it_matters", "advice", "detail"].map((k) => [k, field(k)]))]
}))
const UNIT = { kt: " kt", fpm: " fpm", ft: " ft", g: " g", deg: "°" }
/** A number as the rules engine shows it (crates/rules/src/rule.rs `show`): two decimals in g, one under 10 when not whole. */
const digits = (x, unit) => (unit === "g" ? 2 : Math.abs(x) < 10 && Math.round(x * 10) % 10 ? 1 : 0)
const show = (x, unit, extra = 0) => (x + 0).toFixed(digits(x, unit) + extra) + UNIT[unit]
/** The rule's `detail` with {value}, {threshold} and the named [value, unit] pairs filled in; a value that would read as its threshold gets more decimals. */
function detail(rule, value, threshold, unit, vars = {}) {
  const template = RULES[rule].detail ?? (value === undefined ? undefined : "{value}, limit {threshold}")
  const extra = value === undefined ? 0 : [0, 1].find((e) => show(value, unit, e) !== show(threshold, unit)) ?? 2
  return template?.replace(/\{([^}]+)\}/g, (_, name) => (name === "value" ? show(value, unit, extra) : name === "threshold" ? show(threshold, unit) : show(...vars[name])))
}
const KT = 0.514444, FT = 0.3048, G = 9.80665, DEG = Math.PI / 180
/** Wind at contact on the runway as the debrief states it: headwind (tail negative), crosswind (from the right positive). */
const WIND_KT = 14, WIND_FROM = 15
const runwayWind = (speed_kt, from_deg) => {
  const a = (from_deg - 350) * Math.PI / 180
  return { from_deg, speed_kt, headwind_kt: Math.round(speed_kt * Math.cos(a) * 10) / 10, crosswind_kt: Math.round(speed_kt * Math.sin(a) * 10) / 10 }
}
const THR35 = { lat: 38.4985, lon: 27.0128 }, HDG = 350, LDA = 2950, ELEV_FT = 15
const TDZ = Math.min(3000 * FT, LDA / 3)

/** Local runway frame (along the 35 centreline, right of it) to lat/lon. */
function toLatLon(along, right) {
  const h = HDG * DEG
  const east = along * Math.sin(h) + right * Math.cos(h), north = along * Math.cos(h) - right * Math.sin(h)
  return { lat: THR35.lat + north / 111320, lon: THR35.lon + east / (111320 * Math.cos(THR35.lat * DEG)) }
}

/** A polyline through the nodes with corners rounded to `radius`, sampled every metre or so; returns points and node distances. */
function path(nodes, radius) {
  const points = [], nodeS = []
  let s = 0, last = null
  const push = (p) => { if (last) s += Math.hypot(p[0] - last[0], p[1] - last[1]); points.push({ a: p[0], r: p[1], s }); last = p }
  push(nodes[0]); nodeS.push(0)
  for (let i = 1; i < nodes.length; i++) {
    const [A, B, C] = [nodes[i - 1], nodes[i], nodes[i + 1]]
    const d1 = norm([B[0] - A[0], B[1] - A[1]])
    const d2 = C ? norm([C[0] - B[0], C[1] - B[1]]) : d1
    const turn = Math.acos(Math.max(-1, Math.min(1, d1[0] * d2[0] + d1[1] * d2[1])))
    const t = turn > 1e-6 ? radius * Math.tan(turn / 2) : 0
    const start = [B[0] - d1[0] * t, B[1] - d1[1] * t]
    straight(last, start, push)
    if (t > 0) {
      const end = [B[0] + d2[0] * t, B[1] + d2[1] * t]
      for (let k = 1; k <= 40; k++) { const u = k / 40; push(bezier(start, B, end, u)) }
    }
    nodeS.push(s)
  }
  return { points, nodeS }
}
const norm = (v) => { const l = Math.hypot(v[0], v[1]); return [v[0] / l, v[1] / l] }
const bezier = (p0, p1, p2, u) => [0, 1].map((k) => (1 - u) ** 2 * p0[k] + 2 * (1 - u) * u * p1[k] + u * u * p2[k])
function straight(from, to, push) {
  const n = Math.max(1, Math.ceil(Math.hypot(to[0] - from[0], to[1] - from[1]) / 5))
  for (let k = 1; k <= n; k++) push([from[0] + (to[0] - from[0]) * k / n, from[1] + (to[1] - from[1]) * k / n])
}
const lerpKeys = (keys, s, field) => {
  if (s <= keys[0].s) return keys[0][field]
  for (let i = 1; i < keys.length; i++) if (s <= keys[i].s) {
    const u = (s - keys[i - 1].s) / (keys[i].s - keys[i - 1].s)
    return keys[i - 1][field] + (keys[i][field] - keys[i - 1][field]) * u
  }
  return keys.at(-1)[field]
}

function generate({ bounce = true, overspeed = true, secondAlong = 1150, radius = 600 } = {}) {
  const circuit = [[2600, 0], [2600, -1500], [-1200, -1500], [-1200, 0], [0, 0]]
  const { points, nodeS: N } = path([[-300, 0], [0, 0], ...circuit, ...circuit, [2000, 0], [2000, 250]], radius)
  // Node indexes: 1 threshold, 2-6 circuit one (6 = threshold), 7-11 circuit two (11 = threshold), 12 runway exit, 13 end.
  const td1 = N[6] + 250, td2 = N[11] + secondAlong
  const keys = [
    { s: 0, gs: 0, h: 0, flap: 10 }, { s: 20, gs: 9, h: 0, flap: 10 }, { s: N[1] - 30, gs: 9, h: 0, flap: 10 }, { s: N[1], gs: 4, h: 0, flap: 10 },
    { s: N[1] + 380, gs: 55, h: 0, flap: 10 },
  ]
  const lap = (s0, c) => [
    { s: s0 + 1500, gs: 72, h: 120, flap: 10 }, { s: s0 + 1800, gs: 75, h: 150, flap: 0 }, { s: N[c + 1] + 900, gs: 95, h: 305, flap: 0 },
    { s: N[c + 1] + 2500, gs: 92, h: 305, flap: 0 }, { s: N[c + 1] + 2700, gs: 84, h: 290, flap: 10 }, { s: N[c + 2] - 200, gs: 80, h: 220, flap: 20 },
    { s: N[c + 2] + 300, gs: overspeed && c === 2 ? 89 : 78, h: 180, flap: 30 }, { s: N[c + 3], gs: 70, h: 120, flap: 30 },
  ]
  keys.push(...lap(N[1] + 380, 2))
  keys.push({ s: td1 - 150, gs: 64, h: 8, flap: 30 }, { s: td1, gs: 57, h: 0, flap: 30 })
  if (bounce) keys.push({ s: td1 + 40, gs: 56, h: 1.1, flap: 30 }, { s: td1 + 85, gs: 55, h: 0, flap: 30 })
  const lo2 = td1 + 520
  keys.push({ s: td1 + 300, gs: 45, h: 0, flap: 10 }, { s: lo2, gs: 55, h: 0, flap: 10 }, ...lap(lo2, 7))
  keys.push({ s: td2 - 220, gs: 63, h: 4, flap: 30 }, { s: td2, gs: 56, h: 0, flap: 30 }, { s: td2 + 450, gs: 18, h: 0, flap: 30 },
    { s: N[12], gs: 9, h: 0, flap: 0 }, { s: N[13] - 10, gs: 6, h: 0, flap: 0 }, { s: N[13], gs: 0, h: 0, flap: 0 })

  const dt = 0.125, STAND = 40, DWELL = 30
  const raw = []
  let s = 0, t = 0, endT = null, pi = 0
  while (true) {
    while (pi < points.length - 2 && points[pi + 1].s < s) pi++
    const p0 = points[pi], p1 = points[pi + 1], u = Math.min(1, Math.max(0, (s - p0.s) / (p1.s - p0.s || 1)))
    const a = p0.a + (p1.a - p0.a) * u, r = p0.r + (p1.r - p0.r) * u
    const heading = Math.atan2(p1.r - p0.r, p1.a - p0.a)
    const gsKt = t < STAND || endT !== null ? 0 : Math.max(2, lerpKeys(keys, s, "gs"))
    raw.push({ t, s, a, r, heading, gsKt, h: lerpKeys(keys, s, "h"), flap: Math.round(lerpKeys(keys, s, "flap") / 10) * 10 })
    if (endT === null && s >= N[13] - 0.5) endT = t
    if (endT !== null && t >= endT + DWELL) break
    s += gsKt * KT * dt
    t += dt
  }
  // Derived parameters from neighbours: vertical speed, turn rate -> bank.
  for (let i = 0; i < raw.length; i++) {
    const p = raw[Math.max(0, i - 4)], n = raw[Math.min(raw.length - 1, i + 4)], x = raw[i]
    const span = n.t - p.t || 1
    x.vs = (n.h - p.h) / span / FT * 60
    let dh = n.heading - p.heading; dh = Math.atan2(Math.sin(dh), Math.cos(dh))
    const airborne = x.h > 0.05
    x.roll = airborne ? Math.atan((dh / span) * x.gsKt * KT / G) / DEG : 0
    x.ias = x.gsKt > 25 ? x.gsKt + 3 : 0
    x.pitch = x.h <= 0.05 ? -1 : x.h < 10 && x.vs < 0 ? 5 : 2 + Math.max(-6, Math.min(8, x.vs / 120))
  }
  // Touchdowns: first ground contact after being airborne.
  const tds = []
  for (let i = 1; i < raw.length; i++) if (raw[i - 1].h > 0.05 && raw[i].h <= 0.05) tds.push(i)
  const mains = [tds[0], tds.at(-1)]
  const peaks = [bounce ? 1.94 : 1.35, 1.28]
  for (const x of raw) {
    x.nz = 1 / Math.cos(x.roll * DEG) + 0.01 * Math.sin(x.t * 7.3)
    tds.forEach((i) => { const k = (x.t - raw[i].t) / 0.15; x.nz += (i === mains[1] ? peaks[1] - 1 : peaks[0] - 1) * Math.exp(-k * k) * (i === tds[1] && bounce && tds.length > 2 ? 0.4 : 1) })
  }
  return { raw, tds, mains, peaks, keys, N, td1, td2 }
}

/** The phase at sample `x`, the flare from `flareFt` above the runway. */
function phaseOf(x, ctx, flareFt) {
  if (x.t < 40) return "Standing"
  if (x.s < ctx.N[1]) return "Taxi"
  if (x.gsKt === 0) return "Standing"
  if (x.h <= 0.05 && x.s < ctx.td1 && x.gsKt < 30) return "Taxi"
  if (x.s > ctx.td2 && x.gsKt < 30) return "Taxi"
  if (x.h <= 0.05) return x.s > ctx.td1 + 280 && x.s < ctx.td2 - 300 ? "Takeoff" : x.s >= ctx.td1 - 200 ? "Landing" : "Takeoff"
  if (x.s > ctx.td1 - 200 && x.s < ctx.td1 + 280 && (x.s > ctx.td1 || x.h < flareFt * FT)) return "Landing"
  if (x.h < 35 * FT && x.vs > 0) return "Takeoff"
  if (x.h < flareFt * FT && x.vs <= 0) return "Landing"
  if (x.vs > 150 && x.h < 1000 * FT - 1) return "InitialClimb"
  if (x.vs < -150 || x.h < 1000 * FT - 3) return x.vs > 150 ? "InitialClimb" : "Approach"
  return "EnRoute"
}

/** The phase boundaries in effect (`phase_values`): none of these profiles moves its class's defaults (docs/phases.md). */
function phaseValues(cls) {
  const values = { approach_ft: 1000, climb_end_ft: 1000, flare_ft: cls === "light" ? 20 : 50, go_around_drop_ft: 50, go_around_rise_ft: 200, roll_kt: 30 }
  return Object.fromEntries(Object.entries(values).map(([k, value]) => [k, { value, source: `${cls} class default` }]))
}

/** What opened a phase at sample `x`, worded as crates/debrief/src/lib.rs `phase_trigger`: a value that would read as its threshold gets a decimal. */
function trigger(phase, prev, x, values) {
  const n = (v, th) => (v.toFixed(0) === String(th) ? v.toFixed(1) : v.toFixed(0))
  const ft = x.h / FT, kt = x.gsKt, flare = values.flare_ft.value
  return {
    Standing: `stopped, under 2 kt on the ground (${n(kt, 2)} kt)`,
    Taxi: `moving on the ground under 30 kt (${n(kt, 30)} kt)`,
    Takeoff: prev === "Landing" ? `lowest speed of the touch-and-go roll, takeoff power applied (${n(kt)} kt)` : `on the ground above 30 kt (${n(kt, 30)} kt)`,
    InitialClimb: `35 ft above the runway (${n(ft, 35)} ft RA)`,
    EnRoute: `climbing through 1000 ft above the aerodrome (${n(ft, 1000)} ft AAL)`,
    Approach: `descending through 1000 ft above the aerodrome (${n(ft, 1000)} ft AAL)`,
    Landing: `flare, below ${flare} ft above the runway (${n(ft, flare)} ft RA)`,
  }[phase]
}

function build(id, opts = {}) {
  const ctx = generate(opts)
  const { raw, tds, mains, peaks } = ctx
  const near = (t) => mains.some((i) => Math.abs(t - raw[i].t) <= 10)
  const keep = raw.filter((x) => Math.abs(x.t * 2 - Math.round(x.t * 2)) < 1e-6 || near(x.t))
  const cut = opts.damagedAt ? keep.findIndex((x) => x.t > opts.cutAt) : -1
  const samples = cut > 0 ? keep.slice(0, cut) : keep
  const r1 = (v, d = 1) => Math.round(v * 10 ** d) / 10 ** d
  // profiles/C172.yaml flap bands; other types here have none.
  const vfe = (flap) => (opts.aircraft?.icao === "C172" && flap > 0 ? (flap <= 10 ? 110 : 85) : null)
  const pos = samples.map((x) => toLatLon(x.a, x.r))
  const series = {
    t_s: samples.map((x) => r1(x.t, 3)),
    alt_ft: samples.map((x) => r1(ELEV_FT + x.h / FT)),
    aal_ft: samples.map((x) => r1(x.h / FT)),
    ias_kt: samples.map((x) => r1(x.ias)),
    gs_kt: samples.map((x) => r1(x.gsKt)),
    vs_fpm: samples.map((x) => Math.round(x.vs)),
    nz_g: samples.map((x) => r1(x.nz, 3)),
    pitch_deg: samples.map((x) => r1(x.pitch)),
    roll_deg: samples.map((x) => r1(x.roll)),
    flap_deg: samples.map((x) => x.flap),
    vfe_kt: samples.map((x) => vfe(x.flap)),
    lat: pos.map((p) => r1(p.lat, 6)),
    lon: pos.map((p) => r1(p.lon, 6)),
  }
  if (opts.noPosition) series.lat = series.lat.map((v, i) => (i % 400 < 10 ? null : v))

  const phase_values = phaseValues(opts.class)
  const phases = []
  for (const x of samples) {
    const phase = phaseOf(x, ctx, phase_values.flare_ft.value)
    const last = phases.at(-1)
    if (last?.phase === phase) last.end_s = r1(x.t, 3)
    else { if (last) last.end_s = r1(x.t, 3); phases.push({ phase, start_s: r1(x.t, 3), end_s: r1(x.t, 3), trigger: trigger(phase, last?.phase, x, phase_values) }) }
  }

  const lastT = samples.at(-1).t
  const withRunway = !opts.noRunway
  // Every ground contact; with a bounce, the one between the two mains is the first landing's second contact.
  const touchdowns = tds.filter((i) => raw[i].t <= lastT).map((i) => {
    const x = raw[i], before = raw[i - 5], isBounce = !mains.includes(i), k = i === mains[0] ? 0 : 1
    const sink = Math.round(-before.vs)
    return {
      t_s: r1(x.t, 3), bounce: isBounce, sink_fpm: sink, sim_sink_fpm: Math.round(sink * 0.93),
      peak_g: isBounce ? 1.45 : peaks[k], pitch_deg: r1(before.pitch), roll_deg: r1(before.roll), ias_kt: r1(x.ias),
      wind: withRunway ? runwayWind(WIND_KT, WIND_FROM + k * 10) : undefined,
      runway: withRunway ? (() => {
        const cross = k === 0 ? -2.8 : opts.secondCross ?? 9.5
        return { icao: "LTFA", runway: "35", along_m: Math.round(x.a), cross_m: cross, lda_m: LDA, tdz_m: Math.round(TDZ), aiming_point_m: 400, in_tdz: x.a >= 0 && x.a <= TDZ, long_landing: x.a > TDZ, short_landing: x.a < 0, zone: x.a < 0 ? "Short" : x.a > TDZ ? "Long" : "In TDZ" }
      })() : undefined,
    }
  })

  const events = []
  // Thresholds as in rules/core.yaml; `vars` fills the detail's other placeholders.
  const add = ({ rule, level, start, end = start, peak = start, measure, value, levels, unit, vars }) => {
    level ??= ["high", "medium", "low"].find((l) => value > levels[l])
    const threshold = levels?.[level], { title, description, why_it_matters, advice } = RULES[rule]
    // JSON key order follows `fdm debrief --json`.
    events.push({ rule, version: 1, title, description, level, start_s: r1(start, 3), end_s: r1(end, 3), peak_s: r1(peak, 3), measure,
      value, threshold, unit, why_it_matters, advice, detail: detail(rule, value, threshold, unit, vars) })
  }
  const span = (pred) => { const out = []; let cur = null; for (const x of samples) { if (pred(x)) { cur ??= { start: x.t, xs: [] }; cur.end = x.t; cur.xs.push(x) } else if (cur) { out.push(cur); cur = null } } if (cur) out.push(cur); return out }
  const peakOf = (xs, f) => xs.reduce((a, b) => (f(b) > f(a) ? b : a))
  for (const b of span((x) => x.h < 500 * FT && x.h > 0.05 && Math.abs(x.roll) > 15)) {
    if (b.end - b.start < 2) continue
    const p = peakOf(b.xs, (x) => Math.abs(x.roll))
    add({ rule: "bank-below-500", start: b.start, end: b.end, peak: p.t, measure: "abs(roll)", value: r1(Math.abs(p.roll)), levels: { low: 15, medium: 20, high: 30 }, unit: "deg" })
  }
  for (const b of span((x) => vfe(x.flap) !== null && x.ias > vfe(x.flap) && x.h > 0.05)) {
    if (b.end - b.start < 2) continue
    const p = peakOf(b.xs, (x) => x.ias - vfe(x.flap))
    add({ rule: "flap-overspeed", start: b.start, end: b.end, peak: p.t, measure: "ias - profile.vfe", value: r1(p.ias - vfe(p.flap)), levels: { low: 0, medium: 5, high: 10 }, unit: "kt",
      vars: { ias: [p.ias, "kt"], "flap.te_angle": [p.flap, "deg"], "profile.vfe": [vfe(p.flap), "kt"] } })
  }
  for (const td of touchdowns) {
    const at = { start: td.t_s }
    if (td.bounce) { add({ rule: "bounced-landing", ...at, level: "medium", measure: "td.bounce" }); continue }
    if (td.peak_g > 1.8) add({ rule: "hard-landing-light", ...at, measure: "td.peak_g", value: td.peak_g, levels: { low: 1.8, medium: 2.2, high: 2.6 }, unit: "g" })
    if (td.runway?.long_landing) add({ rule: "long-landing", ...at, level: "medium", measure: "td.long_landing" })
    const crossFt = td.runway ? r1(Math.abs(td.runway.cross_m) / FT) : 0
    if (crossFt > 25) add({ rule: "off-centreline-touchdown", ...at, measure: "abs(td.cross)", value: crossFt, levels: { low: 25, medium: 40, high: 60 }, unit: "ft" })
  }
  const rank = { high: 0, medium: 1, low: 2, advisory: 3 }
  const ordered = events.sort((a, b) => a.start_s - b.start_s || rank[a.level] - rank[b.level])

  const counts = { advisory: 0, low: 0, medium: 0, high: 0 }
  ordered.forEach((e) => counts[e.level]++)
  const worst = ["high", "medium", "low", "advisory"].find((l) => counts[l] > 0)
  // Each rule counts once, at its worst level: high 4 points, medium 2, low 1, advisory 0 (crates/debrief/src/wire.rs).
  const POINTS = { high: 4, medium: 2, low: 1, advisory: 0 }
  const worstOf = {}
  for (const e of ordered) {
    const part = worstOf[e.rule] ??= { rule: e.rule, title: e.title, level: e.level, points: 0, count: 0 }
    if (POINTS[e.level] > POINTS[part.level]) part.level = e.level
    part.count++
    part.points = POINTS[part.level]
  }
  // As Assessment::parts: by rule name, then most points first (stable); the bands are Grade::BANDS.
  const parts = Object.values(worstOf).sort((a, b) => (a.rule < b.rule ? -1 : a.rule > b.rule ? 1 : 0)).sort((a, b) => b.points - a.points)
  const points = parts.reduce((a, p) => a + p.points, 0)
  const legacy = { grade: points <= 1 ? "good" : points <= 5 ? "fair" : "poor", points, findings: parts.filter((p) => p.points > 0).length }
  const scored = landing2({ touchdowns, events: ordered, samples, phases, lastT, cls: opts.class, legacy })
  const assessment = { ...legacy, method: scored.method, score: scored.score ?? undefined, confidence: scored.confidence ?? undefined }
  if (scored.method === "landing-2") assessment.grade = scored.grade
  const assessment_detail = { ...scored, legacy_points: points, legacy_findings: legacy.findings, parts, bands: [{ grade: "good", up_to: 1 }, { grade: "fair", up_to: 5 }, { grade: "poor" }], grades: { good: 85, fair: 60 } }
  const summary = {
    id, started_utc: opts.started, duration_s: r1(lastT), aircraft: opts.aircraft,
    departure: "LTFA", arrival: withRunway && touchdowns.length ? "LTFA" : undefined,
    landings: touchdowns.filter((td) => !td.bounce).length, worst_level: worst, event_counts: counts, assessment,
  }
  const thr17 = toLatLon(LDA, 0)
  const debrief = {
    flight: { ...summary, lfl_version: 1, profile: opts.profile, class: opts.class, phase_values, assessment_detail, damaged_at: opts.damagedAt },
    phases, touchdowns, events: ordered, series,
    airports: withRunway ? [{ icao: "LTFA", runways: [{ width_m: 45, ends: [
      { runway: "35", lat: THR35.lat, lon: THR35.lon, heading_deg: HDG, lda_m: LDA, tdz_m: Math.round(TDZ) },
      { runway: "17", lat: r1(thr17.lat, 6), lon: r1(thr17.lon, 6), heading_deg: 170, lda_m: LDA, tdz_m: Math.round(TDZ) },
    ] }] }] : [],
  }
  return { summary, debrief }
}

// landing-2 (docs/scoring.md) for the light class, approximated in JS for these synthetic flights only: the scalar criteria
// at touchdown, the approach criteria as their worst sample in the window (no sustain), the findings by their rule's domain,
// the material and undershoot caps and Decision 3's flight grade. Real debriefs come from crates/assess.
const SCORING = readFileSync(join(import.meta.dirname, "../../scoring/landing-2.yaml"), "utf8").replace(/\r/g, "")
const CRITERIA = Object.fromEntries([...SCORING.matchAll(/- id: (\w+)\n\s+title: (.*)\n(?:.*\n)*?\s+source: "(.*)"/g)].map(([, id, title, source]) => [id, { title, source }]))
const RULE_DOMAIN = Object.fromEntries(readFileSync(join(import.meta.dirname, "../../rules/core.yaml"), "utf8").replace(/\r/g, "").split(/^---$/m)
  .map((doc) => [doc.match(/^rule: (.*)$/m)?.[1], doc.match(/^domain: (\w+)$/m)?.[1]]))
// Titles, short names and summaries as crates/debrief/src/assessment.rs words them.
const DOMAINS = [
  ["stability", "Approach stability", 25, "Approach", "Speed, descent rate, bank, pitch, glideslope and configuration on final"],
  ["energy", "Threshold and flare", 10, "Flare", "Height over the threshold, touchdown speed and float down the runway"],
  ["point", "Touchdown point", 20, "Landing spot", "How far from the aiming point the wheels touched down"],
  ["lateral", "Lateral control", 15, "Centreline", "Offset from the centreline, crab and drift at touchdown, then on the rollout"],
  ["contact", "Touchdown contact", 20, "Touchdown", "Load, sink rate, bounce, tail clearance and bank at touchdown"],
  ["rollout", "Rollout and stopping", 10, "Rollout", "How soon braking began and how hard it had to be to stop"],
]
const DOMAIN_OF = { A: "stability", B: "energy", C: "point", D: "lateral", E: "contact", F: "rollout" }
const LOSS = [[0, 0], [0.5, 0.1], [1, 0.35], [2, 0.65], [3, 1]]
const curveAt = (keys, x) => { if (x <= keys[0][0]) return keys[0][1]; for (let i = 1; i < keys.length; i++) if (x <= keys[i][0]) { const [a, b] = [keys[i - 1], keys[i]]; return a[1] + ((x - a[0]) / (b[0] - a[0])) * (b[1] - a[1]) } return keys.at(-1)[1] }
const sideEll = (anchors, x, sign) => {
  const keys = []
  anchors.forEach((a, k) => { if (a === null) return; const ell = [0, 0.5, 1, 2, 3][k]; if (!keys.length && ell > 0) keys.push([sign * a, 0]); keys.push([sign * a, ell]) })
  return keys.length ? curveAt(keys, sign * x) : 0
}
const NONE = [null, null, null, null, null]
const round1 = (x) => Math.round(x * 10) / 10
/** The approach window's split height by class (`windows`). */
const SPLIT_FT = Object.fromEntries([...SCORING.matchAll(/^\s+(light|turboprop|jet): \{[^}]*split: (\d+) ft/gm)].map(([, cls, ft]) => [cls, Number(ft)]))
/** A criterion's value as the debrief words it (crates/debrief/src/assessment.rs `value_text`). */
const valueText = (id, unit, x) => {
  if (!unit) return x ? `L${x.toLocaleString("en-US")}` : "none"
  const digits = unit === "g" ? 2 : unit === "dot" || (Math.abs(x) < 10 && Math.round(x) !== x) ? 1 : 0
  const sign = ["A1", "B1", "B2", "C1"].includes(id) && x > 0 ? "+" : ""
  return `${sign}${x.toLocaleString("en-US", { minimumFractionDigits: digits, maximumFractionDigits: digits })}${unit === "deg" ? "°" : ` ${unit}`}`
}

/** What a cap at `score` allows, in words (`Method::ceiling`). */
const ceiling = (score) => (score <= 0 ? "at zero" : score >= 85 ? `at ${score.toFixed(1)} at most` : score >= 60 ? `below Good (max ${score.toFixed(1)})` : `below Fair (max ${score.toFixed(1)})`)
/** A graded item in words with its level: "Descent rate 1,565 fpm above 300 ft (level 2)". */
const itemText = (c) => `${[c.title, c.unit ? c.value_text : null, c.height_text].filter(Boolean).join(" ")} (level ${c.level})`
const CAUSE = new WeakMap()

function landing2({ touchdowns, events, samples, phases, lastT, cls, legacy }) {
  const VREF = 60, VAPP = 65, TRACK_FT = 8.2, M = 1 / FT
  const firsts = touchdowns.filter((td) => !td.bounce)
  const counted = new Set()
  const landings = firsts.map((td, n) => {
    const next = firsts[n + 1]
    if (!td.runway) return { t_s: td.t_s, status: "insufficient", score: null, raw: null, grade: null, confidence: null, class: null, partial: [], missing: ["runway"], note: "Not scored: runway data not recorded, and a score needs it. Missing data never earns credit, so this landing counts through its findings only (event points).", facts: [], full_stop: !next, caps: [], stable_from_ft: null, split_ft: null, speeds: null, domains: [] }
    const fix = td.runway, full = !next
    const approach = phases.filter((p) => p.phase === "Approach" && p.start_s <= td.t_s).at(-1)
    const from = approach?.start_s ?? td.t_s - 120, to = next ? Math.min(next.t_s, td.t_s + 30) : lastT
    const win = samples.filter((x) => x.t < td.t_s && x.h / FT <= 500 && x.h / FT >= 20 && x.t > td.t_s - 200)
    const worst = (xs, f) => xs.length ? xs.reduce((a, b) => (Math.abs(f(b)) > Math.abs(f(a)) ? b : a)) : null
    const target = fix.aiming_point_m * M, tdz = fix.tdz_m * M, lda = fix.lda_m * M
    const crit = [], split = SPLIT_FT[cls === "transport" ? "jet" : "light"]
    const add = (id, height, value, above, below = NONE) => {
      if (value === null || value === undefined) return
      const ell = Math.max(sideEll(above, value, 1), sideEll(below, value, -1))
      const unit = { A1: "kt", A2: "fpm", A3: "deg", B1: "ft", B2: "kt", C1: "ft", D1: "ft", E1: "g", E2: "fpm", E5: "deg", F2: "g" }[id]
      const text = (x) => (x === null ? null : valueText(id, unit, x))
      value = round1(value * 100) / 100
      crit.push({ id, title: CRITERIA[id].title, height, value, value_text: id === "C1" && value && text(value) !== "0.0 ft" ? `${text(Math.abs(value)).replace(/^\+/, "")} ${value < 0 ? "before" : "past"} the target` : text(value),
        height_text: height === "all" ? null : height === "above" ? `above ${split} ft` : `below ${split} ft`, unit,
        ell, level: Math.min(3, Math.floor(ell)), loss: curveAt(LOSS, ell), bands: { above, below }, bands_text: { above: above.map(text), below: below.map(text) }, standard: CRITERIA[id].source, sets: false })
    }
    for (const [height, xs] of [["above", win.filter((x) => x.h / FT >= 300)], ["below", win.filter((x) => x.h / FT < 300)]]) {
      const s = worst(xs, (x) => x.ias - VAPP); if (s) add("A1", height, s.ias - VAPP, [5, 10, 15, 20, 30], [-2.5, -5, -7.5, -10, -15])
      const v = worst(xs, (x) => -x.vs); if (v) add("A2", height, -v.vs, [Math.round(5.31 * VAPP + 200), 1000, 1200, 1500, 2000])
      const r = worst(xs, (x) => x.roll); if (r && height === "below") add("A3", height, Math.abs(r.roll), [5, 10, 15, 20, 30])
    }
    add("B2", "all", td.ias_kt - VAPP, [-10, -5, 0, 10, 20])
    add("C1", "all", (fix.along_m - fix.aiming_point_m) * M, [200, 400, Math.min(800, tdz - target), tdz - target, lda / 2 - target], [0, -100, -200, 100 - target, -target])
    add("D1", "all", Math.abs(fix.cross_m) * M, [TRACK_FT / 4, TRACK_FT / 2, 25, 40, 60])
    add("E1", "all", td.peak_g, [1.3, 1.6, 1.8, 2.2, 2.6])
    add("E2", "all", td.sink_fpm, [240, 300, 400, 500, 700])
    add("E5", "all", Math.abs(td.roll_deg ?? 0), [3, 5, 7, 10, 15])
    if (full) add("F2", "all", (td.ias_kt * KT) ** 2 / (2 * (fix.lda_m - fix.along_m)) / G, [0.1, 0.15, 0.2, 0.3, 0.4])
    const findings = events.flatMap((e, i) => {
      const domain = RULE_DOMAIN[e.rule]
      if (!domain || domain === "other" || e.start_s < from || e.start_s > to) return []
      counted.add(i)
      const ell = { advisory: 0, low: 1, medium: 2, high: 3 }[e.level]
      return [{ event: i, rule: e.rule, level: e.level, ell, loss: curveAt(LOSS, ell), sets: false, domain }]
    })
    const domains = DOMAINS.map(([id, title, weight, short, summary]) => {
      const criteria = crit.filter((c) => DOMAIN_OF[c.id[0]] === id), fs = findings.filter((f) => f.domain === id).map((f) => Object.fromEntries(Object.entries(f).filter(([k]) => k !== "domain")))
      const items = [...criteria.map((c) => [c.loss, c]), ...fs.map((f) => [f.loss, f])]
      const top = items.reduce((a, b) => (b[0] > a[0] ? b : a), [0, null])
      if (top[1] && top[0] > 0) top[1].sets = true
      const sets_by = top[1] && top[0] > 0 ? (top[1].id ? (top[1].height === "all" ? top[1].id : `${top[1].id} ${top[1].height}`) : top[1].rule) : null
      const ell = top[1]?.ell ?? 0
      const set = sets_by && top[1]
      const set_by_text = !set ? null : set.id ? [set.title, set.value_text, set.height_text].filter(Boolean).join(" ") : events[set.event]?.title ?? set.rule
      return { id, title, short, summary, weight, loss: items.length ? top[0] : null, points: 0, level: Math.min(3, Math.floor(ell)), criteria, findings: fs, sets_by, set_by_text }
    })
    const available = domains.filter((d) => d.loss !== null).reduce((a, d) => a + d.weight, 0)
    for (const d of domains) d.points = d.loss === null ? 0 : round1((d.weight * d.loss * 100) / available)
    const raw = round1(100 - domains.reduce((a, d) => a + (d.loss === null ? 0 : d.weight * d.loss), 0) * 100 / available)
    const caps = []
    const material = [...crit.filter((c) => c.ell >= 2).map(itemText), ...findings.filter((f) => f.ell >= 2).map((f) => `${events[f.event]?.title ?? f.rule} (level ${f.level === "high" ? 3 : 2})`)]
    if (material.length) {
      const cause = material.length > 1 ? `${material.slice(0, -1).join(", ")} and ${material.at(-1)}` : material[0]
      const cap = { kind: "material", score: 84.9, title: "A significant deviation limits the grade", reason: `${cause} ${material.length > 1 ? "hold" : "holds"} this landing ${ceiling(84.9)}.`, source: "A material FDM event (level 2 or more); docs/scoring.md 4" }
      CAUSE.set(cap, cause); caps.push(cap)
    }
    if (fix.short_landing) {
      const cause = "Touched down before the runway threshold"
      const cap = { kind: "undershoot", score: 39, title: "Touchdown short of the runway", reason: `${cause}, which holds this landing ${ceiling(39)}.`, source: "short-landing: touchdown before the threshold" }
      CAUSE.set(cap, cause); caps.push(cap)
    }
    const score = round1(Math.min(raw, ...caps.map((c) => c.score)))
    const stableFrom = crit.some((c) => c.id[0] === "A" && c.ell > 0.5) ? null : 500
    return { t_s: td.t_s, status: "scored", score, raw, grade: score >= 85 ? "good" : score >= 60 ? "fair" : "poor", confidence: "full", class: cls === "transport" ? "jet" : "light", partial: [], missing: [], note: null, full_stop: full, caps, stable_from_ft: stableFrom, split_ft: split,
      facts: [`Graded with ${cls === "transport" ? "jet" : "light-aircraft"} limits`, `Reference speed ${VREF} kt, approach speed ${VAPP} kt`, stableFrom === null ? "Approach never fully stable" : `Approach stable from ${stableFrom} ft down`],
      speeds: { vref_kt: VREF, vapp_kt: VAPP, source: "profile", approximate: false, basis: "Cessna 172S POH, Section 4 (sample)" }, domains }
  })
  const other_findings = events.map((_, i) => i).filter((i) => !counted.has(i))
  const scoredAt = landings.map((l, i) => [l, i]).filter(([l]) => l.status === "scored")
  if (!scoredAt.length) return { method: "points", score: null, grade: legacy.grade, confidence: null, caps: [], graded_by: null, landings, other_findings }
  const [grading, graded_by] = scoredAt.findLast(([l]) => l.full_stop) ?? scoredAt.at(-1)
  let score = grading.score
  const caps = grading.caps.map((c) => ({ ...c, landing: graded_by })), reasons = []
  for (const [l, i] of scoredAt) if (i !== graded_by) for (const c of l.caps) if (c.kind !== "material" && c.score < score) { score = c.score; reasons.push(`Landing ${i + 1}: ${CAUSE.get(c)}, which holds the flight ${ceiling(c.score)}.`) }
  const highs = other_findings.filter((i) => events[i].level === "high").length
  const limit = highs === 1 ? 84.9 : highs > 1 ? 59.9 : 100
  if (limit < score) { score = limit; reasons.push(`${highs === 1 ? "A high-severity finding outside the landings holds" : `${highs} high-severity findings outside the landings hold`} the flight ${ceiling(limit)}.`) }
  caps.push(...reasons.map((reason) => ({ kind: "flight", score, title: "The flight grade is held below the landing's score", reason, source: "docs/scoring.md Decision 3: flight grade" })))
  return { method: "landing-2", score, grade: score >= 85 ? "good" : score >= 60 ? "fair" : "poor", confidence: "full", caps, partial_note: null, graded_by, landings, other_findings }
}

const c172 = { aircraft: { icao: "C172", title: "Cessna Skyhawk G1000 Asobo" }, profile: "C172 profile", class: "light" }

const flights = [
  build("flight-1793027213", { started: "2026-10-26T14:26:53Z", ...c172 }),
  build("flight-1792941000", { started: "2026-10-25T14:30:00Z", ...c172, bounce: false, overspeed: false, secondAlong: 420, secondCross: 1.2, radius: 1100 }),
  build("flight-1792850400", { started: "2026-10-24T13:20:00Z", aircraft: { icao: "B738", title: "Boeing 737-800 Asobo" }, profile: "B738 profile", class: "transport", noRunway: true, bounce: false }),
  build("flight-1792764000", { started: "2026-10-23T13:20:00Z", ...c172, damagedAt: 1048576, cutAt: 520 }),
  build("flight-1792677600", { started: "2026-10-22T13:20:00Z", profile: "light class default (no aircraft identity)", class: "light", overspeed: false }),
]
mkdirSync(OUT, { recursive: true })
writeFileSync(join(OUT, "sample-flights.json"), JSON.stringify(flights.map((f) => f.summary), null, 2))
for (const f of flights) writeFileSync(join(OUT, `sample-debrief-${f.summary.id}.json`), JSON.stringify(f.debrief))
console.log(flights.map((f) => `${f.summary.id}: ${f.debrief.series.t_s.length} samples, ${f.debrief.events.length} events, ${f.debrief.touchdowns.length} touchdowns, phases ${f.debrief.phases.map((p) => p.phase).join(">")}`).join("\n"))
