# Clairvoyance (formerly GameRecorder): Project Plan

> **For Claude:** read this whole file at the start of every session. After finishing a
> milestone, tick it off below, add finished decisions to docs/HISTORY.md, and update "Current status".

## Current status
- Older status entries (before v1.7), finished decisions and finished milestones: see docs/HISTORY.md (open it only for background).
- **v1.7 part 1 (2026-10-03): window sizes + one "Trail & bubbles" time.** Nothing overlaps or
  is cut off at any window size from the new minimum 940 × 560 up to 4K at 100 / 125 / 150 %
  (automatic check of 21 page states × 24 sizes: 355/504 before, 504/504 after): the player's
  controls collapse into a "⋯ More controls" menu, popovers stay inside the window. The trail
  length and the bubbles' fade time are one slider (0.25-3 s); a bubble disappears on the same
  frame as the piece of trail drawn at its key-down (pixel-tested at 0.25 s and 3 s). Details in
  "Window sizes, and one Trail & bubbles time".
- **v1.7.1 (2026-10-03, v1.7 part 2): ready right after the game + replays not recorded.** The
  recording makes itself instantly playable the moment it stops (full index written in place
  into room reserved after its header, no copy, crash-safe); League's victory-screen tail is 2 s
  (was 6 s); the thumbnail is made right away. Owner's PC, simulated real-time games opened the
  moment they appear: the game is in Games 2.5 s (5 min) / 3.2 s (35 min) after the match ended and shows its first
  frame at 2.8 / 3.3 s (v1.7.0: 6.5 / 6.4 s and 7.9 / 16.7 s, faststart only after ~28 s); a
  real-size 35 min game (3.15 GB) opens in 134 ms instead of 24.3 s, marker jumps 82-183 ms. Replays and spectated games are detected before anything is
  recorded (League client + in-game API, from a replay captured on the owner's PC) and never
  recorded ("Replay: not recorded"; setting "Record games you spectate"). Details in "Ready
  right after the game" and "Replays and spectating aren't recorded".
- **Owner's check of v1.7.1 passed (2026-10-03):** replays not recorded, a spectated game
  caught by the in-game API and its 8 s partial recording deleted, a Practice Tool game ready at
  once.
- **Waiting for the owner**: the Practice Tool test of the ult kinds (v1.6, see "Next steps"),
  the PC benchmark of the v1.6 player (`--bench-replays` with `"player": true`), and the older
  Practice Tool test of the ability bubbles.

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

## Milestones
- [x] 0-31 finished: see docs/HISTORY.md ("Milestones (finished)")
- [ ] Owner's Practice Tool test of the ability bubbles (see "Next steps")
- [ ] Owner's Practice Tool test of the ult kinds (Annie, Ivern, Shaco, Ahri, Jhin, Jayce/Nidalee, Kog'Maw)

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
  Spectating: the client reports a spectated game like your own match (owner's check), so it's
  caught only by the in-game API ~10 s after the game process starts: those seconds are
  recorded and then deleted, and the label says "Replay or spectating". Possible improvement:
  compare the client's session players with the signed-in player (`/lol-summoner/v1/current-summoner`,
  read-only) to decide before recording; needs one read-only capture while the owner spectates.
- Hovering a game card starts loading its video: a few MB read from disk per hovered game.
- Ult check: tuned on the owner's HUD (4K, HUD scale 0, numeric cooldowns, HUD animations off);
  other HUD scales are found by the scale search (tested synthetically at 1.5×), but colour
  thresholds for very different settings (colour-blind mode, HUD animations on) are untested.
  Auto clips are cut right after the game, before the check, so an auto-clip rule on ult events
  still uses the live presses. Typing in the shop search can't be told apart live (the check
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
- **Owner, Practice Tool test of the ult kinds (v1.6, ~15 min):** see the steps in the v1.6
  test report (also below). Practice Tool with cooldowns ON (not "No Cooldowns"; use the "Reset
  cooldowns" button between ults), Settings > Game modes > Practice Tool on "Record". Level 16+
  so R is rank 3. Then the helper job collects the recording; before/after per champion goes
  into "Ult kinds", and real recast-icon crops replace the synthetic ones in `tests/hud/`.
  1. Annie: R on a spot (Tibbers), then R 10 times on different spots, 1 s apart (commands).
     Wait for Tibbers to vanish, press R once (on cooldown). Reset cooldowns, R, then R 3 more
     times. Expected: 2 ults, 13 recasts, 1 no cast.
  2. Ivern: R (Daisy), R 5 times. Reset, R once. Expected 2 ults, 5 recasts.
  3. Shaco: R (clone), R 5 times. Expected 1 ult, 5 recasts.
  4. Ahri: R and both recasts (3 dashes). Reset, again 3 dashes. Expected 2 ults, 4 recasts.
  5. Jhin: R, then R 4 times (the 4 shots). Expected 1 ult, 4 recasts.
  6. Jayce (or Nidalee): R 6 times, ~7 s apart (Nidalee ~4 s). Expected 6 form swaps.
  7. Kog'Maw: R 5 times, 2-3 s apart. Expected 5 ults.
  8. Caitlyn (control): R once. Expected 1 ult.
- **Owner, Practice Tool test of the ability bubbles (v1.5.0, ~10 min):**
  1. Clairvoyance on v1.5.0 (Settings > General & updates). Settings > Game modes: Practice Tool
     on "Record" for this test (it's Off by default).
  2. In League, before the game: rebind one ability, e.g. E (quick cast) to **T**
     (Settings > Hotkeys). Start a Practice Tool game with a champion whose Q can be spammed
     (e.g. Ezreal), turn on No Cooldowns, buy 2-3 actives (e.g. potions, a Control Ward).
  3. Play this script, slowly enough to remember it: Q spam ~3 s while moving the mouse in a
     circle, with one **W** in the middle; **T** (the rebound E) twice; **D** and **F**; the
     item keys of 2 items; the trinket key (C on your settings); **Ctrl+Q** and **Ctrl+W** a few
     times (level-ups); open chat, type "q w r", close it; R once. End the game.
  4. Open the game, press **I**, overlay options: Ability bubbles on (all four), fade 1.0 s.
     Step through the moments with **,** / **.** (one frame).
  Expected: a bubble on the frame where each key went down (not a frame before), its dot exactly
  on the cursor, staying there while the cursor moves on; Q spam = Q bubbles piled up along the
  mouse path, the W in the middle readable (later Qs step aside, its dot stays on the path); T
  shows **E** with a small "T"; D and F larger; items show their slot number 1-6 (no hint),
  the trinket a ward icon with a small "C"; **nothing** for Ctrl+Q / Ctrl+W and nothing for the
  chat; R solid purple (it says "Ult used" on the timeline), presses without a cast faded and only
  with "Unconfirmed presses" on. Fade slider at 0.1 s and 3 s changes it at once. Then send
  Settings > Advanced > Save test report (it has the game's session.json with the saved binds).
- Owner: play a real game on v1.1.x (timeline, thumbnail after the game, storage page), and
  test game modes: turn ARAM off and play an ARAM (nothing recorded, tray says why), play a
  ranked or normal game (recorded, card shows the queue name).
- Nice-to-haves: Dota 2 module (GSI, like CS2); code-signing certificate for the installer;
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
