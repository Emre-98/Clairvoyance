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

export function relativeDate(iso: string): string {
  const d = new Date(iso);
  const now = new Date();
  const days = Math.floor((startOfDay(now) - startOfDay(d)) / 86400000);
  const time = d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  if (days === 0) return `Today, ${time}`;
  if (days === 1) return `Yesterday, ${time}`;
  if (days < 7) return `${d.toLocaleDateString(undefined, { weekday: "long" })}, ${time}`;
  return d.toLocaleDateString(undefined, { day: "numeric", month: "short", year: d.getFullYear() === now.getFullYear() ? undefined : "numeric" });
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

export function num(n: number | null | undefined, digits = 0): string {
  if (n == null) return "-";
  return n.toLocaleString(undefined, { maximumFractionDigits: digits, minimumFractionDigits: digits });
}
