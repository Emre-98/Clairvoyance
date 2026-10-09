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
- **Owner tests not available (2026-10-07):** the owner can't run the Practice Tool tests of
  the ult kinds and ability bubbles or the v1.6 player benchmark. They are dropped from the
  plan; what stays unverified is under "Known issues" and the scripts are kept in
  docs/HISTORY.md ("Owner test scripts (not run)") in case they become possible.
- **Focus: League of Legends and Deadlock** (owner; League only since 2026-10-07, lifted for
  Deadlock on 2026-10-09). No CS2 or Dota 2 work for now; the CS2 module stays as it is.
- **Deadlock, milestone 1 steps 1-2 (2026-10-09): signals found, from the owner's two real
  runs.** Crate `games/deadlock` (`cv-game-deadlock`) with the developer option "Deadlock: log
  match signals" (v1.11.0; ran on the owner's PC for a full bot match, the sandbox, a match left
  early and spectating). Result: **Steam's own `content_log.txt` marks a match's start and end
  live with no launch option** (updates disabled / enabled), and the `-condebug` console log
  gives every stage to the second; the normal path is built on the first, refined by the
  second, so users don't need `-condebug`. Not a game integration yet: nothing of Deadlock is
  recorded. Details and the decision: "Deadlock" below.
- **Deadlock, milestone 1 normal path (2026-10-09), tried in a real game (v1.12.0, owner, 2026-10-09: one Street Brawl match without `-condebug` gave exactly one recording, 11:41:40-11:58:17, started the second the match was found, stopped 8 s after the end banner, picture and sound fine, 0 dropped frames, app CPU 0.5 % average; the Hideout and what came after were not recorded):** Deadlock is
  now a game (`DeadlockIntegration`, Settings > Games): one recording per match, started when
  Steam's log says a match was found (before the loading screen) and stopped after the end
  screen; nothing in the Hideout, the sandbox or while spectating. No setup; `-condebug`
  only tightens the end. Replaying the owner's real log lines gives exactly two recordings
  (09:28:20-10:09:17 and 10:15:44-10:17:12 with the console log; 09:28:21-10:09:07 and
  10:15:44-10:17:12 from Steam's log alone). Written without a local compiler: checked by CI
  only. No timeline yet, no fallback buffer, no mode rules: see "Deadlock".
- **Deadlock, key presses on the timeline (2026-10-09), not yet tried in a real game:** your
  presses of abilities 1-3, the ultimate (ability 4), item slots 1-4, melee and parry become
  timeline markers ("Ability 1", "Ultimate", "Item 2", "Melee", "Parry"), only while Deadlock
  has the focus and the match is in progress. The binds are read from the game's own
  `citadelkeys_personal.lst` at each match start (the `.vcfg` files turned out to hold console
  binds only). Not counted: Alt+1-4 (ability upgrade), typing in the chat, keys while the shop
  is open, the same key again within 1 s (melee 0.4 s). Every press is also kept as a key mark
  for the replay check to come. New groups "Abilities", "Item keys", "Melee & parry" (hidden
  until their chip is clicked, like the ult). Written without a local compiler: CI only. Details:
  "Deadlock" > "Key presses on the timeline".
- **Engineering upkeep (2026-10-07, after a full review of the code):** the architecture is
  sound and the project continues as is (no rewrite). Done: CI compiles and lints the Windows
  code (`windows` job) and checks rustfmt + clippy (Rust pinned to 1.99); engine session states
  made explicit (`Stage`); `GameIntegration` split into optional capability traits; open /
  reveal limited to the app's own files. See "Engineering upkeep" in docs/HISTORY.md.
- **League, 2026-10-07:** a spectated friend's game is found before anything is recorded (the
  match's players vs your account); auto clips follow the ult check; the timeline shows kills /
  deaths / assists by default (the rest are greyed chips, remembered); clips can be exported
  with the input overlay burned in. Details in docs/HISTORY.md: "Spectating: you're not one of
  the match's players", "Auto clips follow the recording check", "Timeline: kills, deaths,
  assists by default", "Clips with the input overlay". Not yet tried inside the app on Windows.
- **v1.8 (2026-10-07, owner's request), not yet tried in a real game:**
  - *Part 1, richer timeline:* towers / inhibitors / objectives say who, lane / tier / side and
    the gold you got ("≈ +250", measured from your gold); "Completed <item>" chips (inventory
    compared between player-list reads, undo-safe); summoner spell chips ("Flash": D / F press
    + the slot's cooldown in the recording, ult check v5); a faint APM chart (10 s bins) behind
    the markers with the value on hover.
  - *Part 2, time-synced scoreboard:* all 10 players (champion, level, KDA, CS, items, spells) at
    the playback time, card under the player + over the video with O (Tab held in fullscreen);
    saved as changes only (35 min worst case 44 KB vs 296 KB as full snapshots); older games
    show your own final numbers.
  - *Part 3, smaller files:* measured first with software stand-ins on a game-like test clip: no
    candidate matched the current quality at a smaller size except B-frames, so **defaults are
    unchanged** until `scripts/encoder-compare.ps1` measures the GPU encoder on the owner's PC
    (VMAF + SSIM + size on a real capture). HEVC / AV1 recording and quality-based rate control
    are built as options (H.264 fallback whenever the GPU, Windows' decoder or the player can't).
  - Tests after merging main (2026-10-07): Rust 181 (+ rustfmt, clippy -D warnings with 1.99, and
    the Windows cross-clippy of the whole workspace: clean), UI unit 30, Chromium: timeline 23/23,
    scoreboard 16/16, layout audit 528/528, filters 7/7, export 11/11, fullscreen 94/94,
    overlay 20/20, bubbles 20/20, frame stepping 27/27. Details in docs/HISTORY.md: "Richer timeline",
    "Time-synced scoreboard", "Smaller files (v1.8 part 3)".

- **Share size, no desktop, Developer tools (2026-10-08, owner's request), not yet tried on
  Windows:** Discord copies target 18 MB with 10 % headroom, peak 1.25× average, up to 3 tries
  (GPU, GPU, then libx264), never a file over the limit; clips up to ~3 min. Recording never falls
  back to the screen any more (window not found in 30 s → error; no frames → only a log line);
  "Capture the whole screen" is a dev option and settings v5 turns it off once. New
  `dev_tools` setting (off) hides the technical options, recorder test, CPU/RAM stat,
  Performance test and Advanced.
- **v1.8 sharing (2026-10-08, owner's request), not yet tried on Windows:** a Share button on
  every clip puts it on the clipboard as a file (Ctrl+V in Discord). "Fit for Discord" (saved
  setting, on by default, a tick on the Clips / game page and in Settings) makes an H.264 copy
  under 19.5 MB (Discord's free limit is 20 MB since 2026-08-13): bitrate from the length,
  1080p60 → 1080p30 → 720p60 → 720p30 → 540p30, microphone mixed into the game track, one
  retry if the encoder overshoots; clips that already fit are shared as they are; up to ~3.5 min.
  Copies live in `<data>\Share`, deleted at start-up and after 24 h. Allowed during a game
  (ffmpeg below-normal priority, GPU encoder). Linux, x264 standing in for the GPU: a 30 s
  1080p60 12 Mbps clip with a mic track (46.8 MB) → 18.98 MB at 1080p30 in 17 s; 100 s 720p
  (53.7 MB) → 18.45 MB at 540p30; both first try. Details in docs/HISTORY.md "Share (Discord)".
- **Spectating caught earlier (2026-10-08, owner's request), not yet tried on Windows:** when the
  League client can't tell (not reachable, or the match's players can't be read after 3 tries),
  nothing is recorded until the in-game API says: spectator mode → not recorded; your champion →
  recording starts (your own game then misses its loading screen). TFT, when the client says
  so, records at once as before. Safety net: no verdict after 60 in-game reads or 3 min →
  recorded anyway, the late check still deletes it. Your id in the players list now counts as
  "you're playing" even alone with bots (Practice Tool, Co-op vs AI). Details in
  docs/HISTORY.md "Spectating: when the client can't tell".
- **Cursor trail colors + clicks in the keys strip (2026-10-08, owner's request; PR #9 + this
  one):** the trail is colored by cursor speed (Rocket, default: blue pilot light when slow,
  orange -> red flame on flicks, same clock for width and fade so colors blend). Other color
  sets in the overlay menu: Plasma, Toxic, Sunset, Frost, and Classic (the old yellow line).
  "Keys pressed" shows LMB / RMB / MMB / M4 / M5 in their own lane. Drawing cost ~0.35 ms per
  frame at 1080p (Chromium, Linux).
- **Strive Gold ability bubbles (2026-10-08, owner's pick from mockups):** round buttons like a
  fighting game's input display (Guilty Gear Strive): black edge, League gold ring, the action's
  colour inside, a centred Inter Black letter (bundled, @fontsource/inter). A press punches in at
  1.5x and springs back (~0.3 s) with a white flash, a colour burst and two gold rings. Colours
  (Strive's buttons): Q orange (Dust), W green (Slash), E blue (Kick), R red (Heavy Slash), D
  violet, F yellow, ward teal, items slate. Sizes by importance: Q W E R 1.18, D F 1.0, ward 0.88,
  items 0.76. Bubble pixel test 20/20; draw with 40+ bubbles p95 1.2 ms (Chromium, Linux).
- **Clip editor + Share polish (2026-10-08, owner's request), Ctrl+V into Discord not yet tried
  on Windows:** the clip editor has the "Share: fit for Discord" tick (ShareFitToggle, same saved
  setting as the clips-list tick, in sync), "Exact cut" renamed "Start exactly at the handle",
  "Create clip" scrolls the editor into view (block "nearest"; instant with reduced motion).
  Share's clipboard code moved to `cv_capture::win::clipboard`: CF_HDROP (UTF-16, double NUL) +
  "Preferred DropEffect" = copy like Explorer, a message-only window owns the clipboard (a NULL
  owner can make SetClipboardData fail), 5 × 50 ms retries, absolute path of a file checked to
  exist. Its unit test (copy a temp file, read CF_HDROP back with DragQueryFileW) runs in CI's
  `windows` job. Copy failed → error toast with "Show file". Chromium: share 23/23, export 21/21.

- **Copy protection without lag (2026-10-09, owner's request), not yet tried on Windows:**
  - *Tamper seal:* `crates/cv-seal` signs the release exe after compiling (Ed25519, secret
    `CV_SEAL_KEY`, run by `beforeBundleCommand`); `app/src/integrity.rs` checks it once at
    start-up on its own thread and refuses to start a changed copy (message with the official
    download page). Off in dev / test builds. Release workflow: draft first, verify the exact
    exe, then publish. Public key, magic and download URL masked in the exe. Tested on a
    cross-built release exe (35 MB): sealed copy passes; rebranded name, one patched byte,
    zeroed seal fail; Tauri's bundle-type bytes and appended data (code signing) pass. Check
    ~60 ms CPU, 35 MB RAM freed at once (Linux, `cv-seal verify`).
  - *Cold UI only obfuscated:* Settings and first-run Setup load on demand (Settings preloaded
    when idle) and only their chunks are obfuscated (strong settings: none of it runs per frame).
    Library / player / timeline / overlay untouched; main chunk 270 → 218 KB. Chromium, mock
    build, obfuscated vs plain: start-up 255 vs 258 ms, Settings open 14.2 vs 14.1 ms, first-run
    Setup 467 vs 377 ms (once). Layout --quick 66/66 (all Settings sections).

- **League: a real game taken for spectating after an account switch (2026-10-09, owner's
  report), fixed, not yet tried in a real game; written without a local compiler (CI only):**
  - *What happened (the app's log + League's own game log, 18:44:54):* first game on the second
    account, Ranked Solo/Duo. The client check was right ("you are one of the match's players:
    Some(true)", new lockfile and account read fine), recording started, and 3 s later the
    in-game API answered "spectator mode" once: 20 ms **before** the champions were spawned
    (League's log: API read at 5.326 s, `GAMESTATE_SPAWN` at 5.346 s). That single answer deleted
    the recording and the rest of the game was ignored. Nothing was cached from the first
    account; the account switch only happened to be the game where the read hit that instant
    (the ten games before it read 0.3-1 s after the spawn). Whether the second account answers
    like that every time can't be told from the logs: the fix covers both.
  - *Fix (`games/league/src/watch.rs`, `lib.rs`):* "spectator mode" counts only after 3 answers
    in a row over 1.5 s (`LiveCheck`; your champion settles it at once); and when the client
    lists the account logged in now among the match's players, that is final (the in-game API
    can't turn it into spectating; log: "kept as your match"). "You" on the timeline / scoreboard
    is the game's own answer, else the account logged in to the client now; the saved Riot ID
    is used only when neither says (before, it was always added). Lockfile, account and players
    were and are read again at every game start.
  - *Unchanged:* replays (client says so) never recorded; not in the match's players / players
    unreadable / client not found → nothing recorded until the in-game API says, now 2 s later.
  - *Simulator:* "A replay found late" became "A match, the game says spectator mode" (recorded).
  - Tests: `watch.rs` (roster per account, the repeated-answer rule), `watch_mock.rs` (the early
    answer with and without a readable roster), `mock_match.rs` `switching_accounts_between_games`
    (A, then B with a new lockfile, an in-game API that never shows a champion and A's Riot ID
    still saved, then A again; with and without a new `LeagueIntegration`).

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
- [-] Owner's Practice Tool test of the ability bubbles: not available (unverified: see "Known issues")
- [-] Owner's Practice Tool test of the ult kinds: not available (unverified: see "Known issues")
- [x] 32. Engineering upkeep: Windows code compiled and linted in CI, rustfmt + clippy, engine `Stage`, `GameIntegration` capabilities, open / reveal limited to the app's files
- [x] 33. Spectating a friend's game (the client calls it your match) found before recording: the match's players vs your account
- [x] 34. Auto clips re-cut after the ult check; timeline decluttered (kills, deaths, assists by default); clip export with the input overlay burned in
- [x] 35. Richer timeline: tower / inhibitor / objective details + gold, completed-item chips (undo-safe), summoner spell chips (key + recording), APM chart behind the markers (v1.8 part 1)
- [x] 36. Time-synced scoreboard: all 10 players saved as changes only, card + overlay (O / Tab) following playback and scrubbing, final-only fallback for old recordings (v1.8 part 2)
- [x] 37. Smaller files: measured (software stand-ins + PC script), HEVC / AV1 recording and quality-based rate control as options with H.264 fallback; defaults unchanged until the GPU numbers (v1.8 part 3)
- [x] 38. Share: one click to the clipboard, "Fit for Discord" copy under 19.5 MB (on by default), copies cleared after 24 h
- [x] 39. Spectating caught before recording also when the client can't tell (the in-game API decides; safety net records anyway)
- [x] 40. Deadlock M1 step 1: match-signal diagnostic ("Deadlock: log match signals" + "Copy log path")
- [x] Owner: bot match + sandbox + left-early match + spectating with the diagnostic on (2026-10-09)
- [x] 41. Deadlock M1 step 2: signals chosen from the owner's logs (Steam's content log without -condebug, the console log with it), written under "Deadlock"
- [x] 42. Deadlock M1 normal path: live start / stop (`match_only`) on Steam's log, refined by the console log; no pre-roll, trim or `-condebug` notice needed (owner's first match on v1.12.0 worked)
- [ ] 43. Deadlock M1 fallback: capped rolling buffer on disk, health rules, matches cut out of it, game-neutral in the core
- [ ] 44. Deadlock M1 modes + spectating / replays never saved
- [ ] Owner: second bot match on the normal path = exactly one recording; one more without `-condebug` saved through the fallback
- [x] 45. Deadlock M2 part 1: key presses on the timeline (abilities, ultimate, item slots, melee, parry) with the game's own binds
- [ ] Owner: one match with the key presses, then compare the markers with the video
- [ ] 46. Deadlock M2 part 2: kills, objectives, items, souls, scoreboard, ult confirmation and auto clips from the replay file
- [ ] Owner's real-game check of v1.8 (see "Next steps")
- [ ] Owner's `scripts/encoder-compare.ps1` run, then switch the defaults where the numbers allow

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
  all; the player-list format comes from the LCU's documented shape, not a capture. Since
  2026-10-08, if the list (or the client) can't be read, nothing is recorded until the in-game
  API says (log: "session check: ... -> Unsure(Unknown)"); only a hold the in-game API never
  settles (60 reads / 3 min) records anyway and falls back to the old late delete.
  Since 2026-10-09 the in-game API's "spectator mode" needs 3 answers in a row (a real match
  says it once before the champions spawn), and it never overrides a client that lists you
  among the match's players: a replay the client would report as your running match with you
  in it (never seen) would be recorded.
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
  thumbnails come with the next maintenance pass. Typing in the shop search can't be told apart
  live (the check marks it "no cast" afterwards).

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

- Clip export with the input overlay: checked in Chromium (mock backend) and with the system
  ffmpeg on Linux (frame alignment, sound, cancel), not yet inside the app on Windows. Rendering
  is bound by PNG encoding in the page: ~18 ms per 1080p frame in headless Chromium, so a 30 s
  clip at 60 fps takes ~45 s (progress bar, Cancel). Possible speed-up: encode the PNGs in
  workers in parallel.

- v1.8 summoner chips: D / F geometry measured at HUD scale 0 (scaled with the ult's HUD fit);
  untested on other HUD scales in real recordings. Smite's charges and Teleport's channel may
  show the cooldown at another moment than the press (the press window is 1.5 s).
- v1.8 item chips: an UNDO more than ~2 minutes after a sale, or a purchase and undo within one
  inventory read (< 1 s), can't be told apart; Data Dragon must be reachable once per patch
  (else no item chips that game, logged).
- v1.8 part 3: the HEVC / AV1 encoders, Windows' decoders and WebView2 playback of HEVC can only
  be checked on the owner's PC (this Chromium build plays AV1 but no H.264 / HEVC). The
  quality-based mode's values (Media Foundation "Quality" 70 / 80) and the HEVC / AV1 bitrate
  factors (0.75 / 0.6 of H.264) are starting points, not measured on a GPU yet.


## Deadlock (in progress)
Goal: Deadlock support that feels built for Deadlock, with League's integration as the quality
bar. Crate `games/deadlock` (`cv-game-deadlock`); no Deadlock code in `cv-core` / `cv-capture`
(what the core needs is added game-neutrally and documented in docs/ADDING_A_GAME.md).

Hard rules (owner, 2026-10-09):
- VAC-safe: no memory reading, no injection, no in-game overlay (so no deadlock-rs, no
  Overwolf event provider). Allowed: files the game or Steam writes, Valve's own replay files,
  local key presses. The performance rules above apply.
- No dependence on unofficial web services (deadlock-api.com ...); ask the owner first if one
  would add a lot. Downloading replays ourselves: ask first; by default only replays the user
  downloaded are used.
- Only real matches are saved, one per game entry, from the loading screen to just after the
  end screen. Nothing from the Hideout, menus, sandbox, replays or spectating (unless "Record
  games you spectate" is on).
- Normal path first; the on-disk fallback buffer is only a safety net, never above its cap
  (default 75 min), and a long session is never stored in full.
- **Normal users shouldn't have to add `-condebug`** (owner's preference, 2026-10-09). It is
  needed for the diagnostic run only (to see everything once). When choosing the signals, prefer
  any source that works without it; require it for users only if nothing else can tell a match's
  start and end, and say so to the owner before building on it.

Checked on the owner's PC (2026-10-09, read-only):
- Steam `C:\Program Files (x86)\Steam`, Deadlock in the library `E:\SteamLibrary`
  (`steamapps\common\Deadlock`, app id 1422450, `appmanifest_1422450.acf` → `installdir`).
- The exe is `game\bin\win64\deadlock.exe`; its window title in the Hideout / menu is "Deadlock".
- `game\citadel` has `cfg`, `rpt`, `save`; **no `console.log`** (no `-condebug` yet) and no
  `replays` folder yet (no replay downloaded so far).
- Key binds: the gameplay binds are in `userdata\<id>\1422450\remote\cfg\citadelkeys_personal.lst`
  (every action with its key, see "Key presses on the timeline"). `local\cfg\user_keys_0_slot0.vcfg`
  and `game\citadel\cfg\user_keys_default.vcfg` are the engine's console binds only (the owner's
  has just F7 = console; the default one has no ability in it).
- Steam's recording folder is `userdata\<id>\gamerecordings` (`timelines\` empty: Steam's game
  recording is off or was never used). Launch options live in
  `userdata\<id>\config\localconfig.vdf` (`"LaunchOptions"` in the app's block; Steam writes
  that file late), so the app can tell whether `-condebug` is set without touching Steam's files.
- Unverified still: the console log's line format and flush delay, the replays folder, the
  timeline file format, whether Steam saves a chosen recording folder as `BackgroundRecordPath`.

Milestone 1 step 1, the diagnostic (`games/deadlock/src/diag.rs`, `app/src/devopts.rs`):
- Switch: Settings > General > Developer tools > "Deadlock: log match signals" (stored as
  `settings.games.deadlock.log_match_signals`; needs "Save changes"). A thread
  ("deadlock-signals") idles until it's on; then it checks the process list every 2 s, and while
  `deadlock.exe` (or `project8.exe`) runs it writes
  `%LOCALAPPDATA%\Clairvoyance\logs\deadlock-signals-<date>-<time>.log`, one file per game run
  (the 8 newest are kept, 256 MB cap each). "Copy log path" copies the newest one's path.
- Each line: wall-clock time (ms), seconds since the log began, source, text. Sources:
  - `console.log` (and any other `.log` in `game\citadel`, `game`, `game\bin\win64`): every new
    line as written, so the game's own timestamp stays next to ours; looked at every 100 ms.
    Lines that were in the file before watching began are marked `console.log*`.
  - `window`: titles, classes and sizes of the game's visible windows + whether it has the
    focus (every 300 ms, logged on change); `process`: running / exited.
  - `replays` (`citadel\replays`, `citadel\addons\replays`), `game-folder`, `game-cfg`,
    `game-rpt`, `game-save`: files added / changed / removed, with size and time (every 1 s).
  - `steam-timeline` (`gamerecordings\timelines`, content of changed `.json` files copied in),
    `steam-recording`, `steam-userdata` (`userdata\<id>\1422450`, small text files copied in
    when they change, e.g. `pending_replay_requests.lst`).
  - `steam-httpcache` (`appcache\httpcache`): file names, sizes and times only (every 3 s).
  - Steam's own logs (`Steam\logs\*.txt` except the web helper's and the connection logs): new
    lines only.
- Read-only and without locks: files are opened for a moment with full sharing, folders are
  only listed, window titles come from the window manager. Nothing in the game process.
  Watching continues 60 s after the game closes (Steam writes its timeline file then). The last
  line says how long the looking itself took (its CPU cost).
- The log can contain player names, chat lines and Steam ids (it's the game's console output):
  send it privately, don't attach it to a public issue.
- Tests: `cargo test -p cv-game-deadlock` (Valve file parsing on the owner's file shapes, the
  log follower: appearing / half-written lines / started over / gone, folder watching by name
  only, content copies, log rows, pruning).

Milestone 1 step 2, the signals (owner's two runs, 2026-10-09, game build 10931; the real lines
are the test fixtures in `games/deadlock/tests/fixtures/`):
- Run 1: Hideout → bot match 113291198 played to the end → Hideout → quit. Run 2: Hideout →
  sandbox → bot match 113300990 left after ~1 min → Hideout → spectating a live match → quit.
  Owner's notes: match 1 started 9:28, finished 10:09 (matches the log to the minute; no finer
  notes were taken, so lateness is measured against the lines' own timestamps).
- **Without `-condebug`: Steam's own `Steam\logs\content_log.txt`** (always written). Deadlock
  asks Steam not to update it while a match is on:
  - `App 1422450 updates disabled for 300 seconds` (+ `state changed : …,Updated Disabled by
    app,`) at 09:28:21 and 10:15:44 = the second the match was found (console: lobby created
    09:28:20 / 10:15:44), i.e. **before the loading screen**; renewed every 6 minutes during
    the match (a heartbeat);
  - `App 1422450 updates now enabled` at 10:09:07 (= `PostGame`, the end banner) and 10:17:12
    (= the moment the owner left the second match);
  - nothing for the Hideout, the sandbox or spectating. Seen by the logger 0.2-1.2 s after
    the line's own (whole-second) timestamp, at a 1 s look interval.
  - It doesn't say: when the match is really in progress, played to the end vs left early, the
    end of the end screen, the mode, win / loss. Street Brawl gives the same lines (owner, 2026-10-09). Private lobbies: unseen.
- **With `-condebug`: `game\citadel\console.log`** (lines `MM/DD HH:MM:SS text`, whole seconds,
  local time; seen by the logger in the same second; **the game appends to the file across
  launches**, it never clears it). Per match:
  | moment | line | run 1 | run 2 |
  |---|---|---|---|
  | match found | `Lobby <id> for Match <match id> created`, then `[Citadel Play Controller] CCitadel_PlayController::OnMatchFormed` | 09:28:20 | 10:15:44 |
  | connecting | `[Client] Map: "start"`, `Host activate: Remote Connect (…)` | 09:28:21-25 | 10:15:45-49 |
  | loading / intro | `OnGameStateChanged: MatchIntro (4)`, `WaitForMapToLoad (5)`, `PreGameWait (6)` | 09:28:26 / :31 / :32 | 10:15:50 / :55 / :56 |
  | match running | `OnGameStateChanged: GameInProgress (7)` | 09:29:01 | 10:16:25 |
  | match over | `OnGameStateChanged: PostGame (8)`, `Lobby … for Match … destroyed` | 10:09:07 / :08 | - |
  | end screen left | `OnGameStateChanged: End (11)`, `[Client] CL:  disconnect` | 10:09:17 | - |
  | left early | `[Client] CL:  disconnect` + `Send msg 9015 (k_EMsgClientToGCLeaveLobby)` with no `PostGame` before it, then `Lobby … destroyed` | - | 10:17:12 |
  | back in the Hideout | `[Client] Map: "dl_hideout"`, `[Hideout] Hideout Lobby Connection State: NoLobby (0)` | 10:09:20 | 10:17:15 |
  - The match id is in the lobby line (it names the replay / match history entry).
  - **Traps:** the Hideout and the sandbox are local games that also print `ChangeGameState: …
    GameInProgress (7)` and `OnGameStateChanged: GameInProgress (7)`. A match is only: a lobby
    created → `Remote Connect`. Sandbox = `Spawn Server: new_player_basics` / `Map:
    "new_player_basics"` (10:14:13-10:15:13), never a lobby.
  - Spectating a live match: `Send msg 9109 (k_EMsgClientToGCSpectateLobby)`, `Host activate:
    Playing Broadcast (http://…steamcontent.com/tv/…)`, `[HLTV Broadcast] …` lines, then
    `OnGameStateChanged: GameInProgress (7)` / `PostGame (8)` of the watched match, and
    `[HLTV Broadcast] OnDemoStreamStop()`; no lobby of your own, and Steam's updates stay enabled.
  - No win / loss line was found in the console log (it comes from the replay later).
- Sources that showed nothing about the match: the window title ("Deadlock", class `SDL_app`,
  always), `rpt` / `save` / `cfg`, the Steam cloud folder (written only at start and exit),
  Steam's timeline folder (empty: Steam recording off), `gameprocess_log.txt` (`SSGL: Game
  server change` on every map change, Hideout included). Steam's HTTP cache changed only while
  browsing the match list in run 2. **No replay was downloaded** (no `replays` folder), and
  playing back a downloaded replay is unseen: both still to capture.
- The diagnostic's own cost: 62.65 s of looking over 2,337 s (2.7 %) in run 1, 16.99 s over
  526 s (3.2 %) in run 2: above the 1 % target (it lists folders every second and follows ~45
  Steam logs). Fine for a one-off diagnostic; the integration will follow two files only.
- **Switching the console log on automatically: tried, doesn't work** (owner's test,
  2026-10-09 10:32-10:37, launch options empty, command line `-steam -console`): an
  `autoexec.cfg` in `game\citadel\cfg` with `con_logfile "console_cv.log"` produced no log file
  and `console.log` didn't grow. Editing Steam's `localconfig.vdf` (Steam overwrites it on exit)
  and launching through `steam://run` (confirmation prompt, only from our app) are ruled out.
  So the console log stays an opt-in extra for users who add `-condebug` themselves.

Decision (owner's preference: no launch option for normal users): the **normal path is built
on Steam's content log**, which needs nothing from the user: recording starts when updates are
disabled (the match was found: on time, so **no pre-roll**), and ends a fixed tail after they
are enabled again. The console log, when the user has `-condebug`, refines it (exact
in-progress / end-screen moments, left early, match id, spectating) but is never required, and
no "add -condebug" notice is shown. The safety buffer stays the net for when neither works.
Risks to check in the next owner test: only two matches seen so far (both bot matches on
Valve servers); the Steam client must be running normally (it is whenever Deadlock runs).

Milestone 1, the normal path as built (`games/deadlock/src/signals.rs`, `lib.rs`):
- `match_only() = true`. Each poll (1 s) reads what was appended to Steam's
  `logs\content_log.txt` and, if it exists, `game\citadel\console.log` (two small reads, files
  opened for a moment with full sharing), and feeds the lines' signals, by their own
  timestamps, to a `Detector`.
- A match begins at "updates disabled" or at a lobby of your own being created, whichever line
  comes first (they are the same match). Phase `Loading` until the console says
  `GameInProgress (7)`; without the console log, `InProgress` 40 s after the match was found
  (only the status text depends on it). The match clock (`game_time`) is seconds since found.
- It ends (`Ended`, then the engine's tail): with the console log at `End (11)` / `CL:
  disconnect` / the Hideout's map, i.e. when the end screen is left or the match is (tail
  2 s), at most 45 s after `PostGame (8)`; without it at "updates now enabled" = the end
  banner or the moment you leave (tail 8 s, which covers a little of the end screen; after
  leaving early it is 8 s of the way back to the Hideout: the known cost of having no console
  log).
- Game-state lines only count inside a match of your own, so the Hideout, the sandbox and
  spectating never start anything. Spectating can't be recorded yet ("Record games you
  spectate" is not offered for Deadlock).
- Safety rules: no renewal from Steam for 420 s → over (leftover lines of a crashed run);
  Steam says the game closed, or the console shows a new launch (`Source2Init OK`; the game
  appends to its log across launches) → over; the console goes quiet for 30 s after Steam
  enabled updates → over; nothing lasts longer than 3 h.
- When the game process appears, and again when a recording starts, the logs' recent past
  (64 KB / 256 KB) is replayed, so starting the app in the middle of a match works and a match
  that already ended is ignored.
- **No pre-roll and no trim were needed:** the start signal comes before the loading screen
  and is seen within a second, so the recording itself begins at the match start and ends
  after the end screen; the file is made playable in place like League's.
- Not built yet: the capped fallback buffer (for when Steam's log says nothing), mode rules
  (bot match / Street Brawl rows), the "left early" tag in the library, anything from the
  replay file. Whether window capture of Deadlock (`SDL_app`, 3840x2160) gives a picture is
  unknown until the owner's test: if the video is black, switch on Developer tools > "Capture
  the whole screen" and report it.
- Tests (`cargo test -p cv-game-deadlock`): the real lines of both runs replayed (one
  recording per match with exact times, with and without each log; nothing outside the
  matches although the Hideout / sandbox / spectating print "GameInProgress" 8 times), line
  parsing, the time rules, the log follower, and the module against real files through
  `start` / `poll` / `stop` as the engine calls them.

Key presses on the timeline (`games/deadlock/src/binds.rs`, `keys.rs`; 2026-10-09, game build 6766):
- **Where the binds are (read on the owner's PC):** `citadelkeys_personal.lst` is a Valve
  KeyValues text: `"KeyBindings" { "Name" "DEFAULT" … "Keys" { "Ability1" { "Key" "1" } … } }`,
  about 150 actions, each with `"Key"`, sometimes `"Modifier"` (`"ALT"`, `"SHIFT"`) and, for
  newer actions, `"Key2"` (`"NONE"` = empty). The owner's values: `AbilityMelee` Q, `Ability1-4`
  1 2 3 4, `Item1-4` Z X C V, `HeldItem` F (the game's settings call it "Melee parry / throw
  held item"), `OpenHeroSheet` B ("Open shop"), `ChatTeam` Enter, `Chat` Shift+Enter,
  `AbilityUpgrade1-4` Alt+1-4. The file is the test fixture
  `games/deadlock/tests/fixtures/citadelkeys-personal.txt`.
- The file of the Steam account that played last is read at every match start (6.7 KB, opened
  for a moment, never written). Actions it doesn't list, or no file at all, use those defaults.
  The log says once which binds are in effect ("Deadlock: key binds (the game's file): abilities
  1 2 3 4, items Z X C V, melee Q, parry F").
- A plain bind also counts with Shift / Ctrl / Alt held (casting while dashing or crouching),
  unless that chord is bound to something itself (Alt+1 = upgrade ability 1: no marker).
- Live gates (the engine already requires focus + match in progress): chat open (from the chat
  binds until Enter / Escape, or 15 s without any key), shop open (from its bind until the bind
  again / Escape, at most 90 s: there the number keys switch tabs and letters go to the search),
  contact bounce (80 ms), the same action again within 1 s (melee 0.4 s). Filtered presses stay
  in the session as key marks with the reason (`chat`, `shop`, `repeat`).
- Timeline: `UltPressed` for ability 4; new game-neutral kinds `AbilityPressed`, `ItemPressed`,
  `Melee`, `Parry` (core `events.rs`; UI groups "Abilities", "Item keys", "Melee & parry" with
  their own icons). Titles are the game's own bind names; the hero's ability names come with
  the replay file.
- **Limits:** a press isn't a cast (cooldown, silenced, dead, empty item slot: unknown until
  the replay check). Only keyboard keys are seen: an action bound to a mouse button, the wheel
  or to Shift / Ctrl / Alt alone can't be followed (the log names them). Hero-specific binds
  (`citadel_hero_settings.lst` > `heroes`, empty on the owner's PC: format unseen) aren't read.
  `"Modifier2"` for a second key is assumed, not seen. Whether the shop really keeps the keys
  the whole time it is open, and whether it closes by itself (death, damage), is unverified:
  that is what the 90 s cap is for.
- Tests (`cargo test -p cv-game-deadlock`): the owner's file parsed (every followed action, the
  chords), changed / unbound / mouse binds, half-written files, Valve key names, every key's
  event, mashing, modifiers, chat, shop, and the module's `on_key` / `take_key_marks`.

Next: the replay file (milestone 46; needs the owner's replay run first), then the fallback buffer and the modes (milestones 43-44).

## Next steps
- **Owner, League account switch (after installing the build with the fix):** (1) play a game
  on account A, log out of the League client, log in to account B, play a game (any queue)
  without restarting Clairvoyance: both are in Games, B's with B's champion, KDA and "you" on
  the scoreboard. (2) The same with Clairvoyance closed and started again between the two.
  (3) Spectate a friend's live game once: nothing in Games, the tray says "Spectating: not
  recorded". (4) Settings > Advanced > Simulate, What = "A match, the game says spectator
  mode": recorded. In the log each game has "session check: you are one of the match's
  players: Some(true)"; a line "in-game API: no champion of yours ..." followed by a normal
  recording is the old bug being caught.
- **Owner, move releases to Clairvoyance-releases (2026-10-09; RELEASING.md "Moving releases to
  Clairvoyance-releases"):** create the public repo with a README, the seal key + `RELEASES_TOKEN`,
  then release 1.14.0 (the bridge) while this repo is public, check both latest.json addresses,
  wait about a week, make this repo private. Code side done: updater endpoint, Release workflow
  (publishes there, SHA-256 table, mirrors here while public).
- **Owner, tamper seal (once, ~5 min, before the next release):** RELEASING.md "The tamper
  seal": make the key, set `CV_SEAL_KEY` / `CV_SEAL_PUBKEY`. After the release: install it,
  start it (log line "integrity: ok (N ms)"), and try a copy with one byte changed (it must
  refuse to start).
- **Owner, Deadlock key presses (one match, any mode):** play normally and use every ability,
  the ultimate, an active item, melee and parry at least once; type one chat line and open the
  shop once. Afterwards open the match in Games and click the greyed chips "Ult", "Abilities",
  "Item keys", "Melee & parry": each marker should sit on the moment you pressed the key in the
  video. Nothing to send: the log and the session file are read afterwards.
- **Owner, Deadlock's first recorded match (done 2026-10-09, v1.12.0):** worked, see "Current status".
- **Owner, Deadlock (done 2026-10-09):** the two diagnostic runs. Switch "Deadlock: log match
  signals" off again (Settings > General). Still useful when you have a minute, with the option
  on: download one replay (match history > "Download Replay"), watch it for 30 s, close the
  game: it shows where the file lands and what replay playback looks like in the logs.
- **Owner, spectating caught earlier (~5 min, no Riot needed):** Settings > Advanced >
  Simulate, What = "Spectating, players unreadable": the status says "Checking whether this is
  a replay…", then "not recorded", and no folder is left. What = "A match, players unreadable":
  recorded from the end of the loading screen. Then, when a friend plays, spectate them once:
  nothing should appear in Games and the log shows "session check: ...".
- **Owner, Share (~5 min):** Clips page, "Fit for Discord" ticked: Share a 30 s hotkey clip,
  Ctrl+V in a Discord chat, send it; it should upload (no Nitro prompt) and play in the embed.
  Untick it and share again: the original file is pasted. Then share once during a Practice
  Tool game (alt-tab) and watch FPS; the log line "share ...: ... in N ms" gives the encode
  time and size.
- **Owner, encoder measurement (v1.8 part 3, ~10 min):** Clairvoyance's ffmpeg downloaded
  (Settings > Clips). Start a Practice Tool game, then in PowerShell in the repo folder:
  `powershell -ExecutionPolicy Bypass -File scripts\encoder-compare.ps1`, switch to League within
  10 s and play a normal minute (walk, fight, move the camera, open the shop). It beeps when the
  capture ends; measuring takes a few minutes more. Send `results.txt`. Then: Settings >
  Recording > Video codec HEVC (and AV1 if offered), run the 5 s recorder test, play a short
  Practice Tool game and check the replay opens, seeks, steps frames, makes a thumbnail, the
  ult check runs, a hotkey clip and an exported clip play.
- **Owner, v1.8 part 1 in a real game (Practice Tool is enough, ~10 min):** Practice Tool on
  "Record". In base: buy a component, then a finished item and press UNDO right away; buy it
  again and wait 10 s; buy a second finished item, wait 10 s, then UNDO it; sell the first one.
  Flash and use your other summoner (also once by clicking its icon); press D again while it's
  on cooldown. Destroy a tower and take a camp/dragon with gold changes visible. End the game,
  wait ~1 min (the check runs after the game), open it. Expected: one "Completed <item 1>" chip
  at the purchase, none for the undone ones, the sale doesn't remove it; "Flash" / other spell
  chips at the cast frames (the clicked one too), none for the press on cooldown; the tower
  card says lane / tier / "Your gold ≈ +…"; a faint APM area behind the markers, hover shows
  the APM. Scoreboard (part 2): the card under the player shows all 10 players; drag along the
  timeline and watch levels / items / KDA change with it (compare a moment with the in-game
  Tab screen you remember, e.g. right after a purchase); press O over the video, and hold Tab
  in fullscreen. Then Settings > Advanced > Save test report.
- League and Deadlock only for now (no CS2 / Dota 2 work). Nice-to-haves: code-signing certificate for the installer;
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
