import { useEffect, useState } from "react";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { Switch } from "@ui/components/switch/switch";
import { useNotify } from "../lib/context";

/** Start with Windows (a per-user Run entry, off by default); desktop app only. A row of the settings dialog. */
export function AutostartToggle() {
  const [on, setOn] = useState<boolean | null>(null);
  const notify = useNotify();
  useEffect(() => { isEnabled().then(setOn, () => setOn(null)); }, []);
  if (on === null) return null;
  const toggle = () => (on ? disable() : enable()).then(() => setOn(!on),
    (error) => notify({ title: "Could not change Start with Windows", body: String(error), tone: "error" }));

  return (
    <div className="flex items-center justify-between gap-4">
      <div>
        <p className="text-sm font-medium" id="autostart-label">Start with Windows</p>
        <p className="text-xs text-fg-3">Opens FOQA-Sim, minimised, when you sign in, so the next flight is recorded without launching it.</p>
      </div>
      <Switch checked={on} onCheckedChange={() => void toggle()} aria-labelledby="autostart-label" />
    </div>
  );
}
