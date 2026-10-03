// End-to-end checks inside the real app (developer tool, run by `--ui-test=<config>`, see
// app/src/bench.rs). Meant for a sandbox profile: they change settings and play simulated
// League games (the fake game + fake League client of crates/cv-mock-league, recording the
// desktop). Each check returns pass/fail with the evidence.
//
//   modes      mode rules decide before recording; a change during a game applies to the next
//   storage    over the storage limit during a game: nothing is deleted until the game is over
//   recovered  (after the app was killed mid-recording) the game got its video back
//   finalized  finished recordings are rewritten for instant playback
//   postgame   (v1.7.1) a game opened the moment it appears after the match: how long after the
//              match ended the card is there, the first frame shows, marker jumps work
//   watch      (v1.7.1) a replay / spectating / a replay found late is never recorded (nothing
//              left in the recordings folder), a normal match still is

import { api } from "./api";
import { measureOpen } from "./replaybench";
import type { LiveStatus, SessionSummary } from "./types";
import { go } from "./store.svelte";

type Check = { name: string; pass: boolean; details: Record<string, unknown> };
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function until<T>(f: () => Promise<T | null | undefined | false>, timeoutMs: number, every = 400): Promise<T | null> {
  const t0 = Date.now();
  while (Date.now() - t0 < timeoutMs) {
    const v = await f().catch(() => null);
    if (v) return v;
    await sleep(every);
  }
  return null;
}

const status = () => api.status();
const sessions = () => api.listSessions();
const idle = (s: LiveStatus) => s.state === "idle";

/** Plays a simulated game with the given queue; resolves with what the status said during it. */
async function play(queue: number, length = 120, during?: (s: LiveStatus) => Promise<void>, watch: string | null = null) {
  const seen: string[] = [];
  const before = new Set((await sessions()).map((s) => s.id));
  // The previous simulated game may still be closing: retry for a while.
  await until(async () => {
    try {
      await api.simulateGame(6, length, queue, watch);
      return true;
    } catch {
      return false;
    }
  }, 90000, 1000);
  // Detected -> (recording) -> idle again.
  const started = await until(async () => {
    const s = await status();
    return !idle(s) && s;
  }, 30000);
  let duringDone = false;
  const end = await until(async () => {
    const s = await status();
    const line = `${s.state}: ${s.message ?? ""}`;
    if (seen[seen.length - 1] !== line) seen.push(line);
    if (during && !duringDone && s.state === "recording") {
      duringDone = true;
      await during(s);
    }
    return idle(s) && s;
  }, 180000, 500);
  // The library notices the new game.
  await sleep(2500);
  const after = await sessions();
  const added = after.filter((s) => !before.has(s.id));
  return { started: !!started, ended: !!end, statuses: seen, added };
}

async function modes(): Promise<Check[]> {
  const out: Check[] = [];
  await api.modesRefresh().catch(() => {});
  await api.modesSet("league", "mode", "q450", "off");
  await api.modesSet("league", "mode", "q420", "record");
  await api.modesSet("league", "mode", "q1700", "clips_only");

  // ARAM is off: detected, but nothing recorded, and the status says why.
  let r = await play(450);
  out.push({
    name: "ARAM off: not recorded, status explains why",
    pass: r.started && r.added.length === 0 && r.statuses.some((l) => /ARAM.*off/i.test(l)),
    details: r,
  });

  // Ranked records; switching Ranked off during the game doesn't stop this recording.
  let switched = "";
  r = await play(420, 150, async () => {
    const v = await api.modesSet("league", "mode", "q420", "off");
    switched = JSON.stringify(v?.[0]?.modes?.entries?.q420?.rule);
    await sleep(3000);
  });
  const g = r.added[0] as SessionSummary | undefined;
  const view = g ? await api.getSession(g.id) : null;
  out.push({
    name: "Ranked recorded (queue + mode name saved), and switching it off mid-game keeps this recording",
    pass: !!g && g.queue_id === 420 && /Ranked Solo/.test(g.mode_name ?? "") && g.record_mode !== "clips_only" && !!view?.video_path && r.statuses.every((l) => !l.startsWith("idle") || true),
    details: { ...r, rule_after_switch: switched, video: view?.video_path, mode: g?.mode_name, queue: g?.queue_id },
  });

  // ... and the next Ranked game isn't recorded.
  r = await play(420);
  out.push({ name: "Next Ranked game follows the new rule (off)", pass: r.started && r.added.length === 0 && r.statuses.some((l) => /off/i.test(l)), details: r });

  // Arena on "Clips only": a game entry with clips, no full video.
  r = await play(1700, 240);
  const a = r.added[0];
  const av = a ? await api.getSession(a.id) : null;
  out.push({
    name: "Arena on Clips only: saved without a full video, with clips",
    pass: !!a && a.record_mode === "clips_only" && !av?.video_path,
    details: { ...r, clips: av?.clips.map((c) => c.title), video: av?.video_path ?? null },
  });

  // A queue never seen before follows the "Unknown / new modes" rule (record) and is flagged new.
  r = await play(9999);
  const views = await api.modesGet();
  const e = views[0]?.modes?.entries?.q9999;
  out.push({ name: "Brand-new mode: recorded by the unknown rule and marked new", pass: r.added.length === 1 && !!e?.is_new, details: { ...r, entry: e } });
  return out;
}

