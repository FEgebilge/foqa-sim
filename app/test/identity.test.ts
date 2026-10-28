import assert from "node:assert/strict";
import test from "node:test";
import { identityNotices } from "../src/lib/identity.ts";
import type { Debrief, Identification } from "../src/types.ts";

const flight = (aircraft: Debrief["flight"]["aircraft"], identification?: Identification) =>
  ({ aircraft, identification, profile: "transport class default (no BE35 profile)", class: "transport" }) as unknown as Debrief["flight"];
const id = (matched: Identification["matched"], kind?: "helicopter"): Identification => ({
  matched,
  airframe: kind ? { kind } : {},
  unchecked: ["flap speeds (Vfe)", "tail strike", "hard-landing inspection"],
  runway_landing: !kind,
});

test("a profiled aircraft needs no notice", () => {
  assert.deepEqual(identityNotices(flight({ icao: "C172", title: "C172SP" }, { ...id("type"), unchecked: [] })), []);
  assert.deepEqual(identityNotices(flight({ icao: "B738", title: "PMDG 737-800" }, id("title"))), []);
});

test("a class default says where the class came from and which checks were off", () => {
  const [n] = identityNotices(flight({ icao: "BE35", title: "King Air 350" }, id("sim")));
  assert.equal(n.title, "Graded on the transport class default");
  assert.match(n.body, /^No type profile matches BE35: its class comes from what the sim reports/);
  assert.match(n.body, /These checks were off: flap speeds \(Vfe\), tail strike and hard-landing inspection\./);
  assert.match(identityNotices(flight({ title: "X" }, id("weight")))[0].body, /recorded gross weight/);
});

test("an unidentified aircraft keeps its notice, with or without the identification", () => {
  assert.match(identityNotices(flight(undefined, id("unknown")))[0].body, /so flap speeds \(Vfe\), tail strike and hard-landing inspection could not be checked/);
  assert.equal(identityNotices(flight(undefined))[0].kind, "unidentified");
});

test("a helicopter's landing is said to be not assessed", () => {
  const notices = identityNotices(flight({ icao: "H125", title: "H125" }, id("sim", "helicopter")));
  assert.deepEqual(notices.map((n) => n.kind), ["class-default", "not-assessed"]);
  assert.match(notices[1].body, /^A helicopter gets no airplane landing score/);
});
