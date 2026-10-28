"use client";

import type { CSSProperties, ReactNode } from "react";
import { CopyButton } from "@ui/components/copy-button/copy-button";
import styles from "./description-list.module.css";

export interface DescriptionItem {
  id: string; label: string; value: ReactNode;
  span?: 1 | 2;                 // across two columns (ignored in one)
  empty?: boolean;              // nothing to show: the muted empty text stands in for `value`
  changed?: boolean;            // edited and unsaved: accent edge plus a screen-reader cue
  copy?: string;                // text a copy button puts on the clipboard
  icon?: ReactNode;             // leads the value
  hint?: string;                // quiet line under the value
}
export interface DescriptionListProps {
  items: DescriptionItem[]; columns?: 1 | 2 | 3; density?: "comfortable" | "compact";
  emptyText?: string; label?: string; className?: string; style?: CSSProperties;
}

export function DescriptionList({ items, columns = 2, density = "comfortable", emptyText = "—", label, className, style }: DescriptionListProps) {
  return <div className={[styles.root, className].filter(Boolean).join(" ")} style={style}>
    <dl className={styles.list} data-columns={columns} data-density={density} aria-label={label}>
      {items.map(item => <div key={item.id} className={styles.item} data-span={item.span} data-changed={item.changed || undefined} data-empty={item.empty || undefined}>
        <dt>{item.label}{item.changed && <span className={styles.srOnly}> (changed)</span>}</dt>
        <dd>
          {item.empty ? <span className={styles.value}><span aria-hidden="true">{emptyText}</span><span className={styles.srOnly}>Not provided</span></span>
            : <>{item.icon && <span className={styles.icon} aria-hidden="true">{item.icon}</span>}<span className={styles.value}>{item.value}</span>
              {item.copy && <CopyButton value={item.copy} label={`Copy ${item.label}`} iconOnly variant="plain" />}</>}
        </dd>
        {item.hint && <dd className={styles.hint}>{item.hint}</dd>}
      </div>)}
    </dl>
  </div>;
}

export default DescriptionList;
