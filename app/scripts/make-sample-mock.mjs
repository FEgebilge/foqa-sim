// Writes a synthetic sample flight list and debriefs to public/mock/sample-*.json, following docs/debrief-api.md.
// Only for developing the UI in a browser before real data exists; the API layer prefers flights.json / debrief-<id>.json.
// A C172 flies two left-hand circuits on LTFA runway 35: a bounced touch-and-go, then a long full-stop landing.
import { mkdirSync, writeFileSync } from "node:fs"
import { join } from "node:path"

const OUT = join(import.meta.dirname, "../public/mock")
const KT = 0.514444, FT = 0.3048, G = 9.80665, DEG = Math.PI / 180
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
  return { raw, mains, peaks, keys, N, td1, td2 }
}

function phaseOf(x, ctx) {
  if (x.t < 40) return "Standing"
  if (x.s < ctx.N[1]) return "Taxi"
  if (x.gsKt === 0) return "Standing"
  if (x.s > ctx.td2 && x.gsKt < 30) return "Taxi"
  if (x.h <= 0.05) return x.s > ctx.td1 + 280 && x.s < ctx.td2 - 300 ? "Takeoff" : x.s >= ctx.td1 - 200 ? "Landing" : "Takeoff"
  if (x.s > ctx.td1 - 200 && x.s < ctx.td1 + 280) return "Landing"
  if (x.h < 35 * FT && x.vs > 0) return "Takeoff"
  if (x.h < 15 && x.vs <= 0) return "Landing"
  if (x.vs > 150 && x.h < 1000 * FT - 1) return "InitialClimb"
  if (x.vs < -150 || x.h < 1000 * FT - 3) return x.vs > 150 ? "InitialClimb" : "Approach"
  return "EnRoute"
}

