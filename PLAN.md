# Clairvoyance (formerly GameRecorder): Project Plan

> **For Claude:** read this whole file at the start of every session. The design log with every
> decision, measurement and test is in **docs/DECISIONS.md**: read the sections for the parts
> you're changing (section names quoted below, e.g. "Instant replays", are sections there).
> After finishing a milestone, tick it off below, add the decision and its numbers to
> docs/DECISIONS.md, and update "Current status" here (keep it short).

## Current status
- **Released: v1.7.1** (2026-10-03), published at https://github.com/Emre-98/Clairvoyance
  (public). GitHub Actions builds releases; installed copies update themselves ("Updates and
  releases"). The owner's check of v1.7.1 passed.
- **Stack: Tauri 2 (Rust + Svelte 5 web UI)** with a built-in recorder (Windows Graphics Capture +
  hardware H.264 via Media Foundation + per-process game audio, crash-safe fragmented MP4 that is
  indexed in place when it stops, in-memory replay buffer). Workspace layout: see "Architecture".
- **Done (milestones 0-31):** League + CS2, event timeline, clips, game modes, instant replays,
  ult tracking with a post-game check, input overlay + ability bubbles + Mechanics, fullscreen
  player, timeline zoom, frame stepping, every window size, ready right after the game,
  replays / spectating not recorded. In-game cost measured on the owner's PC: -0.07 % FPS,
  ~0.5 % CPU. Per-release details: docs/DECISIONS.md, "Status log".
- **Engineering upkeep (2026-10-07, after a full review of the code):** the architecture is
  sound and the project continues as is (no rewrite). Done: CI compiles and lints the Windows
  code (new `windows` job) and checks rustfmt + clippy; engine session states made explicit
  (`Stage`); `GameIntegration` split into optional capability traits; open / reveal limited to
  the app's own files. No behaviour change for the owner. See "Engineering upkeep" in
  docs/DECISIONS.md.
- **Owner tests not available (2026-10-07):** the owner can't run the Practice Tool tests of
  the ult kinds and ability bubbles or the v1.6 player benchmark. They are dropped from the
  plan; what stays unverified is under "Known issues" and the scripts are kept in
  docs/DECISIONS.md ("Owner test scripts (not run)") in case they become possible.
- **Focus: League of Legends only** (owner, 2026-10-07). No CS2 or Dota 2 work for now; the CS2
  module stays as it is.
- **Spectating found before recording (2026-10-07):** see "Spectating: you're not one of the
  match's players" in docs/DECISIONS.md.

## What we're building
A lightweight, Ascent/Outplayed-style game recorder for Windows. It starts with League of Legends,
is designed so more games can be added, and is easy to share with friends.
Owner's PC: RTX 5080, i9-9900K, 32 GB RAM, Windows 11.

### Goals (in priority order)
1. **Performance:** unnoticeable in game. No FPS drop, no stutter, no input lag.
2. Automatic recording with an event timeline for every game.
3. A polished, modern UI in the style of Ascent/Outplayed (own name and branding; no copied logos or assets).
4. Easy to install and share with friends.
5. A plugin-style architecture, so other games are easy to add.

## Performance rules (hard requirements)
- Recording is done by GameRecorder's **built-in recorder** (see "Built-in recorder" below). No
  injection into the game: capture uses Windows Graphics Capture, which is Vanguard-safe.
- Always use hardware encoding (NVENC on NVIDIA; AMF on AMD; Quick Sync on Intel), never x264.
- No in-game overlay. Nothing that injects into or reads game memory.
- In game, the app polls the game API about once per second, runs at below-normal process priority,
  and the UI stays closed or minimized to the tray.
- Target: under 1% CPU for this app while in game; RAM raised by the owner (2026-09-30) in favour
  of responsiveness: ~200 MB for GameRecorder itself while recording (the in-memory replay
  buffer is most of it), plus the UI's WebView, which stays loaded but is suspended and trimmed
  while hidden. Idle near 0% CPU when no game is running. A debug stat shows the numbers.

## Game detection & recording
- Auto-detect when a supported game starts and ends (process detection plus the game's API).
- Record the full game as one video, plus a replay buffer for quick clips.
- Configurable manual "save clip" hotkey.
- File names like `2026-09-30_League_Ahri_Win.mp4`, one folder per game.
- Settings: quality, save folder, auto-delete games older than X days, max disk usage.

## League events (Riot Live Client Data API)
Base URL: `https://127.0.0.1:2999/liveclientdata/` (endpoints: `eventdata`, `activeplayer`, `gamestats`).
It uses a self-signed certificate, so it must be trusted for this localhost address only.
Track events involving the player (matched on Riot ID):
- Death (ChampionKill where the player is the victim)
- Kill, Multikill, FirstBlood, Ace
- Assist (the player's name is in `Assisters`)
- TurretKilled, InhibKilled
- BaronKill, DragonKill, HeraldKill, plus any other objective events the API provides, including steals
- **Ult used:** the API does NOT expose ability casts. Detect the ult keypress (R by default,
  configurable) during a game with a global keyboard hook (no game memory access, Vanguard-safe).
  Label it "ult pressed", since it can't confirm the cast succeeded.

Rules:
- The API returns ALL past events on every call, so track `EventID` and never handle an event twice.
- For each event: add a timeline marker; optionally save a short clip (a few seconds before and after);
  optional Windows text-to-speech callouts, toggleable per event type.

## Timeline (main feature)
- **Offset sync:** the recording starts before the game clock (loading screen). When recording
  starts, read `gameTime` from `/gamestats` and store the offset:
  `video position = EventTime + offset` (see `GameSession.VideoOffset`). Keypresses use the same conversion.
- Video player with a timeline bar underneath; each event is a marker with its own icon and color:
  kill = green, death = red, assist = blue, ult pressed = purple, tower/inhibitor = orange,
  Dragon/Baron/Herald = gold (with a "steal" badge if stolen).
- Hover a marker for details. Click one to jump to 5 seconds before the event. Next/previous-event buttons.
- Filter buttons to show or hide each event type.
- Each game's events are saved as JSON next to its video.

## UI (Ascent/Outplayed style)
- Dark, modern gaming look: sidebar, a game library with thumbnails, and a grid of recent games/clips
  (champion, result, KDA, date).
- Game detail page: player + timeline + post-game summary (KDA, CS/min, gold, deaths timeline).
- Clip editor: trim and export. Tray icon with status (idle / recording / game detected).

## Other games
- Each game is its own crate under `games/` implementing `GameIntegration` (in `crates/cv-core/src/game.rs`).
- The core (recording, timeline, UI, storage) must contain NO game-specific code.
- Games without an event API: recording + manual hotkey clips + manual markers.
- The README must explain how to add a game module. Suggested next games: CS2 (official
  Game State Integration); Valorant has no official live event API, so recording + manual clips only.

## Sharing with friends
- A Windows installer, or a single self-contained .exe, that works on a fresh PC.
- No hardcoded paths, usernames or Riot ID. Nothing else to install: the built-in recorder picks
  the friend's GPU encoder automatically. First-run setup offers a 5-second recording test and asks
  for the Riot ID.
- Auto-update via GitHub Releases is a nice-to-have. No accounts, cloud or telemetry.

## Milestones
- [x] 0. Project setup: solution, projects, core interfaces, this plan
- [x] Choose the language/stack: Tauri 2 (see Decisions made)
- [x] 1. Detect League start/end (process + API), shown live in the app
- [x] 2. Control OBS through obs-websocket (removed later: the built-in recorder replaced it)
- [x] 3. Event tracking + EventID dedupe + game-clock offset + ult keypress (Raw Input)
- [x] 4. Timeline UI: player, colored markers, hover, click-to-jump, filters
- [x] 5. Clips + clip editor + library grid + post-game summary
- [x] 6. Settings + first-run setup (Riot ID, encoder auto-config) + tray icon
- [x] 7. Installer / portable build for friends (`dist\`)
- [x] 8. Game-module guide (`docs/ADDING_A_GAME.md`) + a second game (CS2)
- [x] 9. Built-in recorder: Windows Graphics Capture + GPU colour conversion + hardware H.264
      (NVENC / AMF / Quick Sync via Media Foundation), window → monitor fallback
- [x] 10. Built-in audio: per-process game audio (WASAPI process loopback) + optional mic track (AAC)
- [x] 11. Crash-safe fragmented MP4 muxer + in-memory replay buffer for clips + screenshots
- [x] 12. Recorder self-test (`recorder-selftest.exe` + in-app 5 s test), first-run setup
      without OBS
- [x] 13. Performance test in a real game: FPS (PresentMon), CPU and GPU, not recording vs recording
- [x] Owner's test on Windows: self-test + performance test in a real League game
- [x] 14. Remove OBS completely (code, deps, settings, UI, docs)
- [x] 15. Rename to Clairvoyance + one-time migration of settings, recordings and tools
- [x] 16. Themes: Dark / Light / Match Windows, CSS variables only, no flash
- [x] 17. Storage limit with automatic clean-up, favorites + kept clips protected, log
- [x] 18. Thumbnails in their own folder, made after the game, regenerated when missing
- [x] 19. Responsiveness: library cache, virtualization, lazy thumbnails, 1 s keyframes, measured
- [x] 20. GitHub repo + auto-updater + release workflow (v1.0.0)
- [x] 21. Game modes: per-queue Record / Clips only / Off from the League client, dynamic list (v1.1.0)
- [x] 22. Instant replays: faststart finalize, keyframe snapping, shared player, poster, queued jumps
- [x] 23. Test tools: replay benchmark, end-to-end tests with a fake League client, test report
- [x] 24. Accurate ult tracking: League keybinds + live gates, post-game check from the recording
- [x] Owner's scripted Practice Tool ult test (34/34 casts, 0 false)
- [x] Performance test with the ult build (done with the input build, 2026-10-02: -0.07 % FPS)
- [x] 25. Input tracking (keyboard Raw Input + cursor/button polling, no hooks) + replay overlay + Mechanics stats (v1.4.0)
- [x] Input tracking tested on the owner's PC (SendInput/DPI/video alignment, benchmark, ult regression, League performance test, owner's replay check)
- [x] 26. Ability bubbles on the input overlay: League binds (all cast variants, saved per game), exact frame + interpolated position, overlap rules, ult tie-in, options (v1.5.0)
- [-] Owner's Practice Tool test of the ability bubbles: not available (unverified: see "Known issues")
- [x] 27. Fullscreen without black bars (overlay panel, drop-down, Fit/Fill) + zoomable timeline + frame-exact stepping (v1.6 part A)
- [x] 28. Ult kinds: command / multi_cast / transform / charges, ult episodes live and after the game, "Ult recast" / "Form swap", Data Dragon scan tool (v1.6 part B)
- [-] Owner's Practice Tool test of the ult kinds: not available (unverified: see "Known issues")
- [x] 29. Every window size from 940 × 560 to 4K at 100/125/150 %: no overlap or cut-off (More controls menu, popover placement, page fixes, layout audit test); one "Trail & bubbles" time, bubbles end with their trail piece (v1.7 part 1)
- [x] 30. Ready right after the game: in-place index when the recording stops (no copy, crash-safe), 2 s victory-screen tail, thumbnail right away, faster exit detection (v1.7.1)
- [x] 31. Replays and spectating aren't recorded: League client + in-game API detection before recording, late detection deletes the partial recording, "Record games you spectate" setting, simulator replay / spectate modes (v1.7.1)
- [x] Owner's check of v1.7.1: replays not recorded, spectating deleted after 8 s, Practice Tool game ready at once
- [x] 32. Engineering upkeep: Windows code compiled and linted in CI, rustfmt + clippy, engine `Stage`, `GameIntegration` capabilities, open / reveal limited to the app's files
- [x] 33. Spectating a friend's game (the client calls it your match) found before recording: the match's players vs your account

## Known issues
- v1.6 player: frame steps are seeks, so on recordings with long keyframe gaps (before v1.2:
  2-5.5 s) a step costs more: 49.8 ms median / 117 ms p95 on the owner's PC for a 2-2.5 s one;
  ~290 ms in headless Chromium for 5.5 s (worst case). The 600-marker zoom test ran in headless
  Chromium (redraw median 1 ms, p95 4-7 ms in software); on the PC with real games: p95 0.6 ms. The optional hover
  thumbnail above the zoomed timeline isn't built (it needs a second decoder; skipped to keep
  the targets).
- v1.6 ult kinds: the recast-icon detector is calibrated on the owner's real ready / cooldown
  crops plus synthetic recast icons; real recast/command icons (Tibbers, Daisy...) come with the
  owner's Practice Tool test. Champions whose R doesn't change the icon or show a cooldown until
  the end rely on presses + the duration cap. Old recordings with 2-5.5 s keyframes may miss a
  short recast state (a cast still shows by its cooldown).
- The Windows-only parts added on 2026-10-01 (Media Foundation thumbnails, updater install,
  migration on a real old install) compile and were checked on the owner's PC where noted in
  "Current status"; watch the log for "thumbnail for ... failed" (then ffmpeg is used if present).
- Scrolling the library is smooth at 60 fps; on a much slower CPU (4x throttled) it's ~50 fps.
- Game modes: the League client must be running for the exact queue (it always is when you
  play normally). If the lockfile isn't found (unusual install folder), set Settings > Games >
  League > "League install folder"; until then the coarse game mode decides (Ranked vs Normal
  can't be told apart that way, so both follow the most permissive of their rules).
- Clips-only games have no full video: their clips can't be re-cut in the clip editor, and the
  "Create clip" button is disabled for them.
- Windows SmartScreen may warn about the installer (it isn't code-signed with a certificate;
  updates are still signature-checked by the updater).

- Recordings made before v1.2.0 keep their 2–5.5 s keyframe spacing (only a re-encode would
  change that); marker jumps snap to keyframes so they're fast anyway, but scrubbing to an
  arbitrary point in an old recording can take ~0.1–0.3 s.
- v1.7.1: each recording has ~10 MB of reserved room for its index (8-10 MB of it stays unused
  inside the file). A game longer than ~3.5 h doesn't fit and is finalized by copying after the
  game (as before v1.7.1). Recordings made before v1.7.1 are still finalized by copying.
- Replays: detected from the League client's state + the in-game API's spectator mode (captured
  on patch 16.19). If Riot changes those answers, a replay could be recorded again (the in-game
  check is the safety net; the log says "session check: ..." and "in-game API: spectator mode").
  Spectating: the client reports a spectated game like your own match (owner's check). Since
  2026-10-07 the match's players are compared with your account first, so it isn't recorded at
  all; the player-list format comes from the LCU's documented shape, not a capture. If the
  list can't be read, it falls back to the old way (recorded, then deleted when the in-game API
  says spectator mode ~10 s in).
- Unverified without the owner's tests: the ult kinds on real recast / command icons (Annie's
  Tibbers, Ivern's Daisy...: the detector is calibrated on synthetic recast icons), the ability
  bubbles on a real game with rebound keys, and the v1.6 player's numbers in WebView2 (they
  were measured in Chromium).
- Hovering a game card starts loading its video: a few MB read from disk per hovered game.
- Ult check: tuned on the owner's HUD (4K, HUD scale 0, numeric cooldowns, HUD animations off);
  other HUD scales are found by the scale search (tested synthetically at 1.5×), but colour
  thresholds for very different settings (colour-blind mode, HUD animations on) are untested.
  Auto clips are cut right after the game from the live presses; once the check has run, the
  ones whose moment changed are re-cut (2026-10-07, `engine::recut_auto_clips`); their
  thumbnails come with the next maintenance pass. Typing in the shop search can't be told apart live (the check
  marks it "no cast" afterwards).

- Input tracking: the capture thread costs 0.27 % of one core at 250 Hz while the cursor moves
  non-stop (0.18 % at 125 Hz, 0.10 % at rest), above the 0.1 % target: almost all of it is
  `GetCursorPos` itself (5.5-10 µs per call on the owner's PC); FPS impact measured -0.07 %.
  The mouse wheel isn't recorded (no mouse Raw Input, see "Input tracking"). The first switch-on
  of the overlay for a very long game with the cursor moving non-stop can take ~100 ms.

- Ability bubbles: recordings made before v1.5.0 have no saved binds and use League's
  *defaults* (items on 1 2 3 5 6 7, trinket 4): on the owner's older games (items on 1-6,
  trinket on C) a "4" press shows as a ward and "C" gives no bubble. From v1.5.0 on, each game
  keeps the binds it was played with. Binds changed in the middle of a game count from the next
  game. The first switch-on of the overlay for the 33 min worst case stays ~120-130 ms (as in
  v1.4); the bubbles arrive in parallel and don't add to it.

- v1.7 layout: checked in Chromium (Linux, Inter font) against the mock backend, not inside
  WebView2 on Windows (no Node on the owner's PC); WebView2 is the same engine and Segoe UI
  Variable is narrower than Inter. The "More controls" level is decided from measured widths,
  so other fonts / scalings adapt by themselves.

## Next steps
- League only for now (no CS2 / Dota 2 work). Nice-to-haves: code-signing certificate for the installer;
  optional WebP thumbnails if a WebP encoder is added.

After each milestone: explain how to test it and how to measure its performance impact.

## How to work with the owner
- **Build everything yourself.** This is not a learning project: write all the code and make the
  technical decisions without stopping to teach or asking the owner to write code.
- Only ask the owner when you need him (testing in a real League game, approving installs,
  or a big change to the spec).
- After each milestone, give brief test steps plus a performance check, then continue.
- **Never launch anything Riot** (owner's rule, 2026-10-03): don't start the Riot Client or
  League, don't start or watch replays, and never call a League client (LCU) endpoint that does
  something (POST / PUT / DELETE). Read-only GETs only while the owner is in the client himself.
  Anything that needs League running goes into the owner's test list (helper jobs may only read
  logs and files while he plays). (Job 72 of 2026-10-03 started the Riot Client unattended and
  job 70 started a replay through the LCU: not again.)
