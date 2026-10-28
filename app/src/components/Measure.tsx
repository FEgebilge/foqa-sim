import type { ReactNode } from "react";
import { Tooltip } from "@ui/components/tooltip/tooltip";

/** A measured value, or "–" with a "not recorded" tooltip when null: a missing measure never disappears, and never reads as zero. */
export function Measure({ value, missing = "not recorded", children }: { value: unknown; missing?: string; children: ReactNode }) {
  if (value === null || value === undefined) return <Tooltip content={missing}><span className="cursor-help text-fg-3" tabIndex={0} aria-label={missing}>–</span></Tooltip>;
  return <>{children}</>;
}
