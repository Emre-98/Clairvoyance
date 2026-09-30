export function clock(secs: number | null | undefined): string {
  if (secs == null || !isFinite(secs)) return "--:--";
  const s = Math.max(0, Math.round(secs));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const ss = String(s % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${ss}` : `${m}:${ss}`;
}

export function bytes(n: number | null | undefined): string {
  if (n == null) return "?";
  const u = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  let v = n;
  while (v >= 1024 && i < u.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 100 || i === 0 ? v.toFixed(0) : v.toFixed(1)} ${u[i]}`;
}

// Intl formatters are expensive to create: build them once (this runs for every card).
const fmtTime = new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit" });
const fmtWeekday = new Intl.DateTimeFormat(undefined, { weekday: "long" });
const fmtDayMonth = new Intl.DateTimeFormat(undefined, { day: "numeric", month: "short" });
const fmtDayMonthYear = new Intl.DateTimeFormat(undefined, { day: "numeric", month: "short", year: "numeric" });
const relCache = new Map<string, { at: number; text: string }>();

export function relativeDate(iso: string): string {
  const today = startOfDay(new Date());
  const hit = relCache.get(iso);
  if (hit && hit.at === today) return hit.text;
  const d = new Date(iso);
  const days = Math.floor((today - startOfDay(d)) / 86400000);
  const time = fmtTime.format(d);
  let text: string;
  if (days === 0) text = `Today, ${time}`;
  else if (days === 1) text = `Yesterday, ${time}`;
  else if (days < 7) text = `${fmtWeekday.format(d)}, ${time}`;
  else text = (d.getFullYear() === new Date().getFullYear() ? fmtDayMonth : fmtDayMonthYear).format(d);
  if (relCache.size > 5000) relCache.clear();
  relCache.set(iso, { at: today, text });
  return text;
}

function startOfDay(d: Date) {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

export function kda(s: { kills: number; deaths: number; assists: number } | null | undefined): string {
  if (!s) return "-";
  return `${s.kills} / ${s.deaths} / ${s.assists}`;
}

export function kdaRatio(s: { kills: number; deaths: number; assists: number } | null | undefined): string {
  if (!s) return "-";
  if (s.deaths === 0) return s.kills + s.assists > 0 ? "Perfect" : "0.00";
  return ((s.kills + s.assists) / s.deaths).toFixed(2);
}

const numFmts = new Map<number, Intl.NumberFormat>();
export function num(n: number | null | undefined, digits = 0): string {
  if (n == null) return "-";
  let f = numFmts.get(digits);
  if (!f) numFmts.set(digits, (f = new Intl.NumberFormat(undefined, { maximumFractionDigits: digits, minimumFractionDigits: digits })));
  return f.format(n);
}
