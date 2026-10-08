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
- **Focus: League of Legends only** (owner, 2026-10-07). No CS2 or Dota 2 work for now; the CS2
  module stays as it is.
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


## Next steps
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
