/** Where focus is when a page-wide shortcut would clash with the control's own keys. */
const OVERLAY = "[role='menu'], [role='listbox'], [role='dialog'], dialog[open]";
const TEXT = "input, textarea, select, [contenteditable]:not([contenteditable='false'])";
const CONTROL = `${TEXT}, button, a[href], summary, [role='menuitem'], [role='menuitemcheckbox'], [role='option'], [role='slider'], ${OVERLAY}`;

/** Whether a page-wide shortcut may act on `key` pressed with focus on `target`. Space and the arrows belong to a focused
 * button, link, field, menu or dialog; Escape and the character keys ([ ] J K) only to a field or a menu, so a focused
 * plain button still lets them through. */
export function shortcutAllowed(target: EventTarget | null, key: string): boolean {
  if (!(target instanceof Element)) return true;
  if (target.closest(OVERLAY)) return false;
  return !target.closest(key === "Escape" || (key.length === 1 && key !== " ") ? TEXT : CONTROL);
}
