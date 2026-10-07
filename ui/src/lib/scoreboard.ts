// The time-synced scoreboard: the session keeps the first full state and then only changes
// (cv_core::scoreboard); here every frame's full state is rebuilt once, and the state at a
// playback time is a binary search (scrubbing never replays the changes).
import type { PlayerState, Scoreboard } from "./types";

export interface Timeline {
  times: number[];
  states: PlayerState[][];
}

const empty = (): PlayerState => ({ level: 0, kills: 0, deaths: 0, assists: 0, cs: 0, items: [], spells: [] });

export function build(sb: Scoreboard): Timeline {
  const times: number[] = [];
  const states: PlayerState[][] = [];
  let cur: PlayerState[] = sb.players.map(empty);
  for (const f of sb.frames) {
    cur = cur.slice();
    for (const d of f.d) {
      const p = cur[d.i];
      if (!p) continue;
      cur[d.i] = {
        level: d.lv ?? p.level,
        kills: d.k ?? p.kills,
        deaths: d.d ?? p.deaths,
        assists: d.a ?? p.assists,
        cs: d.cs ?? p.cs,
        items: d.it ?? p.items,
        spells: d.sp ?? p.spells,
      };
    }
    times.push(f.t);
    states.push(cur);
  }
  return { times, states };
}

/** Index of the frame shown at game time `t` (before the first read: the first). */
export function frameAt(tl: Timeline, t: number): number {
  let lo = 0;
  let hi = tl.times.length - 1;
  if (hi < 0) return -1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (tl.times[mid] <= t) lo = mid;
    else hi = mid - 1;
  }
  return lo;
}

export function stateAt(tl: Timeline, t: number): PlayerState[] | null {
  const i = frameAt(tl, t);
  return i < 0 ? null : tl.states[i];
}

/** "Faker#KR1" -> "Faker". */
export const shortName = (n: string) => n.split("#")[0];