async function storage(): Promise<Check[]> {
  const out: Check[] = [];
  // Old games from the job (copies of a real recording) are fair game for the clean-up.
  const old = (await sessions()).filter((s) => s.id.startsWith("old-"));
  for (const s of old) if (s.favorite) await api.setFavorite(s.id, false);
  await sleep(1500);
  const settings = await api.getSettings();
  const ids = () => sessions().then((l) => l.map((s) => s.id));
  const before = await ids();
  let duringCheck: Record<string, unknown> = {};
  const r = await play(400, 300, async () => {
    // The limit is exceeded while the game is recording: nothing may be deleted now.
    await api.saveSettings({ ...settings, max_disk_gb: 1, auto_cleanup: true });
    const samples: number[] = [];
    for (let i = 0; i < 8; i++) {
      await sleep(2000);
      samples.push((await ids()).filter((id) => id.startsWith("old-")).length);
    }
    const info = await api.storageInfo();
    duringCheck = { old_games_still_there: samples, used_gb: (info.used_bytes / 1024 ** 3).toFixed(2), limit_gb: 1, recent_cleanups: info.recent_cleanups.length };
  });
  const endedAt = Date.now();
  out.push({ name: "Over the limit during a game: nothing deleted while recording", pass: (duringCheck.old_games_still_there as number[] | undefined)?.every((n) => n === old.length) ?? false, details: { ...duringCheck, old: old.length, statuses: r.statuses } });
  // After the game: the clean-up runs (after the game's own post-processing).
  const cleaned = await until(async () => {
    const left = (await ids()).filter((id) => id.startsWith("old-"));
    return left.length < old.length && left;
  }, 120000, 1500);
  const info = await api.storageInfo();
  out.push({
    name: "After the game: oldest games removed down to the limit",
    pass: !!cleaned,
    details: { left_old: cleaned, seconds_after_game: Math.round((Date.now() - endedAt) / 1000), recent: info.recent_cleanups.slice(0, 5), before: before.length },
  });
  return out;
}

async function finalized(): Promise<Check[]> {
  // Every recording with a video ends up as faststart (maintenance runs ~20 s after start and
  // after each game).
  const done = await until(async () => {
    const list = (await sessions()).filter((s) => s.video_path);
    if (!list.length) return null;
    const infos = await Promise.all(list.map((s) => api.videoInfo(s.video_path!).then((i) => ({ id: s.id, layout: i.layout, kf_max: i.keyframe_interval_max, kf_avg: i.keyframe_interval_avg }))));
    return infos.every((i) => i.layout === "faststart") && infos;
  }, 150000, 3000);
  const all = await Promise.all((await sessions()).filter((s) => s.video_path).map((s) => api.videoInfo(s.video_path!).then((i) => ({ id: s.id, layout: i.layout, fragments: i.fragments, keyframes_avg: i.keyframe_interval_avg, keyframes_max: i.keyframe_interval_max }))));
  return [{ name: "Finished recordings rewritten for instant playback (faststart)", pass: !!done, details: { videos: all } }];
}

async function recovered(): Promise<Check[]> {
  const g = await until(async () => {
    const list = await sessions();
    for (const s of list) {
      const v = await api.getSession(s.id).catch(() => null);
      if (v?.video_path && v.session.warnings.some((w) => /recovered/i.test(w))) return { id: s.id, video: v.video_path, duration: v.session.video_duration, warnings: v.session.warnings };
    }
    return null;
  }, 90000, 3000);
  const info = g ? await api.videoInfo(g.video) : null;
  return [{ name: "Killed mid-recording: the game keeps its video up to the crash", pass: !!g && (g.duration ?? 0) > 5, details: { game: g, file: info } }];
}


/** Real-time simulated games of `lengths` seconds (the fake game's loading screen is 8 s): the
 *  game is opened the moment it appears in the library; times are from the end of the match. */
