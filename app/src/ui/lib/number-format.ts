const cache = new Map<string, Intl.NumberFormat>();

/** A cached `Intl.NumberFormat` with Latin digits, so server and client markup agree in every locale. Safe to call every render. */
export function numberFormat(locale: string, options: Intl.NumberFormatOptions = {}): Intl.NumberFormat {
  const key = `${locale}|${JSON.stringify(options)}`;
  let format = cache.get(key);
  if (!format) cache.set(key, format = new Intl.NumberFormat(locale, { numberingSystem: "latn", ...options }));
  return format;
}

export interface NumberSymbols { group: string; decimal: string; minus: string }

/** The group, decimal and minus characters a locale prints, with Latin digits. The minus is "−" (U+2212) in some locales, such as sv. */
export function numberSymbols(locale: string): NumberSymbols {
  const parts = numberFormat(locale, { minimumFractionDigits: 1 }).formatToParts(-1234.5);
  const find = (type: string, fallback: string) => parts.find(part => part.type === type)?.value ?? fallback;
  return { group: find("group", ","), decimal: find("decimal", "."), minus: find("minusSign", "-") };
}

/**
 * Reads a typed number in a locale. The locale's decimal mark is the fraction and its group mark is dropped. A lone
 * "." or "," from the other convention is also read as the fraction unless it looks like a thousands group, so a numpad dot
 * works in Turkish ("1.5" is 1.5, "1.234" is 1234). Returns null when no digit was typed.
 */
export function parseNumber(text: string, { group, decimal, minus }: NumberSymbols): number | null {
  let clean = text.replace(/[\s\u00a0\u202f]/g, "").replace(/[\u2212\u2013]/g, "-").split(minus).join("-");
  const other = decimal === "," ? "." : ",", [before, after, ...rest] = clean.split(other);
  const groupLike = other === group && /^\d{3}(\D|$)/.test(after ?? "");
  if (after !== undefined && !rest.length && !groupLike && !before!.includes(decimal) && !after.includes(decimal)) clean = before + decimal + after;
  clean = clean.split(group).join("");
  clean = clean.replace(decimal, ".").replace(/[^\d.-]/g, "");
  if (!/\d/.test(clean)) return null;
  const parsed = Number(clean);
  return Number.isFinite(parsed) ? parsed : null;
}
