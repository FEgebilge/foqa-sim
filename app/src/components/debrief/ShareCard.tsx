import { useEffect, useMemo, useRef, useState } from "react";
import { Copy, Download } from "lucide-react";
import { Button } from "@ui/components/button/button";
import SegmentedControl from "@ui/components/segmented-control/segmented-control";
import { api } from "../../api";
import { useNotify, useTheme } from "../../lib/context";
import { groupLandings } from "../../lib/landings";
import { LOGO } from "../../lib/logo";
import { levelColor, type Theme } from "../../lib/palette";
import { CARD, cardData, cardFileName, type CardData } from "../../lib/share-card";
import type { Debrief } from "../../types";

interface Props {
  debrief: Debrief;
  /** The landing to show; null keeps the dialog closed. */
  landing: number | null;
  onLanding: (index: number) => void;
  onClose: () => void;
}

/** The landing card: a preview drawn on a canvas in the app's theme, copied to the clipboard or saved as a PNG. Nothing is uploaded. */
export function ShareCard({ debrief, landing, onLanding, onClose }: Props) {
  const ref = useRef<HTMLDialogElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const { theme, setTheme } = useTheme();
  const notify = useNotify();
  const [busy, setBusy] = useState<"copy" | "save" | null>(null);
  const count = useMemo(() => groupLandings(debrief.touchdowns).length, [debrief]);
  const data = useMemo(() => (landing === null ? null : cardData(debrief, landing)), [debrief, landing]);

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog || (landing !== null) === dialog.open) return;
    if (landing !== null) dialog.showModal();
    else dialog.close();
  }, [landing]);

  useEffect(() => {
    if (data && canvas.current) void drawCard(canvas.current, data, theme);
  }, [data, theme]);

  const png = () => new Promise<Blob>((resolve, reject) => canvas.current?.toBlob((b) => (b ? resolve(b) : reject(new Error("The card could not be drawn."))), "image/png"));
  const run = (kind: "copy" | "save", action: () => Promise<string | null | void>, done: (result: string | null | void) => string | null) => {
    setBusy(kind);
    action()
      .then((result) => { const title = done(result); if (title) notify({ title }); })
      .catch((error: unknown) => notify({ title: kind === "copy" ? "Could not copy the card" : "Could not save the card", body: String(error), tone: "error" }))
      .finally(() => setBusy(null));
  };
  const copy = () => run("copy", () => navigator.clipboard.write([new ClipboardItem({ "image/png": png() })]), () => "Landing card copied");
  const save = () => run("save", async () => api.saveImage(await png(), cardFileName(debrief, landing ?? 0)), (file) => (file ? `Saved ${file}` : null));

  return (
    <dialog ref={ref} onCancel={(event) => { event.preventDefault(); onClose(); }} aria-labelledby="share-title"
      className="panel m-auto w-[min(56rem,calc(100vw-2rem))] p-5 text-fg backdrop:bg-black/50">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h2 id="share-title" className="font-display text-base font-semibold">Landing card</h2>
          <p className="text-xs text-fg-3">An image of this landing to share. It names the aircraft type only and is made on this PC; nothing is uploaded.</p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          {count > 1 ? (
            <SegmentedControl label="Landing" options={Array.from({ length: count }, (_, i) => ({ value: String(i), label: `Landing ${i + 1}` }))}
              value={String(landing ?? 0)} onValueChange={(v) => onLanding(Number(v))} />
          ) : null}
          <SegmentedControl label="Theme" options={[{ value: "light", label: "Light" }, { value: "dark", label: "Dark" }]} value={theme} onValueChange={(v) => setTheme(v as Theme)} />
        </div>
      </div>
      <canvas ref={canvas} role="img" aria-label={data ? describe(data) : "Landing card"} className="mt-4 block aspect-[1200/630] w-full rounded-[var(--radius-control)] border border-line-subtle" />
      <div className="mt-4 flex flex-wrap justify-end gap-2">
        <Button variant="secondary" size="sm" onClick={onClose}>Close</Button>
        <Button variant="secondary" size="sm" onClick={copy} loading={busy === "copy"} disabled={!data}><Copy size={15} aria-hidden="true" />Copy image</Button>
        <Button size="sm" onClick={save} loading={busy === "save"} disabled={!data}><Download size={15} aria-hidden="true" />Save as…</Button>
      </div>
    </dialog>
  );
}

/** The card in words, for screen readers. */
const describe = (d: CardData) => [d.landing, d.route, d.runway, d.aircraft, d.date, d.score ? `score ${d.score} of 100, ${d.gradeLabel}` : d.note, ...d.stats.map((s) => `${s.label} ${s.value}`)].filter(Boolean).join(", ");

/** A CSS value of the current theme, resolved (a token's `var()` chain followed) by a probe element. */
function resolve(property: "color" | "fontFamily", value: string) {
  const probe = document.createElement("span");
  probe.style[property] = value;
  document.body.append(probe);
  const out = getComputedStyle(probe)[property];
  probe.remove();
  return out;
}

const PAD = 56;

