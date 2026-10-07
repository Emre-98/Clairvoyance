// Which event groups the timeline and the event list show, remembered between replays (this PC
// only). What's *shown* is stored, so a group added in a later version starts hidden.

import { GROUPS, SHOWN_BY_DEFAULT, type Group } from "./eventmeta";

const KEY = "cv.timelineFilters";

/** The hidden groups: everything but the remembered shown ones (kills, deaths, assists at first). */
export function loadHidden(): Set<Group> {
  let shown: Group[] = SHOWN_BY_DEFAULT;
  try {
    const raw = localStorage.getItem(KEY);
    const v = raw ? JSON.parse(raw) : null;
    if (v && v.v === 1 && Array.isArray(v.shown)) shown = v.shown;
  } catch {}
  return new Set(GROUPS.map((g) => g.id).filter((g) => !shown.includes(g)));
}

export function saveHidden(hidden: Set<Group>) {
  try {
    const shown = GROUPS.map((g) => g.id).filter((g) => !hidden.has(g));
    localStorage.setItem(KEY, JSON.stringify({ v: 1, shown }));
  } catch {}
}
