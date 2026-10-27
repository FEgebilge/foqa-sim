import { useEffect, useState } from "react";
import { Power } from "lucide-react";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { Button } from "@ui/components/button/button";
import { Tooltip } from "@ui/components/tooltip/tooltip";
import { useNotify } from "../lib/context";

/** Start with Windows (a per-user Run entry, off by default); desktop app only. */
export function AutostartToggle() {
  const [on, setOn] = useState<boolean | null>(null);
  const notify = useNotify();
  useEffect(() => { isEnabled().then(setOn, () => setOn(null)); }, []);
  if (on === null) return null;
  const label = `Start with Windows: ${on ? "on" : "off"}`;
  const toggle = () => (on ? disable() : enable()).then(() => setOn(!on),
    (error) => notify({ title: "Could not change Start with Windows", body: String(error), tone: "error" }));

  return (
    <Tooltip content={label} side="bottom">
      <Button variant={on ? "secondary" : "ghost"} size="sm" iconOnly label={label} aria-pressed={on} onClick={toggle}>
        <Power size={16} aria-hidden="true" />
      </Button>
    </Tooltip>
  );
}
