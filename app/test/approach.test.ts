import assert from "node:assert/strict";
import test from "node:test";
import { crossing, departures, linesOf, pathHeight, pathSink } from "../src/lib/approach.ts";
import type { ApproachProfile, CriterionAssessment, LandingAssessment, Series } from "../src/types.ts";

const p: ApproachProfile = {
  t_s: [0, 5, 10, 15], dist_nm: [3, 2, 1, 0.2], height_ft: [950, 700, 330, 60], gs_dot: [null, null, null, null], loc_dot: [null, null, null, null],
  path_deg: 3, tch_ft: 50, top_ft: 1000, bottom_ft: 50,
};
const series = { t_s: [0, 15], ias_kt: [100, 100], gs_kt: [90, 90], vs_fpm: [-700, -700], flap_deg: [0, 0], gear_ext: [100, 100] } as unknown as Series;
const crit = (id: string, height: CriterionAssessment["height"], level: number, above: CriterionAssessment["bands"]["above"], below = [null, null, null, null, null] as CriterionAssessment["bands"]["below"], first_out_s?: number) =>
  ({ id, height, level, bands: { above, below }, first_out_s }) as unknown as CriterionAssessment;
const landing = { split_ft: 500, speeds: { vapp_kt: 85 }, profile: p, domains: [{ criteria: [crit("A1", "above", 1, [2.5, 5, null, null, null], [-2.5, -5, null, null, null], 5.4), crit("A1", "below", 1, [2.5, 5, null, null, null], [-2.5, -5, null, null, null], 10)] }] } as unknown as LandingAssessment;

test("the 3 degree path and its sink at a ground speed", () => {
  assert.ok(Math.abs(pathHeight(p, 1) - (50 + 6076 * Math.tan((3 * Math.PI) / 180))) < 1e-9);
  assert.ok(Math.abs(pathSink(p, 100) - 530.9) < 0.5);
});

test("a gate's distance is interpolated where the height falls through it", () => {
  assert.equal(crossing(p, 1000), null);
  assert.ok(Math.abs(crossing(p, 500)! - 1.5) < 0.1);
});

test("the first point outside tolerance is where the score found it, the earlier half first", () => {
  const lines = linesOf(series, p, 85);
  const [d] = departures(landing, lines);
  assert.equal(d?.id, "A1");
  // first_out_s 5.4 s is nearest the point at 5 s.
  assert.equal(d?.index, 1);
  assert.equal(d?.text, "2.0 nm, 700 ft: 15 kt fast");
});

test("a criterion at level 0 is not listed", () => {
  const ok = { ...landing, domains: [{ criteria: [crit("A1", "above", 0, [2.5, 5, null, null, null])] }] } as unknown as LandingAssessment;
  assert.deepEqual(departures(ok, linesOf(series, p, 85)), []);
});
