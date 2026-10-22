import i18n from "../i18n";

/** `mm:ss` or `h:mm:ss` from milliseconds. */
export function clock(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${pad(h)}:${pad(m)}:${pad(s)}` : `${pad(m)}:${pad(s)}`;
}

/** Seconds with one decimal, formatted for the current language. */
export function seconds(ms: number): string {
  return new Intl.NumberFormat(i18n.language, { minimumFractionDigits: 1, maximumFractionDigits: 1 }).format(ms / 1000);
}

/** Milliseconds as a whole number. */
export function millis(ms: number): string {
  return new Intl.NumberFormat(i18n.language, { maximumFractionDigits: 0 }).format(ms);
}

/** US dollars from micro-dollars, with more precision for small amounts. */
export function usd(micros: number | null | undefined): string {
  if (micros === null || micros === undefined) return "—";
  const value = micros / 1_000_000;
  const digits = value !== 0 && value < 0.1 ? 4 : 2;
  return new Intl.NumberFormat(i18n.language, {
    style: "currency",
    currency: "USD",
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  }).format(value);
}

export function price(value: number | null | undefined): string {
  if (value === null || value === undefined) return "—";
  return new Intl.NumberFormat(i18n.language, {
    style: "currency",
    currency: "USD",
    maximumFractionDigits: 4,
  }).format(value);
}

export function date(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return new Intl.DateTimeFormat(i18n.language, { dateStyle: "medium", timeStyle: "short" }).format(d);
}

/** Splits `**bold**` markup into plain and bold segments. */
export function boldSegments(text: string): { text: string; bold: boolean }[] {
  const out: { text: string; bold: boolean }[] = [];
  const re = /\*\*(.+?)\*\*/g;
  let last = 0;
  for (let m = re.exec(text); m; m = re.exec(text)) {
    if (m.index > last) out.push({ text: text.slice(last, m.index), bold: false });
    out.push({ text: m[1], bold: true });
    last = m.index + m[0].length;
  }
  if (last < text.length) out.push({ text: text.slice(last), bold: false });
  return out;
}
