import { useState } from "react";
import { Settings } from "lucide-react";
import { Button } from "@ui/components/button/button";
import { Dialog, DialogContent } from "@ui/components/dialog/dialog";
import SegmentedControl from "@ui/components/segmented-control/segmented-control";
import { Select } from "@ui/components/select/select";
import { Tooltip } from "@ui/components/tooltip/tooltip";
import { api } from "../api";
import { useTheme } from "../lib/context";
import { number } from "../lib/format";
import { setPrefs, useUnits, type Prefs } from "../lib/prefs";
import { AutostartToggle } from "./AutostartToggle";

const LOCALES = [
  { value: "system", label: "Same as the system" },
  { value: "en-GB", label: "English (UK): 28 Oct 2026, 1,234.5" },
  { value: "en-US", label: "English (US): Oct 28, 2026, 1,234.5" },
  { value: "de-DE", label: "Deutsch: 28. Okt. 2026, 1.234,5" },
  { value: "fr-FR", label: "Français: 28 oct. 2026, 1 234,5" },
  { value: "tr-TR", label: "Türkçe: 28 Eki 2026, 1.234,5" },
];

/** A row with its own label above the options, so each preference reads on its own. */
function Row<K extends "fuel" | "distance" | "time">({ name, label, hint, options }: { name: K; label: string; hint: string; options: { value: Prefs[K]; label: string }[] }) {
  const prefs = useUnits();
  return (
    <div className="flex flex-col gap-1.5">
      <div>
        <p className="text-sm font-medium">{label}</p>
        <p className="text-xs text-fg-3">{hint}</p>
      </div>
      <SegmentedControl label={label} options={options} value={prefs[name]} onValueChange={(value) => setPrefs({ [name]: value })} />
    </div>
  );
}

/** The gear in the header and the dialog it opens: units, time zone, language of dates and numbers, theme, autostart. All display only. */
export function SettingsDialog() {
  const [open, setOpen] = useState(false);
  const { theme, setTheme } = useTheme();
  const prefs = useUnits();
  return (
    <>
      <Tooltip content="Settings" side="bottom">
        <Button variant="ghost" size="sm" iconOnly label="Settings" aria-haspopup="dialog" onClick={() => setOpen(true)}>
          <Settings size={16} aria-hidden="true" />
        </Button>
      </Tooltip>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent title="Settings" description="How FOQA-Sim shows numbers and times. Recordings and exports keep the recorded values (metres, kilograms, UTC)." size="md">
          <div className="flex flex-col gap-5">
            <Row name="fuel" label="Fuel and weight" hint="Fuel used, burn rate and taxi fuel." options={[{ value: "kg", label: "Kilograms" }, { value: "lb", label: "Pounds" }]} />
            <Row name="distance" label="Runway distances" hint="Touchdown point, distance left, offset from the centreline." options={[{ value: "m", label: "Metres" }, { value: "ft", label: "Feet" }]} />
            <Row name="time" label="Time of day" hint="Flight dates and start times. Flight clocks (T+) are not affected." options={[{ value: "utc", label: "UTC" }, { value: "local", label: "This computer's time zone" }]} />
            <p className="text-xs text-fg-3">Altitude stays in feet and speed in knots, the aviation standard everywhere. Example now: {number(1234.5, 1)}.</p>
            <Select label="Dates and numbers" value={LOCALES.some((l) => l.value === prefs.locale) ? prefs.locale : "system"} options={LOCALES} onValueChange={(locale) => setPrefs({ locale })} />
            <div className="flex flex-col gap-1.5">
              <p className="text-sm font-medium">Appearance</p>
              <SegmentedControl label="Theme" options={[{ value: "dark", label: "Dark (night cockpit)" }, { value: "light", label: "Light" }]} value={theme} onValueChange={(next) => setTheme(next as "dark" | "light")} />
            </div>
            {api.live ? <AutostartToggle /> : null}
          </div>
        </DialogContent>
      </Dialog>
    </>
  );
}
