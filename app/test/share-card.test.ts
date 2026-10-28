import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { cardData, cardFileName } from "../src/lib/share-card.ts";
import type { Debrief } from "../src/types.ts";

const mock = (name: string) => JSON.parse(readFileSync(new URL(`../public/mock/${name}`, import.meta.url), "utf8")) as Debrief;
/** A real C172 flight: one landing on LTBL 17, scored 77.4 (Fair). */
const real = mock("debrief-flight-1793037932.json");

test("the card says the route, type, date and score of the landing", () => {
  const card = cardData(real, 0)!;
  assert.equal(card.route, "LTBL → LTBL");
  assert.equal(card.runway, "RWY 17");
  assert.equal(card.aircraft, "C172");
  assert.equal(card.date, "26 Oct 2026");
  assert.equal(card.landing, "Landing");
  assert.equal(card.score, "77.4");
  assert.equal(card.grade, "fair");
  assert.equal(card.gradeLabel, "Fair");
  assert.equal(card.note, null);
});

test("six domain bars keep the share not lost, and the points they cost", () => {
  const { bars } = cardData(real, 0)!;
  const domains = real.flight.assessment_detail.landings![0]!.domains;
  assert.equal(bars.length, 6);
  bars.forEach((bar, i) => {
    const d = domains[i]!;
    assert.equal(bar.label, d.short);
    assert.equal(bar.kept, d.loss === null ? null : 1 - Math.min(1, d.loss));
    assert.ok(bar.level >= 0 && bar.level <= 3);
    assert.match(bar.text, d.loss === null ? /^n\/a$/ : d.points > 0 ? /^−\d+\.\d$/ : /^0$/);
  });
});

test("touchdown values in words, with the side of the centreline and the wind", () => {
  const stats = Object.fromEntries(cardData(real, 0)!.stats.map((s) => [s.label, s]));
  assert.deepEqual(Object.keys(stats), ["Sink rate", "Peak load", "Touchdown", "Centreline", "Wind"]);
  assert.equal(stats["Sink rate"]!.value, "200 fpm");
  assert.equal(stats["Peak load"]!.value, "1.27 g");
  assert.equal(stats.Touchdown!.value, "386 m");
  assert.equal(stats.Touchdown!.sub, "past the threshold · In TDZ");
  assert.equal(stats.Centreline!.value, "5.2 m L");
  assert.equal(stats.Centreline!.sub, "left of the line");
  assert.equal(stats.Wind!.value, "270° 3 kt");
  assert.equal(stats.Wind!.sub, "3 kt from the right");
});

test("never the sim title or tail: the type only", () => {
  const titled = { ...real, flight: { ...real.flight, aircraft: { icao: "C172", title: "C172SP G1000 Passengers" } } };
  assert.ok(!JSON.stringify(cardData(titled, 0)).includes("Passengers"));
  const unknown = { ...real, flight: { ...real.flight, aircraft: undefined } };
  assert.equal(cardData(unknown, 0)!.aircraft, "Unknown aircraft");
});

test("an unscored landing has a note instead of bars; no landing, no card", () => {
  const landings = real.flight.assessment_detail.landings!.map((l) => ({ ...l, status: "insufficient" as const, score: null, note: "No runway data" }));
  const unscored = { ...real, flight: { ...real.flight, assessment_detail: { ...real.flight.assessment_detail, landings } } };
  const card = cardData(unscored, 0)!;
  assert.equal(card.score, null);
  assert.deepEqual(card.bars, []);
  assert.equal(card.note, "No runway data");
  assert.equal(cardData(real, 1), null);
});

test("several landings are numbered and a touch-and-go is named", () => {
  const td = real.touchdowns[0]!;
  const two = { ...real, touchdowns: [{ ...td, t_s: 100 }, { ...td, t_s: 120, bounce: true }, td] };
  const landings = [{ ...real.flight.assessment_detail.landings![0]!, t_s: 100, full_stop: false }, real.flight.assessment_detail.landings![0]!];
  const flight = { ...real.flight, assessment_detail: { ...real.flight.assessment_detail, landings } };
  assert.equal(cardData({ ...two, flight }, 0)!.landing, "Touch-and-go 1 of 2");
  assert.equal(cardData({ ...two, flight }, 1)!.landing, "Landing 2 of 2");
});

test("the file name is the airport, runway, date and landing", () => {
  assert.equal(cardFileName(real, 0), "foqa-sim-ltbl-17-2026-10-26-landing-1.png");
});
