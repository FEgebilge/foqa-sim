import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { CloudOff, MapPinOff } from "lucide-react";
import { LngLatBounds, Map as MapLibre, Marker, NavigationControl, ScaleControl, setWorkerUrl, type AddLayerObject, type GeoJSONSource, type TransformStyleFunction } from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";
import workerUrl from "maplibre-gl/dist/maplibre-gl-worker.mjs?worker&url";
import { EmptyState } from "@ui/components/empty-state/empty-state";
import { phaseName } from "../../lib/format";
import { levelColor, phaseColor, type Theme } from "../../lib/palette";
import { indexAt, type TimeBus } from "../../lib/time";
import type { Debrief, FlightEvent, RunwayEnd } from "../../types";

// MapLibre finds its worker next to its own module, which Vite pre-bundling moves; point it at the emitted file instead.
setWorkerUrl(workerUrl);

type Style = ReturnType<TransformStyleFunction>;
type Source = Style["sources"][string];
type Position = [lon: number, lat: number];
type Geometry = { type: "LineString"; coordinates: Position[] } | { type: "Polygon"; coordinates: Position[][] } | { type: "Point"; coordinates: Position };
type Feature = { type: "Feature"; properties: Record<string, string | number>; geometry: Geometry };
const collection = (features: Feature[]) => ({ type: "FeatureCollection" as const, features });
const geojson = (features: Feature[]): Source => ({ type: "geojson", data: collection(features) });

// Free vector basemaps, no API key (https://openfreemap.org); their styles carry the OSM/OpenMapTiles attribution.
// The overlay stays usable without them.
const BASE = { light: "https://tiles.openfreemap.org/styles/liberty", dark: "https://tiles.openfreemap.org/styles/dark" };

/** Metres east/north of `origin` to lon/lat (equirectangular; fine over a runway's length). */
const offset = (origin: RunwayEnd, east: number, north: number): Position => [origin.lon + east / (111_320 * Math.cos((origin.lat * Math.PI) / 180)), origin.lat + north / 111_320];

function runwayFeatures(debrief: Debrief, theme: Theme): Feature[] {
  const features: Feature[] = [];
  for (const airport of debrief.airports) for (const runway of airport.runways) {
    const [a, b] = runway.ends;
    if (!a || !b) continue;
    const dx = (b.lon - a.lon) * 111_320 * Math.cos((a.lat * Math.PI) / 180), dy = (b.lat - a.lat) * 111_320;
    const length = Math.hypot(dx, dy) || 1;
    const u = [dx / length, dy / length], n = [u[1]!, -u[0]!];
    const half = runway.width_m / 2;
    const at = (along: number, side: number) => offset(a, u[0]! * along + n[0]! * side, u[1]! * along + n[1]! * side);
    const rect = (from: number, to: number, w: number): Geometry => ({ type: "Polygon", coordinates: [[at(from, -w), at(to, -w), at(to, w), at(from, w), at(from, -w)]] });
    features.push({ type: "Feature", properties: { kind: "runway" }, geometry: rect(0, length, half) });
    features.push({ type: "Feature", properties: { kind: "tdz", color: theme === "dark" ? "#4fd08e" : "#1d8f5c" }, geometry: rect(0, Math.min(a.tdz_m, length / 2), half) });
    features.push({ type: "Feature", properties: { kind: "tdz", color: theme === "dark" ? "#4fd08e" : "#1d8f5c" }, geometry: rect(length - Math.min(b.tdz_m, length / 2), length, half) });
    for (const along of [0, length]) features.push({ type: "Feature", properties: { kind: "threshold" }, geometry: { type: "LineString", coordinates: [at(along, -half - 6), at(along, half + 6)] } });
    features.push({ type: "Feature", properties: { kind: "centreline" }, geometry: { type: "LineString", coordinates: [at(0, 0), at(length, 0)] } });
  }
  return features;
}

