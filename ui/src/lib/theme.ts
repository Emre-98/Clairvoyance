// Theme switching: Dark, Light or "Match Windows" (follows the OS, live).
// All colours are CSS custom properties (app.css) that use light-dark(), so switching is just
// one attribute on <html>: instant, no reload, no re-render.
import { api, isTauri } from "./api";

export type Theme = "system" | "dark" | "light";

const media = typeof window !== "undefined" ? window.matchMedia("(prefers-color-scheme: dark)") : null;

export function currentTheme(): Theme {
  const t = document.documentElement.getAttribute("data-theme");
  return t === "dark" || t === "light" ? t : "system";
}

/** Whether the page is showing dark colours right now. */
export function isDark(theme: Theme = currentTheme()): boolean {
  return theme === "dark" || (theme === "system" && (media?.matches ?? true));
}

function paint(theme: Theme) {
  const root = document.documentElement;
  if (root.getAttribute("data-theme") === theme) return;
  // Flip every colour in the same frame (no per-element colour transitions).
  root.classList.add("theme-switching");
  root.setAttribute("data-theme", theme);
  requestAnimationFrame(() => requestAnimationFrame(() => root.classList.remove("theme-switching")));
}

/** Applies a theme at once and remembers it. */
export function setTheme(theme: Theme) {
  paint(theme);
  try {
    localStorage.setItem("cv-theme", theme);
  } catch {}
  if (isTauri) api.setTheme(theme, isDark(theme)).catch(() => {});
}

/** Keeps the page (and the native window background) in step with the settings and Windows. */
export function initTheme(fromSettings: string | undefined) {
  const t: Theme = fromSettings === "dark" || fromSettings === "light" ? fromSettings : "system";
  paint(t);
  try {
    localStorage.setItem("cv-theme", t);
  } catch {}
  if (isTauri) api.setTheme(t, isDark(t)).catch(() => {});
  // "Match Windows": CSS follows the OS by itself; only the native background needs a nudge.
  media?.addEventListener("change", () => {
    const cur = currentTheme();
    if (cur === "system" && isTauri) api.setTheme(cur, isDark(cur)).catch(() => {});
  });
}