function build(id, opts = {}) {
  const ctx = generate(opts)
  const { raw, mains, peaks } = ctx
  const near = (t) => mains.some((i) => Math.abs(t - raw[i].t) <= 10)
  const keep = raw.filter((x) => Math.abs(x.t * 2 - Math.round(x.t * 2)) < 1e-6 || near(x.t))
  const cut = opts.damagedAt ? keep.findIndex((x) => x.t > opts.cutAt) : -1
  const samples = cut > 0 ? keep.slice(0, cut) : keep
  const r1 = (v, d = 1) => Math.round(v * 10 ** d) / 10 ** d
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
    lat: pos.map((p) => r1(p.lat, 6)),
    lon: pos.map((p) => r1(p.lon, 6)),
  }
  if (opts.noPosition) series.lat = series.lat.map((v, i) => (i % 400 < 10 ? null : v))

  const phases = []
  for (const x of samples) {
    const phase = phaseOf(x, ctx)
    const last = phases.at(-1)
    if (last?.phase === phase) last.end_s = r1(x.t, 3)
    else { if (last) last.end_s = r1(x.t, 3); phases.push({ phase, start_s: r1(x.t, 3), end_s: r1(x.t, 3) }) }
  }

  const lastT = samples.at(-1).t
  const withRunway = !opts.noRunway
  const touchdowns = mains.filter((i) => raw[i].t <= lastT).map((i, k) => {
    const x = raw[i], before = raw[i - 5]
    const sink = Math.round(-before.vs)
    return {
      t_s: r1(x.t, 3), bounce: k === 0 && opts.bounce !== false, sink_fpm: sink, sim_sink_fpm: Math.round(sink * 0.93),
      peak_g: peaks[k], pitch_deg: r1(before.pitch), roll_deg: r1(before.roll), ias_kt: r1(x.ias),
      runway: withRunway ? (() => {
        const cross = k === 0 ? -2.8 : opts.secondCross ?? 9.5
        return { icao: "LTFA", runway: "35", along_m: Math.round(x.a), cross_m: cross, lda_m: LDA, tdz_m: Math.round(TDZ), in_tdz: x.a >= 0 && x.a <= TDZ, long_landing: x.a > TDZ, short_landing: x.a < 0 }
      })() : undefined,
    }
  })

  const events = []
  const add = (e) => events.push({ version: 1, ...e })
  const span = (pred) => { const out = []; let cur = null; for (const x of samples) { if (pred(x)) { cur ??= { start: x.t, xs: [] }; cur.end = x.t; cur.xs.push(x) } else if (cur) { out.push(cur); cur = null } } if (cur) out.push(cur); return out }
  const ruleText = {
    "bank-below-500": ["Excessive bank below 500 ft", "Initial tier from the FOQA rule research: more than 15 deg below 500 ft. Higher levels are a FOQA-Sim estimate."],
    "flap-overspeed": ["Flap overspeed", "IAS above the VFE for the flap angle, which comes from the aircraft's type profile (profiles/<ICAO>.yaml, manufacturer's airspeed limitations). Silent without a profile. Levels above VFE are a FOQA-Sim estimate."],
    "hard-landing-light": ["Hard landing (light aircraft)", "Peak normal load at touchdown, light aircraft (normal-category limit load 3.8 g). Levels are a FOQA-Sim estimate."],
    "bounced-landing": ["Bounced landing", "More than one main-gear air-to-ground transition (EASA EOFDM RE30)."],
    "long-landing": ["Long landing", "Touchdown beyond the touchdown zone, whose length is min(3000 ft, LDA/3) of the landing runway (sources to be confirmed: FAA P/CG 3000 ft, one-third rule). Needs the airport data recorded in flight."],
    "off-centreline-touchdown": ["Touchdown off the runway centreline", "Distance of the touchdown from the runway centreline. Levels are a FOQA-Sim estimate. Needs the airport data recorded in flight."],
  }
  const text = (rule) => ({ rule, title: ruleText[rule][0], description: ruleText[rule][1] })
  for (const b of span((x) => x.h < 500 * FT && x.h > 0.05 && Math.abs(x.roll) > 15)) {
    if (b.end - b.start < 2) continue
    const peak = Math.max(...b.xs.map((x) => Math.abs(x.roll)))
    const level = peak > 30 ? "high" : peak > 20 ? "medium" : "low"
    add({ ...text("bank-below-500"), level, start_s: r1(b.start, 3), end_s: r1(b.end, 3), measure: "abs(roll)", value: r1(peak), threshold: { low: 15, medium: 20, high: 30 }[level], unit: "deg" })
  }
  for (const b of span((x) => x.flap >= 20 && x.ias > 85 && x.h > 0.05)) {
    if (b.end - b.start < 2) continue
    const excess = Math.max(...b.xs.map((x) => x.ias - 85))
    const level = excess > 10 ? "high" : excess > 5 ? "medium" : "low"
    add({ ...text("flap-overspeed"), level, start_s: r1(b.start, 3), end_s: r1(b.end, 3), measure: "ias - profile.vfe", value: r1(excess), threshold: { low: 0, medium: 5, high: 10 }[level], unit: "kt" })
  }
  for (const td of touchdowns) {
    const at = { start_s: td.t_s, end_s: td.t_s }
    if (td.peak_g > 1.8) add({ ...text("hard-landing-light"), ...at, level: td.peak_g > 2.6 ? "high" : td.peak_g > 2.2 ? "medium" : "low", measure: "td.peak_g", value: td.peak_g, threshold: 1.8, unit: "g" })
    if (td.bounce) add({ ...text("bounced-landing"), ...at, level: "medium", measure: "td.bounce" })
    if (td.runway?.long_landing) add({ ...text("long-landing"), ...at, level: "medium", measure: "td.long_landing" })
    const crossFt = td.runway ? Math.abs(td.runway.cross_m) / FT : 0
    if (crossFt > 25) add({ ...text("off-centreline-touchdown"), ...at, level: crossFt > 60 ? "high" : crossFt > 40 ? "medium" : "low", measure: "abs(td.cross)", value: r1(crossFt), threshold: crossFt > 60 ? 60 : crossFt > 40 ? 40 : 25, unit: "ft" })
  }
  const rank = { high: 0, medium: 1, low: 2, advisory: 3 }
  events.sort((a, b) => a.start_s - b.start_s || rank[a.level] - rank[b.level])
  // JSON key order follows the contract.
  const ordered = events.map(({ rule, version, title, description, level, start_s, end_s, measure, value, threshold, unit }) => ({ rule, version, title, description, level, start_s, end_s, measure, value, threshold, unit }))

  const counts = { advisory: 0, low: 0, medium: 0, high: 0 }
  ordered.forEach((e) => counts[e.level]++)
  const worst = ["high", "medium", "low", "advisory"].find((l) => counts[l] > 0)
  const summary = {
    id, started_utc: opts.started, duration_s: r1(lastT), aircraft: opts.aircraft,
    departure: "LTFA", arrival: withRunway && touchdowns.length ? "LTFA" : undefined,
    landings: touchdowns.length, worst_level: worst, event_counts: counts,
  }
  const thr17 = toLatLon(LDA, 0)
  const debrief = {
    flight: { ...summary, lfl_version: 1, profile: opts.profile, damaged_at: opts.damagedAt },
    phases, touchdowns, events: ordered, series,
    airports: withRunway ? [{ icao: "LTFA", runways: [{ width_m: 45, ends: [
      { runway: "35", lat: THR35.lat, lon: THR35.lon, heading_deg: HDG, lda_m: LDA, tdz_m: Math.round(TDZ) },
      { runway: "17", lat: r1(thr17.lat, 6), lon: r1(thr17.lon, 6), heading_deg: 170, lda_m: LDA, tdz_m: Math.round(TDZ) },
    ] }] }] : [],
  }
  return { summary, debrief }
}

const c172 = { icao: "C172", title: "Cessna Skyhawk G1000 Asobo" }
const flights = [
  build("flight-1793027213", { started: "2026-10-26T14:26:53Z", aircraft: c172, profile: "C172 profile" }),
  build("flight-1792941000", { started: "2026-10-25T14:30:00Z", aircraft: c172, profile: "C172 profile", bounce: false, overspeed: false, secondAlong: 420, secondCross: 1.2, radius: 1100 }),
  build("flight-1792850400", { started: "2026-10-24T13:20:00Z", aircraft: { icao: "B738", title: "Boeing 737-800 Asobo" }, profile: "B738 profile", noRunway: true, bounce: false }),
  build("flight-1792764000", { started: "2026-10-23T13:20:00Z", aircraft: c172, profile: "C172 profile", damagedAt: 1048576, cutAt: 520 }),
  build("flight-1792677600", { started: "2026-10-22T13:20:00Z", profile: "light class default (no aircraft identity)", overspeed: false }),
]
mkdirSync(OUT, { recursive: true })
writeFileSync(join(OUT, "sample-flights.json"), JSON.stringify(flights.map((f) => f.summary), null, 2))
for (const f of flights) writeFileSync(join(OUT, `sample-debrief-${f.summary.id}.json`), JSON.stringify(f.debrief))
console.log(flights.map((f) => `${f.summary.id}: ${f.debrief.series.t_s.length} samples, ${f.debrief.events.length} events, ${f.debrief.touchdowns.length} touchdowns, phases ${f.debrief.phases.map((p) => p.phase).join(">")}`).join("\n"))