/** Track split at phase boundaries (and at missing positions), each piece in its phase colour. */
function trackFeatures(debrief: Debrief, theme: Theme): Feature[] {
  const { t_s, lat, lon } = debrief.series;
  const features: Feature[] = [];
  for (const phase of debrief.phases) {
    let line: Position[] = [];
    const flush = () => { if (line.length > 1) features.push({ type: "Feature", properties: { color: phaseColor(phase.phase, theme) }, geometry: { type: "LineString", coordinates: line } }); line = []; };
    for (let i = indexAt(t_s, phase.start_s); i < t_s.length && t_s[i]! <= phase.end_s; i++) {
      const la = lat[i], lo = lon[i];
      if (la == null || lo == null) flush(); else line.push([lo, la]);
    }
    flush();
  }
  return features;
}

/** Every recorded position with its sample index, for picking the nearest point under the mouse. */
const positions = (debrief: Debrief) => debrief.series.t_s.flatMap((_, i) => { const la = debrief.series.lat[i], lo = debrief.series.lon[i]; return la == null || lo == null ? [] : [{ i, p: [lo, la] as Position }]; });

/** Initial bearing from a to b, degrees true. */
function bearing([lo1, la1]: Position, [lo2, la2]: Position) {
  const r = Math.PI / 180, dl = (lo2 - lo1) * r;
  return Math.atan2(Math.sin(dl) * Math.cos(la2 * r), Math.cos(la1 * r) * Math.sin(la2 * r) - Math.sin(la1 * r) * Math.cos(la2 * r) * Math.cos(dl)) / r;
}

/** About `count` direction arrows spaced evenly along the track, each pointing along it. */
function arrows(track: { p: Position }[], count: number) {
  const steps = track.slice(1).map((point, k) => Math.hypot(point.p[0] - track[k]!.p[0], point.p[1] - track[k]!.p[1]));
  const total = steps.reduce((sum, d) => sum + d, 0);
  const out: { at: Position; deg: number }[] = [];
  let next = total / count / 2, run = 0;
  steps.forEach((d, k) => {
    run += d;
    if (run < next || d === 0) return;
    out.push({ at: track[k + 1]!.p, deg: bearing(track[k]!.p, track[Math.min(k + 4, track.length - 1)]!.p) });
    next += total / count;
  });
  return out;
}

const marker = (className: string, html: string) => { const el = document.createElement("div"); el.className = className; el.innerHTML = html; return el; };

/** Position at time t, interpolated between the two samples around it (the recording is sparse, about 2 Hz). */
function positionAt(debrief: Debrief, t: number): Position | null {
  const { t_s, lat, lon } = debrief.series;
  const i = indexAt(t_s, t);
  const la = lat[i], lo = lon[i];
  if (la == null || lo == null) return null;
  const la2 = lat[i + 1], lo2 = lon[i + 1], span = (t_s[i + 1] ?? t_s[i]!) - t_s[i]!;
  if (la2 == null || lo2 == null || span <= 0) return [lo, la];
  const f = Math.min(1, Math.max(0, (t - t_s[i]!) / span));
  return [lo + (lo2 - lo) * f, la + (la2 - la) * f];
}

function selectionFeatures(debrief: Debrief, event: FlightEvent | null, theme: Theme): Feature[] {
  if (!event) return [];
  const { t_s } = debrief.series;
  const coordinates: Position[] = [];
  for (let i = indexAt(t_s, event.start_s); i < t_s.length && t_s[i]! <= Math.max(event.end_s, event.start_s + 1); i++) {
    const p = positionAt(debrief, t_s[i]!);
    if (p) coordinates.push(p);
  }
  const color = levelColor(event.level, theme);
  return coordinates.length > 1
    ? [{ type: "Feature", properties: { color }, geometry: { type: "LineString", coordinates } }]
    : coordinates.length ? [{ type: "Feature", properties: { color }, geometry: { type: "Point", coordinates: coordinates[0]! } }] : [];
}

