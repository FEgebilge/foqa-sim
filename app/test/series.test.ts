import assert from "node:assert/strict";
import test from "node:test";
import { adjacentEvent, apSpans, approachSpeedsAt, eventMode, gearAt, indexAt, instrumentsAt, modeAt, trackAt, valueAt } from "../src/lib/series.ts";
import type { Debrief, FlightEvent, LandingAssessment, Series } from "../src/types.ts";

const t_s = [0, 0.5, 1, 1.5];

test("values interpolate between recorded neighbours and never across a gap", () => {
  const v = [0, 10, null, 30];
  assert.equal(valueAt(t_s, v, 0.25), 5);
  assert.equal(valueAt(t_s, v, 0.5), 10);
  // Next to the missing sample: no data, not a guess.
  assert.equal(valueAt(t_s, v, 0.75), null);
  assert.equal(valueAt(t_s, v, 1.25), null);
  assert.equal(valueAt(t_s, v, 1.5), 30);
  // Outside the recording.
  assert.equal(valueAt(t_s, v, -0.1), null);
  assert.equal(valueAt(t_s, v, 1.6), null);
  assert.equal(valueAt([], [], 0), null);
  assert.equal(indexAt(t_s, 1.2), 2);
});

test("headings turn the short way round and discretes hold", () => {
  assert.equal(valueAt(t_s, [350, 10, 20, 30], 0.25, "angle"), 0);
  assert.equal(valueAt(t_s, [10, 350, 20, 30], 0.375, "angle"), 355);
  assert.equal(valueAt(t_s, [0, 1, 1, 0], 0.4, "hold"), 0);
  assert.equal(valueAt(t_s, [0, 1, 1, 0], 0.5, "hold"), 1);
});

const series = (patch: Partial<Series>): Series => {
  const n = t_s.length, none = Array(n).fill(null);
  const keys = ["alt_ft", "aal_ft", "ias_kt", "gs_kt", "vs_fpm", "nz_g", "pitch_deg", "roll_deg", "hdg_deg", "ra_ft", "flap_deg", "vfe_kt", "gear_ext", "ap_engaged", "at_engaged", "lat", "lon"] as const;
  return { t_s, ...Object.fromEntries(keys.map((k) => [k, none])), ...patch } as Series;
};

test("track comes from positions and needs the aircraft moving", () => {
  // Due north, about 110 m every 2 s; then due east.
  const moving = series({ t_s: [0, 2, 4, 6], lat: [38, 38.001, 38.002, 38.003], lon: [27, 27, 27, 27] });
  assert.ok(Math.abs(trackAt(moving, 3)!) < 1e-6);
  const east = series({ t_s: [0, 2, 4, 6], lat: [38, 38, 38, 38], lon: [27, 27.001, 27.002, 27.003] });
  assert.ok(Math.abs(trackAt(east, 3)! - 90) < 0.01);
  const stopped = series({ t_s: [0, 2, 4, 6], lat: [38, 38, 38.00001, 38.00001], lon: [27, 27, 27, 27] });
  assert.equal(trackAt(stopped, 3), null);
  // Positions missing on one side.
  assert.equal(trackAt(moving, 0.5), null);
});

test("the gear is read in percent: up, in transit, down", () => {
  const gear = series({ gear_ext: [0, 50, 100, null] });
  assert.deepEqual(gearAt(gear, 0), { state: "up" });
  assert.deepEqual(gearAt(gear, 0.5), { state: "transit", extended: 0.5 });
  assert.deepEqual(gearAt(gear, 1.2), { state: "down" });
  assert.equal(gearAt(gear, 1.5), null);
});

const speeds = { vref_kt: 60, vapp_kt: 65, source: "profile" as const, approximate: false, basis: "POH" };
const landing = (t: number, s: typeof speeds | null) => ({ t_s: t, speeds: s }) as LandingAssessment;

test("the speed bug is the landing ahead's, and only with its speeds known", () => {
  const landings = [landing(100, speeds), landing(300, null), landing(500, { ...speeds, vapp_kt: 70 })];
  assert.deepEqual(approachSpeedsAt(landings, 50), { ...speeds, landing: 0 });
  assert.equal(approachSpeedsAt(landings, 125)?.landing, 0);
  // The second landing has no reference speeds: no bug, not the next landing's.
  assert.equal(approachSpeedsAt(landings, 200), null);
  assert.equal(approachSpeedsAt(landings, 400)?.vapp_kt, 70);
  assert.equal(approachSpeedsAt(landings, 600), null);
});

test("the instruments read no data where a parameter is missing", () => {
  const debrief = {
    flight: { profile_limits: { vmo: { value: 163, source: "POH" } }, assessment_detail: { landings: [landing(10, speeds)] } },
    series: series({ ias_kt: [60, 70, 80, 90], ra_ft: [100, 3000, 3000, 3000], hdg_deg: [350, 10, 10, 10], ap_engaged: [0, 1, 1, 1] }),
  } as unknown as Debrief;
  const at = instrumentsAt(debrief, 0.25);
  assert.equal(at.ias, 65);
  assert.equal(at.hdg, 0);
  assert.equal(at.ap, false);
  assert.equal(at.at, null);
  assert.equal(at.pitch, null);
  assert.equal(at.gear, null);
  assert.equal(at.vmo, 163);
  assert.equal(at.speeds?.vapp_kt, 65);
  // Radio height only below 2500 ft.
  assert.equal(instrumentsAt(debrief, 1).ra, null);
  assert.equal(instrumentsAt(debrief, 0).ra, 100);
});

test("event jumps go by peak time, in order, and stop at the ends", () => {
  const e = (peak: number) => ({ start_s: peak - 1, peak_s: peak }) as FlightEvent;
  const events = [e(50), e(10), e(30)];
  assert.equal(adjacentEvent(events, 0, 1), 1);
  assert.equal(adjacentEvent(events, 10, 1), 2);
  assert.equal(adjacentEvent(events, 30, -1), 1);
  assert.equal(adjacentEvent(events, 50, 1), null);
  assert.equal(adjacentEvent(events, 10, -1), null);
});

test("autopilot stretches and each event's mode come from the engaged series", () => {
  const series = { t_s: [0, 1, 2, 3, 4], ap_engaged: [0, 1, 1, 0, 1] } as unknown as Series;
  assert.deepEqual(apSpans(series), [{ start: 1, end: 3 }, { start: 4, end: 4 }]);
  assert.equal(modeAt(series, 1.5), "ap");
  assert.equal(modeAt(series, 3.2), "manual");
  assert.equal(eventMode(series, { start_s: 0.5 } as FlightEvent), "manual");
  // Not recorded: no stretches and no mode, rather than "by hand".
  const none = { t_s: [0, 1], ap_engaged: [null, null] } as unknown as Series;
  assert.equal(apSpans(none), null);
  assert.equal(modeAt(none, 0.5), null);
});