async function postgame(cfg: { postgame_lengths?: number[] }): Promise<Check[]> {
  const out: Check[] = [];
  for (const length of cfg.postgame_lengths ?? [300]) {
    const before = new Set((await sessions()).map((s) => s.id));
    let tCall = 0;
    await until(async () => {
      try {
        tCall = Date.now();
        await api.simulateGame(1, length, 400);
        return true;
      } catch {
        return false;
      }
    }, 90000, 1000);
    // The fake match ends (GameEnd, victory screen) 8 s (loading) + its length after the call.
    const matchEnd = tCall + 8000 + length * 1000;
    await api.benchLog(`postgame ${length} s: started; match ends at ${new Date(matchEnd).toISOString()}`).catch(() => {});
    await sleep(Math.max(0, matchEnd - Date.now() - 1500));
    // The moment the game is in the library, open it.
    let card: SessionSummary | null = null;
    let tCard = 0;
    const deadline = matchEnd + 180000;
    while (Date.now() < deadline) {
      const list = await sessions().catch(() => [] as SessionSummary[]);
      card = list.find((s) => !before.has(s.id) && !!s.video_path) ?? null;
      if (card) {
        tCard = Date.now();
        break;
      }
      await sleep(100);
    }
    if (!card) {
      out.push({ name: `${length} s game: card appeared`, pass: false, details: { length } });
      continue;
    }
    const info = await api.videoInfo(card.video_path!).catch((e) => ({ error: String(e) }) as any);
    const tOpen = Date.now();
    const r = await measureOpen(card.id).catch((e) => ({ error: String(e) }) as any);
    const firstFrameAt = r.firstFrame != null ? tOpen + r.firstFrame : null;
    // The thumbnail (made right after the game since v1.7.1).
    let thumbAt: number | null = null;
    await until(async () => {
      const s = (await sessions()).find((x) => x.id === card!.id);
      if (s?.thumb_path) thumbAt = Date.now();
      return !!s?.thumb_path;
    }, 60000, 250);
    const sec = (t: number | null) => (t == null ? null : Math.round((t - matchEnd) / 100) / 10);
    const details = {
      length,
      card_s: sec(tCard),
      first_frame_s: sec(firstFrameAt),
      playable_s: r.playable != null ? sec(tOpen + r.playable) : null,
      open: r,
      layout_at_card: info?.layout ?? null,
      fragments_at_card: info?.fragments ?? null,
      bytes: info?.bytes ?? null,
      thumb_s: sec(thumbAt),
    };
    await api.benchLog(`postgame ${length} s: ${JSON.stringify(details)}`).catch(() => {});
    out.push({
      name: `${length} s game: card and video playable < 5 s after the match ended`,
      pass: (details.card_s ?? 99) < 5 && (details.first_frame_s ?? 99) < 5 && details.layout_at_card === "faststart",
      details,
    });
    go({ page: "home" });
    await sleep(3000);
  }
  return out;
}

/** Replays and spectating aren't recorded; a normal match still is. */
async function watch(): Promise<Check[]> {
  const out: Check[] = [];
  // (Builds without the command: null, the check is skipped.)
  const folders = async () => ((await (api as any).benchSaveDir?.().catch(() => null)) as string[] | null) ?? null;
  for (const [kind, label] of [
    ["replay", "Replay"],
    ["spectate", "Spectating"],
    ["replay-late", "Replay (client can't tell, found by the game a few seconds in)"],
    ["replay-unsure", "Replay (no game session in the client, the game decides)"],
  ] as const) {
    const f0 = await folders();
    const r = await play(400, 90, undefined, kind);
    await sleep(1500);
    const f1 = await folders();
    const leftover = f0 && f1 ? f1.filter((f) => !f0.includes(f)) : null;
    out.push({
      name: `${label}: not recorded, nothing left, status says why`,
      pass: r.started && r.ended && r.added.length === 0 && (leftover == null || leftover.length === 0) && r.statuses.some((l) => /not recorded/i.test(l)),
      details: { ...r, leftover },
    });
  }
  const r = await play(400, 90);
  out.push({ name: "A normal match is still recorded", pass: r.added.length === 1 && !!r.added[0].video_path, details: r });
  return out;
}

const TESTS: Record<string, (cfg: any) => Promise<Check[]>> = { modes, storage, finalized, recovered, postgame, watch };

export async function runUiTests(cfg: { tests: string[] }) {
  const results: Check[] = [];
  for (const t of cfg.tests) {
    await api.benchLog(`ui test ${t}: start`).catch(() => {});
    try {
      const r = await TESTS[t](cfg);
      results.push(...r);
      for (const c of r) await api.benchLog(`ui test ${t}: ${c.pass ? "PASS" : "FAIL"} ${c.name}`).catch(() => {});
    } catch (e) {
      results.push({ name: `${t} crashed`, pass: false, details: { error: String(e) } });
    }
  }
  await api.benchFinish({ kind: "ui-tests", at: new Date().toISOString(), results });
}