function overlay(debrief: Debrief, theme: Theme, selected: FlightEvent | null) {
  const dark = theme === "dark";
  const touchdowns = debrief.touchdowns.flatMap((td): Feature[] => { const p = positionAt(debrief, td.t_s); return p ? [{ type: "Feature", properties: {}, geometry: { type: "Point", coordinates: p } }] : []; });
  const sources: Record<string, Source> = {
    "foqa-runways": geojson(runwayFeatures(debrief, theme)),
    "foqa-track": geojson(trackFeatures(debrief, theme)),
    "foqa-selection": geojson(selectionFeatures(debrief, selected, theme)),
    "foqa-touchdowns": geojson(touchdowns),
  };
  const layers: AddLayerObject[] = [
    { id: "foqa-runway", type: "fill", source: "foqa-runways", filter: ["==", ["get", "kind"], "runway"], paint: { "fill-color": dark ? "#5a6378" : "#5b6270", "fill-opacity": 0.95 } },
    { id: "foqa-tdz", type: "fill", source: "foqa-runways", filter: ["==", ["get", "kind"], "tdz"], paint: { "fill-color": ["get", "color"], "fill-opacity": 0.45 } },
    { id: "foqa-centreline", type: "line", source: "foqa-runways", filter: ["==", ["get", "kind"], "centreline"], paint: { "line-color": "#ffffff", "line-width": 1, "line-dasharray": [4, 4], "line-opacity": 0.7 } },
    { id: "foqa-threshold", type: "line", source: "foqa-runways", filter: ["==", ["get", "kind"], "threshold"], paint: { "line-color": "#ffffff", "line-width": 3 } },
    { id: "foqa-track-casing", type: "line", source: "foqa-track", layout: { "line-cap": "round", "line-join": "round" }, paint: { "line-color": dark ? "#0b1220" : "#ffffff", "line-width": 5, "line-opacity": 0.7 } },
    { id: "foqa-track", type: "line", source: "foqa-track", layout: { "line-cap": "round", "line-join": "round" }, paint: { "line-color": ["get", "color"], "line-width": 3 } },
    { id: "foqa-selection", type: "line", source: "foqa-selection", layout: { "line-cap": "round" }, paint: { "line-color": ["get", "color"], "line-width": 7, "line-opacity": 0.9 } },
    { id: "foqa-selection-point", type: "circle", source: "foqa-selection", filter: ["==", ["geometry-type"], "Point"], paint: { "circle-color": ["get", "color"], "circle-radius": 8 } },
    { id: "foqa-touchdowns", type: "circle", source: "foqa-touchdowns", paint: { "circle-color": dark ? "#ffffff" : "#111827", "circle-radius": 5, "circle-stroke-color": dark ? "#111827" : "#ffffff", "circle-stroke-width": 2 } },
  ];
  return { sources, layers };
}

const blank = (theme: Theme): Style => ({ version: 8, sources: {}, layers: [{ id: "background", type: "background", paint: { "background-color": theme === "dark" ? "#141b2b" : "#eef1f5" } }] });

interface Props { debrief: Debrief; theme: Theme; focus: number | null; selected: FlightEvent | null; bus: TimeBus; onFocus?: (t: number) => void }

/** Pixels from the track within which the mouse picks a point of it. */
const PICK_PX = 14;
/** The camera recentres on the focused aircraft once it comes this close to the map edge. */
const FOLLOW_PX = 40;
/** Starts at full speed, so a pan retargeted every playback tick still catches up with the aircraft. */
const easeOut = (k: number) => 1 - (1 - k) ** 3;

/**
 * 2D track coloured by phase over runways from the airport snapshot; the dot follows the cursor of the charts.
 * Hovering the track moves the shared cursor, clicking it places the focus there.
 */
