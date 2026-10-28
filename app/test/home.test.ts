import assert from "node:assert/strict";
import { test } from "node:test";
import { firstRunStep, packagesState } from "../src/components/home/first-run.ts";
import { byNewest, counted, ordinal, personal, scoredLanding, WINDOW } from "../src/components/home/latest.ts";
import { failing } from "../src/components/setup/setup-groups.ts";
import type { CheckState, FlightSummary, LandingFacts, SetupStatus } from "../src/types.ts";

const setup = (sim: CheckState, packages: CheckState[], recorder: CheckState = "pass"): SetupStatus => ({
  sims: [], community: null, choice: "found",
  checks: [
    { id: "sim", title: "MSFS 2024", state: sim, detail: "" },
    ...packages.map((state, i) => ({ id: `package:p${i}`, title: `p${i}`, state, detail: "" })),
    { id: "recorder", title: "Recorder", state: recorder, detail: "" },
  ],
});

test("packages: checking, installed, missing, skipped, and not applicable without MSFS 2024", () => {
  assert.equal(packagesState(null, false), "checking");
  assert.equal(packagesState(setup("pass", ["pass", "warn"]), false), "installed");
  assert.equal(packagesState(setup("pass", ["pass", "fail"]), false), "missing");
  assert.equal(packagesState(setup("pass", ["fail"]), true), "skipped");
  assert.equal(packagesState(setup("fail", []), false), "no_sim");
});

test("the first-run step follows the recorder; packages lead only while the sim is off", () => {
  assert.equal(firstRunStep("checking", undefined, false), 0);
  assert.equal(firstRunStep("missing", "waiting_for_sim", false), 0);
  assert.equal(firstRunStep("installed", "waiting_for_sim", false), 1);
  assert.equal(firstRunStep("no_sim", "error", false), 1);
  assert.equal(firstRunStep("missing", "connected", false), 2);
  assert.equal(firstRunStep("installed", "recording", false), 2);
  assert.equal(firstRunStep("missing", "connected", true), 3);
});

test("failing ignores the in-sim panel without MSFS 2024, never the recorder", () => {
  assert.deepEqual(failing(setup("fail", [], "fail")).map((c) => c.id), ["recorder"]);
  assert.deepEqual(failing(setup("pass", ["fail"])).map((c) => c.id), ["package:p0"]);
  assert.deepEqual(failing(setup("fail", [])).map((c) => c.id), []);
});

const landing = (sink_fpm: number, peak_g: number, extra: Partial<LandingFacts> = {}): LandingFacts =>
  ({ t_s: 0, full_stop: true, score: 70, domains: {}, sink_fpm, peak_g, bounces: 0, ...extra });
let day = 0;
const flight = (icao: string | undefined, score: number | undefined, sink = 200, g = 1.3, extra: Partial<FlightSummary> = {}): FlightSummary => ({
  id: `f${++day}`, started_utc: new Date(Date.UTC(2026, 9, day)).toISOString(), duration_s: 1800,
  aircraft: icao ? { icao, title: icao } : undefined, landings: 1, event_counts: { advisory: 0, low: 0, medium: 0, high: 0 },
  assessment: { grade: "fair", points: 0, findings: 0, score }, landings_detail: [landing(sink, g)], ...extra,
});

test("counted follows the trends rule; a summary without flown counts", () => {
  const flown = { airborne_s: 59, distance_nm: 5, max_gs_kt: 100, started_airborne: false, ended_airborne: false };
  assert.equal(counted(flight("C172", 70, 200, 1.3, { flown })), false);
  assert.equal(counted(flight("C172", 70, 200, 1.3, { flown: { ...flown, airborne_s: 60 } })), true);
  assert.equal(counted(flight("C172", 70)), true);
});

test("byNewest drops uncounted flights and sorts by start time", () => {
  const old = flight("C172", 60), ground = flight("C172", 50, 0, 1, { flown: { airborne_s: 0, distance_nm: 0, max_gs_kt: 0, started_airborne: false, ended_airborne: false } }), fresh = flight("C172", 80);
  assert.deepEqual(byNewest([old, fresh, ground]).map((f) => f.id), [fresh.id, old.id]);
});

test("scoredLanding takes the last scored full stop, else the last landing", () => {
  const touch = landing(100, 1.1, { full_stop: false }), stop = landing(150, 1.2), go = landing(300, 1.5, { full_stop: false });
  assert.equal(scoredLanding(flight("C172", 70, 0, 0, { landings_detail: [touch, stop, go] })), stop);
  assert.equal(scoredLanding(flight("C172", 70, 0, 0, { landings_detail: [touch, go] })), go);
  assert.equal(scoredLanding(flight("C172", 70, 0, 0, { landings_detail: undefined })), undefined);
});

test("personal compares the latest with your previous flights, in the type and overall", () => {
  // Oldest to newest: three C172s, a B738, an unscored C172, then the latest C172.
  const flights = byNewest([flight("C172", 60, 300, 1.5), flight("C172", 80, 100, 1.3), flight("C172", 70, 200, 1.4), flight("B738", 40), flight("C172", undefined), flight("C172", 75, 150, 1.2)]);
  const p = personal(flights)!;
  assert.deepEqual(p.type, { n: 3, mean: 70, delta: 5 });
  assert.equal(p.overall!.n, 4);
  assert.ok(Math.abs(p.overall!.delta - 12.5) < 1e-9);
  assert.deepEqual(p.rank, { place: 2, of: 4 });
  assert.deepEqual(p.spark, [60, 80, 70, 75]);
  assert.deepEqual(p.sink, { n: 3, mean: 200, delta: -50 });
  assert.ok(Math.abs(p.g!.delta + 0.2) < 1e-9);
});

test("personal: a first flight, an unscored one, and the window", () => {
  assert.equal(personal([]), null);
  const first = personal([flight("C172", 70)])!;
  assert.equal(first.type, null);
  assert.deepEqual(first.rank, { place: 1, of: 1 });
  assert.deepEqual(first.spark, [70]);
  const unscored = personal(byNewest([flight("C172", 70), flight("C172", undefined)]))!;
  assert.equal(unscored.type, null);
  assert.equal(unscored.rank, null);
  assert.deepEqual(unscored.spark, []);
  const many = personal(byNewest(Array.from({ length: 15 }, (_, i) => flight("A20N", 50 + i))))!;
  assert.equal(many.type!.n, WINDOW);
  assert.equal(many.spark.length, WINDOW + 1);
  assert.equal(many.rank!.place, 1);
  const untyped = personal([flight(undefined, 70), flight(undefined, 60)])!;
  assert.equal(untyped.type, null);
  // One earlier flight is not enough to compare with.
  assert.equal(untyped.overall, null);
});

test("ordinal", () => {
  assert.deepEqual([1, 2, 3, 4, 11, 12, 13, 21, 22, 101, 111].map(ordinal), ["1st", "2nd", "3rd", "4th", "11th", "12th", "13th", "21st", "22nd", "101st", "111th"]);
});