/** Draws the card at `CARD.scale`× in the theme's tokens: header, score, the six domain bars, the touchdown values, the mark. */
async function drawCard(canvas: HTMLCanvasElement, d: CardData, theme: Theme) {
  const color = (token: string) => resolve("color", `var(${token})`);
  const c = {
    bg: color("--surface"), fg: color("--foreground"), fg2: color("--text-secondary"), fg3: color("--text-muted"),
    line: color("--border"), track: color("--surface-muted"), accent: color("--accent"),
    grade: d.grade === "good" ? color("--success") : d.grade === "fair" ? color("--warning") : color("--danger"),
  };
  const mono = resolve("fontFamily", "var(--font-display)"), sans = resolve("fontFamily", "var(--font-body)");
  await Promise.all([`600 64px ${mono}`, `400 20px ${mono}`, `500 20px ${sans}`, `400 20px ${sans}`].map((f) => document.fonts.load(f)));

  const { width: W, height: H, scale } = CARD;
  canvas.width = W * scale;
  canvas.height = H * scale;
  const g = canvas.getContext("2d");
  if (!g) return;
  g.setTransform(scale, 0, 0, scale, 0, 0);
  const text = (s: string, x: number, y: number, font: string, fill: string, align: CanvasTextAlign = "left", max?: number) => {
    g.font = font;
    g.fillStyle = fill;
    g.textAlign = align;
    g.fillText(s, x, y, max);
    return g.measureText(s).width;
  };
  const pill = (x: number, y: number, w: number, h: number, fill: string, alpha = 1) => {
    g.globalAlpha = alpha;
    g.fillStyle = fill;
    g.beginPath();
    g.roundRect(x, y, w, h, h / 2);
    g.fill();
    g.globalAlpha = 1;
  };

  g.fillStyle = c.bg;
  g.fillRect(0, 0, W, H);
  g.fillStyle = c.accent;
  g.fillRect(0, 0, W, 6);

  // Header: what and where, and the score.
  text(`${d.landing} · ${d.date}`.toUpperCase(), PAD, 78, `500 18px ${mono}`, c.fg3);
  text(d.route, PAD, 146, `600 60px ${mono}`, c.fg, "left", 700);
  text([d.aircraft, d.runway].filter(Boolean).join(" · "), PAD, 188, `400 24px ${sans}`, c.fg2);
  if (d.score && d.gradeLabel) {
    const out = text("/100", W - PAD, 146, `400 26px ${mono}`, c.fg3, "right");
    text(d.score, W - PAD - out - 8, 146, `600 96px ${mono}`, c.fg, "right");
    g.font = `600 18px ${sans}`;
    const w = g.measureText(d.gradeLabel).width + 32;
    pill(W - PAD - w, 166, w, 34, c.grade, theme === "dark" ? 0.22 : 0.14);
    text(d.gradeLabel, W - PAD - w / 2, 189, `600 18px ${sans}`, c.grade, "center");
  }

  // The six domains, two columns of three.
  const top = 250, colW = (W - 2 * PAD - 48) / 2;
  if (d.bars.length) {
    d.bars.forEach((bar, i) => {
      const x = PAD + (i % 2) * (colW + 48), y = top + Math.floor(i / 2) * 54;
      text(bar.label, x, y, `500 19px ${sans}`, c.fg2);
      text(bar.text, x + colW, y, `400 18px ${mono}`, bar.kept === null ? c.fg3 : c.fg, "right");
      pill(x, y + 12, colW, 10, c.track);
      if (bar.kept !== null) pill(x, y + 12, Math.max(10, colW * bar.kept), 10, bar.level ? levelColor(LEVEL[bar.level - 1]!, theme) : resolve("color", "var(--success)"));
    });
  } else {
    text(d.note ?? "", PAD, top + 40, `400 22px ${sans}`, c.fg2, "left", W - 2 * PAD);
  }

  // Touchdown values in tiles.
  const statsTop = 430;
  g.fillStyle = c.line;
  g.fillRect(PAD, statsTop - 22, W - 2 * PAD, 1);
  const tile = (W - 2 * PAD) / Math.max(1, d.stats.length);
  d.stats.forEach((s, i) => {
    const x = PAD + i * tile;
    text(s.label.toUpperCase(), x, statsTop + 8, `500 15px ${mono}`, c.fg3);
    text(s.value, x, statsTop + 50, `600 32px ${mono}`, c.fg, "left", tile - 16);
    if (s.sub) text(s.sub, x, statsTop + 78, `400 16px ${sans}`, c.fg3, "left", tile - 16);
  });

  // The mark.
  const size = 30, mx = PAD, my = H - PAD - size + 6;
  g.save();
  g.translate(mx, my);
  g.scale(size / LOGO.size, size / LOGO.size);
  g.lineCap = "round";
  g.lineJoin = "round";
  g.strokeStyle = c.accent;
  g.lineWidth = 1.5;
  g.beginPath();
  g.arc(LOGO.ring.cx, LOGO.ring.cy, LOGO.ring.r, 0, Math.PI * 2);
  g.stroke();
  g.strokeStyle = c.fg3;
  g.lineWidth = 1.2;
  g.stroke(new Path2D(LOGO.horizon));
  g.strokeStyle = c.fg;
  g.lineWidth = 1.8;
  g.stroke(new Path2D(LOGO.symbol));
  g.restore();
  const name = text("FOQA-Sim", mx + size + 12, my + 22, `600 22px ${mono}`, c.fg);
  text("Flight data monitoring for MSFS", mx + size + 12 + name + 14, my + 22, `400 17px ${sans}`, c.fg3);
}

const LEVEL = ["low", "medium", "high"] as const;