export default function FlightMap({ debrief, theme, focus, selected, bus, onFocus }: Props) {
  const host = useRef<HTMLDivElement>(null);
  const mapRef = useRef<MapLibre | null>(null);
  const cursor = useRef<Marker | null>(null);
  const following = useRef(false);
  const [offline, setOffline] = useState(false);
  const hasTrack = useMemo(() => debrief.series.lat.some((v) => v !== null), [debrief]);
  const latest = useRef({ selected, focus, onFocus });
  useLayoutEffect(() => { latest.current = { selected, focus, onFocus }; });
  const phases = useMemo(() => [...new Set(debrief.phases.map((p) => p.phase))], [debrief]);

  // Map and base style: rebuilt for a new flight or theme. The overlay is drawn first on a blank style, then merged into the basemap once it loads.
  useEffect(() => {
    const node = host.current;
    if (!node || !hasTrack) return;
    const withOverlay = (base: Style): Style => {
      const own = overlay(debrief, theme, latest.current.selected);
      return { ...base, sources: { ...base.sources, ...own.sources }, layers: [...base.layers, ...(own.layers as Style["layers"])] };
    };
    const map = new MapLibre({ container: node, style: withOverlay(blank(theme)), attributionControl: { compact: true }, dragRotate: false });
    mapRef.current = map;
    map.addControl(new NavigationControl({ showCompass: false }), "top-right");
    map.addControl(new ScaleControl({ unit: "metric" }), "bottom-left");

    const bounds = new LngLatBounds();
    debrief.series.lat.forEach((la, i) => { const lo = debrief.series.lon[i]; if (la != null && lo != null) bounds.extend([lo, la]); });
    debrief.airports.forEach((airport) => airport.runways.forEach((runway) => runway.ends.forEach((end) => bounds.extend([end.lon, end.lat]))));
    map.fitBounds(bounds, { padding: 40, duration: 0, maxZoom: 15 });

    const dot = document.createElement("div");
    dot.className = "foqa-map-cursor";
    cursor.current = new Marker({ element: dot }).setLngLat(bounds.getCenter()).addTo(map);
    dot.style.display = "none";

    const track = positions(debrief);
    const labels = [
      ...debrief.airports.flatMap((airport) => airport.runways.flatMap((runway) => runway.ends.map((end) => {
        const el = document.createElement("div");
        el.className = "foqa-runway-label";
        el.textContent = end.runway;
        const back = (end.heading_deg * Math.PI) / 180;
        return new Marker({ element: el }).setLngLat(offset(end, -Math.sin(back) * 90, -Math.cos(back) * 90)).addTo(map);
      }))),
      ...arrows(track, 18).map(({ at, deg }) => new Marker({ element: marker("pointer-events-none text-fg", '<svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><path d="M6 1 L11 10 L6 7.5 L1 10 Z" fill="currentColor" stroke="var(--surface)" stroke-width="1"/></svg>'), rotation: deg, rotationAlignment: "map" }).setLngLat(at).addTo(map)),
      ...debrief.touchdowns.flatMap((td, index) => {
        const p = positionAt(debrief, td.t_s);
        return p ? [new Marker({ element: marker("readout pointer-events-none rounded-sm bg-fg px-1 text-[10px] leading-4 font-semibold text-bg", `TD${index + 1}`), anchor: "bottom", offset: [0, -8] }).setLngLat(p).addTo(map)] : [];
      }),
    ];

    // The time on the track nearest the mouse (projected onto the segment between samples), if it is close enough to mean the track.
    const { t_s } = debrief.series;
    const pick = (x: number, y: number) => {
      let best: number | null = null, bestDist = PICK_PX;
      const px = track.map(({ p }) => map.project(p));
      track.forEach(({ i }, k) => {
        const a = px[k]!, b = track[k + 1]?.i === i + 1 ? px[k + 1]! : a;
        const dx = b.x - a.x, dy = b.y - a.y, len = dx * dx + dy * dy;
        const f = len ? Math.min(1, Math.max(0, ((x - a.x) * dx + (y - a.y) * dy) / len)) : 0;
        const d = Math.hypot(a.x + dx * f - x, a.y + dy * f - y);
        if (d < bestDist) { bestDist = d; best = t_s[i]! + (b === a ? 0 : (t_s[i + 1]! - t_s[i]!) * f); }
      });
      return best;
    };
    map.on("mousemove", (e) => { const t = pick(e.point.x, e.point.y); map.getCanvas().style.cursor = t === null ? "" : "pointer"; bus.emit(t); });
    map.on("mouseout", () => bus.emit(null));
    map.on("moveend", () => { following.current = false; });
    map.on("click", (e) => { const t = pick(e.point.x, e.point.y); if (t !== null) latest.current.onFocus?.(t); });

    let cancelled = false;
    setOffline(false);
    const controller = new AbortController();
    const timer = window.setTimeout(() => controller.abort(), 8000);
    fetch(BASE[theme], { signal: controller.signal })
      .then((response) => (response.ok ? (response.json() as Promise<Style>) : Promise.reject(new Error(String(response.status)))))
      .then((base) => { if (!cancelled) map.setStyle(withOverlay(base)); })
      .catch(() => { if (!cancelled) setOffline(true); })
      .finally(() => window.clearTimeout(timer));

    return () => { cancelled = true; controller.abort(); labels.forEach((m) => m.remove()); map.remove(); mapRef.current = null; cursor.current = null; };
  }, [debrief, theme, hasTrack, bus]);

  // Selected event: its stretch of track is redrawn in the event's colour.
  useEffect(() => {
    const source = mapRef.current?.getSource("foqa-selection") as GeoJSONSource | undefined;
    source?.setData(collection(selectionFeatures(debrief, selected, theme)));
    if (selected) { const p = positionAt(debrief, selected.start_s); if (p) mapRef.current?.easeTo({ center: p, duration: 400 }); }
  }, [selected, debrief, theme]);

  // The aircraft dot: at the hovered time, else at the focused time.
  useEffect(() => {
    const place = (t: number | null) => {
      const marker = cursor.current;
      if (!marker) return;
      const p = t === null ? null : positionAt(debrief, t);
      marker.getElement().style.display = p ? "block" : "none";
      if (p) marker.setLngLat(p);
    };
    place(focus);
    // Keep the focused aircraft in view (playback, stepping, a jump from an event): one smooth pan once it nears the edge.
    // Hover never moves the camera, and neither does this while the user is dragging or zooming; it may retarget its own pan.
    const map = mapRef.current, p = focus === null ? null : positionAt(debrief, focus);
    if (map && p && (following.current || !map.isMoving())) {
      const { x, y } = map.project(p), { clientWidth: w, clientHeight: h } = map.getContainer();
      if (x < FOLLOW_PX || y < FOLLOW_PX || x > w - FOLLOW_PX || y > h - FOLLOW_PX) { map.easeTo({ center: p, duration: 500, easing: easeOut }); following.current = true; }
    }
    return bus.subscribe((t) => place(t ?? latest.current.focus));
  }, [bus, debrief, focus]);

  if (!hasTrack) return <EmptyState size="sm" icon={<MapPinOff size={18} />} title="No position recorded" description="This flight has no latitude/longitude samples to draw." />;
  return (
    <div className="relative h-full">
      <div ref={host} className="h-full w-full" />
      {offline ? (
        <p className="absolute top-2 left-2 flex items-center gap-1.5 rounded-md border border-line bg-raised/90 px-2 py-1 text-xs text-fg-2 shadow-[var(--shadow-resting)]">
          <CloudOff size={13} aria-hidden="true" /> Map tiles unavailable: showing track and runways only
        </p>
      ) : null}
      <ul className="pointer-events-none absolute right-2 bottom-7 grid grid-cols-2 gap-x-3 gap-y-0.5 rounded-md border border-line bg-raised/90 px-2 py-1.5 text-[11px] text-fg-2 shadow-[var(--shadow-resting)]" aria-label="Map legend">
        {phases.map((phase) => (
          <li key={phase} className="flex items-center gap-1.5"><span className="h-[3px] w-3.5 rounded-full" style={{ background: phaseColor(phase, theme) }} aria-hidden="true" />{phaseName(phase)}</li>
        ))}
        <li className="flex items-center gap-1.5"><span className="readout rounded-sm bg-fg px-0.5 text-[9px] leading-3 font-semibold text-bg">TD</span>Touchdown</li>
        <li className="flex items-center gap-1.5"><span className="size-2.5 rounded-full bg-accent" aria-hidden="true" />Aircraft at the cursor</li>
        {selected ? <li className="flex items-center gap-1.5"><span className="h-1.5 w-3.5 rounded-full" style={{ background: levelColor(selected.level, theme) }} aria-hidden="true" />Selected event</li> : null}
      </ul>
      {!debrief.airports.length ? (
        <p className="absolute bottom-8 left-2 rounded-md border border-line bg-raised/90 px-2 py-1 text-xs text-fg-2">No runway data recorded for this flight</p>
      ) : null}
    </div>
  );
}
