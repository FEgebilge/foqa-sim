// The aviation and scoring terms the app shows, each with a short, exact definition for a pilot who flies at home.
// `<Term id="…">` (components/Term.tsx) puts one behind a dotted underline; nothing else defines these words.

export interface GlossaryEntry {
  /** The term as written, abbreviation first. */
  term: string;
  /** The abbreviation spelled out; absent when the term is a plain word. */
  long?: string;
  text: string;
}

export const GLOSSARY = {
  aal: { term: "AAL", long: "Above airport level", text: "Height above the landing airport's elevation, from the altimeter. The approach gates (1,000 ft, 500 ft) are measured this way." },
  ra: { term: "RA", long: "Radio altitude", text: "Height above the ground directly below, from the radar altimeter. It follows the terrain, so it is the height used close to the runway." },
  ias: { term: "IAS", long: "Indicated airspeed", text: "The speed on the airspeed indicator. The aircraft flies and stalls by it, so every reference speed (Vref, flap limits) is in IAS." },
  gs: { term: "GS", long: "Ground speed", text: "Speed over the ground: IAS corrected for altitude and wind. A headwind makes it lower than IAS." },
  vs: { term: "VS", long: "Vertical speed", text: "Rate of climb or descent in feet per minute; negative is down." },
  load: { term: "Load", long: "Normal load factor", text: "Acceleration through the floor of the aircraft in g. 1 g is level flight; a firm touchdown briefly shows more." },
  "peak-g": { term: "Peak g", long: "Peak normal load at touchdown", text: "The highest load factor in the second around a touchdown. It is how hard the landing felt in the cabin and what limits the structure, independent of the sink rate." },
  sink: { term: "Sink rate", text: "How fast the aircraft was descending at the moment the wheels touched, in feet per minute. Around 100–300 fpm is a normal touchdown; 600 fpm is a structural design limit for transport aircraft." },
  tdz: { term: "TDZ", long: "Touchdown zone", text: "The first part of the runway past the threshold where the wheels should touch: the first 3,000 ft, or the first third of a shorter runway." },
  "aiming-point": { term: "Aiming point", text: "The two wide white bars past the threshold that the approach path points at; the wheels touch a little beyond them after the flare." },
  tch: { term: "TCH", long: "Threshold crossing height", text: "Height over the runway threshold on the way down; the standard 3° path crosses it at about 50 ft." },
  vref: { term: "Vref", long: "Reference landing speed", text: "1.3 × the stall speed in the landing configuration (1.23 × for newer transport types). The speed to have over the threshold." },
  vapp: { term: "Vapp", long: "Approach speed", text: "The speed flown on final: Vref plus an additive for wind and gusts (usually +5 kt or more), bled off to Vref over the threshold." },
  vso: { term: "Vso", long: "Stall speed, landing configuration", text: "The stall speed with flaps and gear down; the bottom of the white arc on a light aircraft's airspeed indicator." },
  vfe: { term: "VFE", long: "Maximum flap extended speed", text: "The highest speed allowed with a given flap setting. Faster than that, the flaps or their tracks can be damaged." },
  vmo: { term: "Vmo/Mmo", long: "Maximum operating speed / Mach", text: "The speed (in IAS, lower down) or Mach number (higher up) that must not be exceeded in any phase. The red-and-white barber pole on the speed tape." },
  "ils-dots": { term: "ILS dots", long: "Instrument landing system deviation", text: "How far off the glideslope or localizer the aircraft is, in dots on the display. One glideslope dot is about 0.35° off the path; past 1 dot an approach is not stable." },
  stable: { term: "Stable approach", text: "On speed, on path, in the landing configuration and with a normal sink rate by the stabilisation height (1,000 ft in cloud, 500 ft in visual conditions). If it is not, the procedure is to go around." },
  flare: { term: "Flare", text: "The nose-up pitch change just before touchdown that turns the descent into a gentle sink." },
  float: { term: "Float", text: "Flying level just above the runway after the flare because there is too much speed. It uses up runway and puts the touchdown long." },
  crab: { term: "Crab", text: "Flying with the nose turned into the wind so the track stays on the centreline in a crosswind. The angle between nose and track is the crab angle." },
  decrab: { term: "Decrab", text: "Aligning the nose with the runway just before touchdown with rudder (and some bank into the wind), so the wheels do not land sideways." },
  drift: { term: "Drift", text: "Sideways movement over the runway at touchdown. It loads the gear sideways; a crosswind landing aims for zero drift." },
  bounce: { term: "Bounce", text: "The aircraft leaves the ground again after the first contact. Counted as one landing, graded on its first contact." },
  "touch-and-go": { term: "Touch-and-go", text: "A landing followed by a take-off without stopping. It is scored, but has no rollout and does not set the flight's grade." },
  rollout: { term: "Rollout", text: "The ground roll after touchdown, down to taxi speed. The required deceleration is how hard the aircraft had to brake to stop within the runway left." },
  autoland: { term: "Autoland", text: "A landing flown by the autopilot down to the runway, so hand-flying measures do not apply to it." },
  level: { term: "Event level", text: "How far a rule's limit was passed: Low just past the first threshold, Medium a significant deviation, High a limit clearly exceeded. Advisory is recorded for context and is not a fault." },
  severity: { term: "Severity of 3", text: "A measured value placed on the scoring scale: 0 is ideal, 0.5 the edge of checkride tolerance, then 1, 2 and 3 for the three event levels. Fractions place the value between two anchors." },
  sustain: { term: "Sustained", text: "A value counts only if it held for the stated time (for example 3 s), so a single gust or sensor spike does not raise an event." },
  domains: { term: "Assessment areas", text: "The landing score (landing-3) has six areas: approach stability, threshold and flare, touchdown point, lateral control, touchdown contact, and rollout. Each starts with its weight in points and loses points for its worst item only." },
  cap: { term: "Safety cap", text: "A ceiling on the score after a safety-critical event (for example a hard landing or tail strike): however good the rest, the landing cannot score above it." },
  points: { term: "Event points", text: "The older grading used when no landing could be scored: each broken rule counts once at its worst level (Low 1, Medium 2, High 4 points; up to 1 is Good, up to 5 Fair, more Poor)." },
  acs: { term: "ACS", long: "Airman Certification Standards", text: "The FAA's checkride standards; the app's tolerance bands for light aircraft come from them (for example ±5 kt on final)." },
  sop: { term: "SOP", long: "Standard operating procedures", text: "An airline's own procedures and limits; the tolerance bands for airliners come from published SOPs." },
  rms: { term: "RMS", long: "Root mean square", text: "An average size of a value that keeps going up and down, such as vertical acceleration in turbulence. Bigger is rougher." },
  fdm: { term: "FDM / FOQA", long: "Flight data monitoring / Flight operational quality assurance", text: "The airline practice of reading every flight's recorded data against limits to find events before they become incidents. This app does the same for sim flights." },
} as const satisfies Record<string, GlossaryEntry>;

export type TermId = keyof typeof GLOSSARY;

export const isTermId = (id: string): id is TermId => Object.hasOwn(GLOSSARY, id);
