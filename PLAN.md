# Clairvoyance (formerly GameRecorder): Project Plan

> **For Claude:** read this whole file at the start of every session. After finishing a
> milestone, tick it off below, add any decisions to "Decisions made", and update "Current status".

## Current status
- **Renamed to Clairvoyance (2026-10-01)** and published on GitHub:
  https://github.com/Emre-98/Clairvoyance (public). Releases are built by GitHub Actions and
  installed copies **update themselves** (see "Updates and releases"). First release: v1.0.0.
- **Stack: Tauri 2 (Rust + Svelte 5 web UI).** The built-in recorder is the only recorder
  (Windows Graphics Capture + hardware H.264 via Media Foundation + per-process game audio,
  crash-safe fragmented MP4, in-memory replay buffer). OBS support was removed.
- Done on 2026-10-01 (owner's list): OBS removed; rename + one-time data migration; Dark /
  Light / Match Windows themes; storage limit with automatic clean-up (favorites and kept clips
  protected); thumbnails in their own folder, made after the game; performance work (library
  cache, virtualized grids, lazy thumbnails, 1 s keyframes, skeletons, transitions);
  auto-updater + release workflow + RELEASING.md.
- **Game modes (2026-10-01, v1.1.0):** choose per League mode (queue) whether it's recorded:
  Record / Clips only / Off, grouped (Ranked, Normal, ARAM, Arena, Rotating & event, Other,
  Unknown / new). The queue comes from the League client (LCU) at game start, before recording.
- Owner test of the 2026-10-01 build (job 1 on the owner's PC): migration from GameRecorder
  worked (settings, `Videos\GameRecorder` → `Videos\Clairvoyance`, app data); thumbnails were
  made for the 3 existing games by the new Media Foundation code; window ready ~540 ms after
  launch (old app ~526 ms; target < 1 s met by both); recorder self-test OK (NVENC, 0 dropped).
- Owner tests so far (2026-09-30, RTX 5080 / i9-9900K, 4K → 1080p60): self-test and a Practice
  Tool game recorded fine: 0 dropped frames, NVENC, game-only audio, ~0.5% CPU, ~180 MB RAM while
  recording, League FPS 140.7 → 140.3 average (−0.3%), 1% low 102.5 → 100.2.
- **Auto-update tested end to end (2026-10-01):** v1.0.0 installed from GitHub; v1.1.0 published by
  `scripts\release.ps1` + GitHub Actions; the installed app showed "Update available: v1.1.0"
  ~60 s after start, "Update now" downloaded, installed and restarted it on 1.1.0.
- **Instant replays (2026-10-01, v1.2.0):** measured in the real window on the owner's PC, 5 opens
  each, before → after: 33 min game first frame ~28.6 s (first open > 30 s) → 97–135 ms first
  open, 16–43 ms repeat opens; 5 min game ~4.0 s → 176–190 ms first open, 12–33 ms repeat; page
  + markers 20–60 ms both (target < 200 ms); marker jump 121–330 ms → 69–175 ms (keyframe
  snapping); marker clicked right away 4–29 s → 75–200 ms. Finalizing a 2.6 GB recording took
  3.5 s. Details in "Instant replays".
- **End-to-end tests (2026-10-01, sandbox profile on the owner's PC, simulated games + fake League
  client):** ARAM off → not recorded, status "ARAM: recording off for this mode"; Ranked recorded
  with queue 420 + "Ranked Solo/Duo" saved; switching Ranked off mid-game keeps that recording and
  the next Ranked game isn't recorded; Arena on Clips only → no full video, 2 clips (triple kill,
  ace); a brand-new queue (9999) → recorded by the "Unknown / new modes" rule and flagged "New";
  over the storage limit during a game → nothing deleted while recording, oldest game
  removed 24 s after the game; every recording finalized (faststart); app killed mid-recording →
  the game got its 34 s video back. Self-test: keyframes every 1.00 s (max 1.02 s) at 54.6 fps.
- **Accurate ult tracking (2026-10-01):** live filtering from League's own keybinds + level /
  cooldown / death gates, then a post-game check of every press against the R icon in the
  recording (hardware decoding, low priority). Owner's scripted Practice Tool test (Ashe, 141 key
  presses, 34 real casts): before 56 "Ult pressed" markers (22 false, precision 61%, recall 100%);
  after 34 "Ult used" (precision 100%, recall 100%, cast frame = ground truth to the frame), 107
  presses kept as hidden "no cast" (on cooldown 88, dead 12, chat 7). Owner's 3 real games: 24/24
  casts, the shop-typed "r" marked no cast (before: precision 96%). A 33 min game is checked in
  2.7–3.3 s inside the app (target ≤ 15 s). Details in "Ult tracking".
- **Input tracking + replay overlay (2026-10-01/02, v1.4.0):** mouse/keyboard recorded during
  cursor-based games (League) and drawn over the replay (trail, clicks, cursor dot, keys,
  heatmap; toggle "Input overlay" / I), plus post-game "Mechanics" stats. Tested on the owner's
  PC: SendInput into a fake game window recorded by the real recorder: cursor samples median
  0.01 px from the true path, clicks median 1-2 ms / max 10 ms, focus gap and window moves
  logged, 100 % (DPI-unaware window) and 150 % correct, recorded cursor vs the cursor in the
  video frames: best fit -5..-6 ms (< 1 frame). Ult check unchanged on the saved clips (9/9) and
  on the owner's 5 games (identical counts). Replay open time unchanged (page 19-23 ms vs 20-24
  ms on v1.3.0, first frame same); overlay: off on every open, re-on 10-18 ms, first on 66 ms (2
  min game) / 125 ms (synthetic 33 min with the cursor moving non-stop; before the i16 payload),
  draw p95 0.2-0.4 ms, 0 stalls toggling at 0.25x/1x/2x. **Performance test in League (Practice
  Tool, 2026-10-02, real play):** not recording 143.8 FPS / 1 % low 110.1, recording 142.3 /
  83.7, recording + input 143.7 / 109.3 (-0.07 % vs not recording; target < 1 %); app CPU
  0.46-0.65 % with input vs 0.55-0.80 % without (no measurable difference), RAM +1-6 MB; capture
  thread 0.26-0.41 % of one core in play (cursor moving most of the time), 0.10 % with the mouse
  at rest. Owner's check: the trail follows the cursor in the replay. Details in "Input tracking".
- **Ability bubbles on the input overlay (2026-10-02, v1.5.0):** every ability / summoner /
  item / ward key press pops up as a bubble on the replay at the exact cursor position
  (interpolated) on the exact video frame of the key-down, then fades (0.1-3 s). Binds from
  League's own settings (same reader as the ult tracking), saved with each game. Tested (details
  in "Ability bubbles"): Rust 107 tests (89 before), Playwright 18/18 bubble pixel checks (anchor
  0.32 px from the expected pixel, on the key-down's frame and not the one before, fade 0.1 s
  and 3 s, Q spam + W readable), v1.4 overlay pixel-identical with bubbles off (16/16), on the
  owner's PC: his real binds read correctly, all 91 R presses of his 4 real games agree with the
  ult check, ult results unchanged on his 5 games, replay open time unchanged (first frame 36 ms
  median before and after), spam with 44 bubbles on screen drawn in p95 0.4 ms.
- **Waiting for the owner**: the Practice Tool test of the ability bubbles (see "Next steps"),
  and a real League game with v1.4.x/v1.5.x: timeline, thumbnail and Settings > Advanced >
  Responsiveness numbers (see "Known issues").

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

## Decisions made
### Stack: Tauri 2 (Rust backend + Svelte 5/TypeScript UI in WebView2)
Compared:
- **C#/.NET 10 + WPF**: mature and easy Windows APIs, but the .NET runtime + WPF stay loaded
  (~80–150 MB RAM) even when minimized, a self-contained exe is 70–150 MB, video playback needs
  LibVLC (another ~100 MB), and an Ascent/Outplayed-style UI is slow to build in XAML.
- **Tauri 2 (Rust + web UI)**: the Rust process idles at ~10–30 MB RAM and ~0% CPU; the WebView2
  UI can be *destroyed* while you play (its ~100 MB comes back only when you open the window);
  HTML/CSS gives a polished gaming UI and a built-in video player (HTML5 `<video>`); the
  installer is ~2.5 MB; WebView2 is part of Windows 11. Cost: Rust compile times.
- **Qt (C++/QML)**: fast, but a heavy toolkit, LGPL packaging hassle and slower development.
- **Electron**: excluded (ships a whole Chromium, heavy in RAM).
- **Chosen: Tauri 2.** Best in-game footprint (window closed = only a small Rust process), best
  UI tooling, smallest download for friends.
  (Practical bonus: it can be cross-built and tested end to end on Claude's Linux build machine.)

### Architecture (ideas kept from the C# skeleton)
- `crates/cv-core`: game-agnostic core. `GameIntegration` trait (one module per game),
  `Recorder` trait, `GameEvent`/`EventKind`, `GameSession` (with `video_offset`), settings,
  library/retention, and the **engine** (state machine: idle → game detected → recording → saved).
  No game-specific code.
- `games/league`, `games/cs2`: one crate per game. `app/src/games.rs` registers them.
- `app`: Tauri shell (tray, windows, commands for the UI, Win32 bits). `ui`: Svelte front end.

### Built-in recorder (crate `cv-capture`, default)
- Implements the `Recorder` trait from `cv-core`, so the engine can be tested with a fake.
- **Capture: Windows Graphics Capture** of the game window (free-threaded frame pool, no yellow
  border, cursor on), with monitor capture as fallback (setting, window not found in 8 s, or no
  frames for 6 s). No hooks, no DLLs, nothing in the game process. Frames are capped at the target
  fps in the capture callback.
- **Colour conversion + scaling on the GPU:** D3D11 video processor BGRA → NV12 (BT.709 limited),
  letterboxed to the output size, into a ring of 8 textures. Frames are dropped (never queued up)
  if the encoder falls 5 behind, so the game can't be slowed down.
- **Encode: Media Foundation hardware H.264 MFT** (async, D3D11-aware) on the same GPU as the
  capture, preferring the vendor's own (NVENC → AMF → Quick Sync by adapter). VBR, GOP = 1 s of
  frames plus a forced keyframe whenever 1 s of *time* has passed without one (games don't always
  deliver the full frame rate, which stretched keyframe gaps to 5 s), no B-frames, low-latency mode. Bitrate: 12 Mbps (Standard) / 20 Mbps (High) at 1080p60, scaled by
  pixel rate. Refuses software encoders.
- **Audio:** game audio through **WASAPI process loopback** (`ActivateAudioInterfaceAsync` with
  `PROCESS_LOOPBACK`, include process tree), so Discord/Spotify aren't recorded. Falls back to
  desktop loopback on Windows builds without process loopback (before Win10 2004). Optional mic
  on its own track (setting "Record microphone"). 48 kHz stereo → AAC 160 kbps through MF.
  Timestamps come from counted samples; gaps are filled with silence and drift is corrected
  against the video clock (QPC).
- **Container: own fragmented-MP4 muxer** (pure Rust, `mp4.rs`): a `moof`+`mdat` fragment every
  ~1 s, flushed to disk every 10 s, `mfra` index at the end. A crash or power loss only loses the
  last seconds; the rest plays. Chosen over MF's sink writer, which only writes a playable file on
  a clean close. After the game the file is **finalized** to a faststart MP4 (see "Instant
  replays").
- **Replay buffer in memory:** the last N seconds of encoded packets (capped in bytes), trimmed at
  keyframes. Saving a clip writes a normal MP4 (moov first) from it, with no re-encode.
- Encoders that only publish SPS/PPS in the output format (not in-stream) get them prepended to
  keyframes from `MF_MT_MPEG_SEQUENCE_HEADER`.
- Crash logging: a panic hook and an unhandled-exception filter write the thread and exception
  code to the log before the process dies (release builds use panic=abort).
- Thumbnails are no longer taken during the game (see "Thumbnails" below).
- `recorder-selftest.exe` (dist\tools) records a few seconds and writes a report; the UI has the
  same 5 s test in Settings > Recorder.

### Performance test (Settings > Performance test)
- Runs in a real game: waits until League is in game, then records phases of N seconds each
  ("Not recording", "Built-in recorder") with voice callouts, and compares them.
- FPS from **PresentMon** (downloaded on demand from its GitHub releases, runs elevated, only
  watches `League of Legends.exe`): average, 1% low, 99th percentile frame time.
- CPU/GPU from Windows performance counters (PDH): total and game CPU, GPU 3D and video-encode
  engine use, GameRecorder's own CPU and RAM.
- Report saved as JSON + text in `<save folder>\perf-tests\`; test recordings are deleted and
  settings restored afterwards.

### OBS removed (2026-09-30, owner's request)
- The OBS backup (obs-websocket client `gr-obs`, OBS setup, `RecorderSwitch`, Settings > Recorder
  choice) is gone; the built-in recorder is the only recorder. Old settings files still load
  (unknown fields are ignored) and the leftover `obs` / `recorder` fields are removed from the
  file on the first start.

### Game-clock ↔ video offset
- `video position = game time + video_offset`. Each poll right after reading `gameTime`, the
  engine asks the recorder for its own recorded duration; `offset = recorded - gameTime`. The median of
  the first 9 samples is used (robust against one slow request). Key presses use the same clock.

### Events
- League: EventID dedupe (module) + id dedupe (engine). Player found automatically via
  `/activeplayername` (the Riot ID setting is a fallback); names matched with and without `#TAG`.
  Victim/killer names shown as champions; turrets/minions/monsters get readable names.
- Ult: **Windows Raw Input**, not a keyboard hook (can't add input lag, doesn't touch the game),
  registered only while a game runs, only counted while the game window is focused. Filtered
  live and checked against the recording after the game: see "Ult tracking".
- Hotkeys use the same Raw Input path, so they never steal the key from the game.
- After a League match ends the process often stays open (victory screen): the engine won't
  start a new session until it exits.
- **Match-only games** (`GameIntegration::match_only`, used by CS2): recording starts when a
  match starts and ends when it ends or you leave it; no recording in menus.
- Auto clips: after the game, windows around chosen event kinds (default: multikill, ace) are
  merged and cut with ffmpeg **stream copy** (no re-encode, low priority).
- Text-to-speech callouts via Windows SAPI (off by default), per event kind.

### Storage
- `Videos\Clairvoyance\<date>_<time>_<Game>\` per game: video
  (`2026-09-30_League_Ahri_Win.mp4`, renamed after the match), `session.json`, `clips\`.
  The folder name is the recording id.
- Settings: `%APPDATA%\Clairvoyance\settings.json`. App data `%LOCALAPPDATA%\Clairvoyance\`:
  `logs\`, `Thumbnails\`, `library-cache.json`, `cleanup-log.json`, `ffmpeg\`, `tools\`
  (the per-user installer also puts `Clairvoyance.exe` there; its uninstaller keeps the data).
- Retention: see "Storage limit and clean-up" below.

### UI
- Frameless dark window with custom title bar; sidebar (Home, Games, Clips, Settings) with a
  live status card and the CPU/RAM debug stat. Own logo/branding.
- Timeline marker colours as specified; every marker also has its own icon so colour is never
  the only cue (the colours alone aren't colour-blind safe). Stolen objectives get a badge.
- Champion portraits come from Riot's public Data Dragon CDN, only while the window is open.
- **Responsiveness (owner's request, 2026-09-30):** the window is no longer closed when a game
  starts (you can alt-tab to it in the loading screen; setting to close it is still there, off).
  Closing it hides it to the tray with the UI kept loaded, so it reopens instantly; when starting
  in the tray it's preloaded invisibly. While hidden, the WebView is set invisible, its page
  suspended (`TrySuspend`, no scripts/timers) and its memory target set to low; live events are
  skipped and a `resync` event catches it up when shown. Setting "Keep GameRecorder ready in
  the background" off restores the old behaviour (window destroyed on close).
- UI speed: game pages are fetched on hover and cached (open instantly), Clips/Home keep their
  data between visits, pages remember their scroll position, disk-touching commands run off
  the UI thread (they used to be synchronous Tauri commands on the main thread), and polling
  stops while the page is hidden. Closing never stops recording; tray > Quit ends any
  recording cleanly first.

### Packaging
- Releases: GitHub Actions (MSVC build on windows-latest, Tauri's NSIS bundler, signed updater
  artifacts). See "Updates and releases".
- Test builds by Claude: cross-compiled from Linux (`scripts/build-windows.sh`, distro rustc 1.91
  with `-Zbuild-std` for `x86_64-pc-windows-gnu`; the gnu exe needs `WebView2Loader.dll` next to
  it). The Tauri CLI can also bundle the NSIS installer from Linux (with a `rustup` shim, see
  `scripts/setup-cross-linux.sh`).
- Dev tools: `Clairvoyance.exe --simulate` / Settings > Advanced plays a fake League match.

### Rename to Clairvoyance (2026-10-01)
- Product/exe/window/tray/installer name Clairvoyance, Tauri identifier `app.clairvoyance.desktop`,
  crates `cv-*` (were `gr-*`), app package `clairvoyance`, UI package `clairvoyance-ui`.
- `app/src/migrate.rs`, first start only (when the new settings file doesn't exist and the old
  one does): copies `%APPDATA%\GameRecorder\settings.json`; renames `Videos\GameRecorder` to
  `Videos\Clairvoyance` when the default folder was used (if the rename fails, e.g. a file is
  open, the settings point at the old folder instead: nothing is ever lost; custom save folders
  are kept as is); moves or copies `%LOCALAPPDATA%\GameRecorder` (ffmpeg, PresentMon, logs);
  re-registers "Start with Windows" (the old Run value is removed). CS2's old GSI cfg is replaced.
- The old install is detected (HKCU uninstall key) and Home shows a banner to uninstall it
  (runs its uninstaller silently; it never touches recordings or settings).
- The project folder on the owner's PC is still called `GameRecorder`; renaming it to
  `Clairvoyance` is harmless (nothing depends on the folder name) and can be done any time.

### Themes (2026-10-01)
- Setting `theme`: "system" (default, follows Windows live), "dark", "light".
- All colours are CSS custom properties in `ui/src/app.css`, written with `light-dark()` so one
  `data-theme` attribute + `color-scheme` switches everything in one frame. Components contain
  no hardcoded colours (media overlays on thumbnails/video use their own always-dark tokens).
- No flash: Rust injects `window.__CV_THEME__` as an initialization script, `public/theme-boot.js`
  applies it before first paint, and the native window background matches the theme.

### Storage limit and clean-up (2026-10-01)
- `max_disk_gb` (default 100; old files with "no limit" get 100 once), `auto_cleanup` (on),
  optional `auto_delete_days`. `library::plan_cleanup` deletes oldest first; favorites are never
  touched; a game with clips marked "keep" (`ClipInfo.keep`) only loses its full video
  (`video_removed_at`), keeping timeline + clips. If only protected games are left and it's
  still over the limit, a warning is shown instead.
- Runs in `app/src/maintenance.rs`: 20 s after start-up, after each game once its auto clips are
  cut (`EngineEvent::PostProcessed`), after storage settings change, and "Clean up now". Never
  while a game runs/records (checked before and between steps) and on a background-priority
  thread. Toast + `cleanup-log.json` (shown in Settings > Storage).

### Thumbnails (2026-10-01)
- `%LOCALAPPDATA%\Clairvoyance\Thumbnails\<recording id>.jpg`, clips `<id>@<clip>.jpg`, 480 px
  JPEG (WIC has no WebP encoder). Made after the game by decoding one frame (~90 s into the match)
  of the finished file with Media Foundation's source reader (`cv-capture/src/win/thumb.rs`),
  ffmpeg as a fallback. The in-game screenshot was removed (no work during the game).
- Missing thumbnails are regenerated by the maintenance pass; old `thumb.jpg` files move into the
  folder; deleting a game/clip (by hand or by the clean-up) deletes its thumbnails; orphans are
  removed.

### Responsiveness (2026-10-01)
- `LibraryIndex` (cv-core): summaries + clips in memory and in `library-cache.json`; the first
  list after start-up answers from the cache and refreshes in the background; later refreshes
  only re-read folders whose `session.json` / folder / `clips` folder changed. No scan during a
  game (a dirty flag defers it).
- UI: virtualized grids (`VirtualGrid.svelte`), lazy `<img>` thumbnails with skeletons, clip
  previews only for the hovered card, cached Intl formatters, page transitions and press
  feedback with transform/opacity only (150-250 ms), skeleton loaders.
- Keyframe every 1 s (was 2 s) so seeking decodes less.
- Settings > Advanced shows measured startup, page-switch and timeline-jump times from the real
  window; the app logs "startup: window content painted N ms after launch".
- Measured (Chromium, 500 games, same mock data, before → after): Games page switch
  234 → 26 ms (4x CPU throttle: 1186 → 112 ms); DOM nodes on the Games page 8,581 → ~650;
  scrolling at 4x throttle 28 → 50 fps (unthrottled 60 fps both); timeline jump (seek to
  painted frame) median 17-45 ms → 33-50 ms, always < 100 ms. Window-open numbers on the
  owner's PC: see "Known issues" / owner test.

### Updates and releases (2026-10-01)
- `tauri-plugin-updater`, endpoint `https://github.com/Emre-98/Clairvoyance/releases/latest/download/latest.json`,
  public key in `app/tauri.conf.json`, NSIS passive install + restart. Checks 60 s after start
  and every 4 h, never while busy; `auto_update_check` setting; nothing downloads until "Update now".
- Releases: `scripts/release.ps1 <version>` bumps versions + CHANGELOG, tags and pushes;
  `.github/workflows/release.yml` (windows-latest, tauri-action) builds, signs with the
  `TAURI_SIGNING_PRIVATE_KEY(_PASSWORD)` secrets and publishes installer + .sig + latest.json.
  `ci.yml` runs tests + UI checks on pushes. See RELEASING.md.
- Private key backup on the owner's PC: `Documents\Clairvoyance-signing-key\` (never in the repo).
- Packaging moved from the hand-written NSIS script to Tauri's bundler (per-user install,
  WebView2 bootstrapper, Start menu entry). `scripts/build-windows.sh` is still the Linux
  cross-build for test builds; `scripts/setup-cross-linux.sh` prepares that toolchain.

### Game modes: per-queue recording rules (2026-10-01)
- Detection (League, `games/league/src/queues.rs`), once per game, in `detect_mode()` right after
  the game process appears (loading screen) and before recording starts:
  1. League Client API (LCU): port + password from `<install>\lockfile` (install folder from
     the setting "League install folder", `C:\ProgramData\Riot Games\RiotClientInstalls.json`,
     the Riot metadata yaml, then `C:/D:/E:\Riot Games\League of Legends`);
     `GET /lol-gameflow/v1/session` → `gameData.queue` (id, description, type, isRanked,
     gameMode). Practice Tool / custom games get the keys `practice` / `custom`.
  2. Fallback: Live Client Data API `gamestats.gameMode` (coarse: CLASSIC/ARAM/CHERRY/...): the
     most permissive rule among known modes of that game mode is used.
  3. Still unknown: the "Unknown / new modes" rule. The source used is logged ("mode: ... via").
  Up to ~3 × 0.7 s of retries, only at game start; nothing extra runs during the game.
- Mode list (`mode_catalog()`), refreshed by the maintenance pass (start-up, after games, every
  6 h, never in game) and when Settings > Game modes opens: the client's
  `/lol-game-queues/v1/queues` (names in the client's language + `queueAvailability`,
  authoritative: queues it doesn't list are "not currently available") and Riot's official
  `queues.json` (deprecated ones skipped), both cached in `%LOCALAPPDATA%\Clairvoyance\cache\`
  for offline use. Only `queues::KNOWN` (a 9-line table of well-known queue ids → group,
  default rule, fallback name) is built in; everything else is classified from the queue's own
  data (isRanked/type/category/gameMode/map).
- Settings (`settings.modes["league"]`, `cv-core/src/modes.rs`): per key (`q<queue id>`) name,
  group, rule, availability, `is_new`. New queues (from the list or first seen in a game) are
  added with the unknown rule and a "New mode detected" badge (the very first list isn't flagged);
  gone ones stay, greyed, with their choice. Defaults: Ranked/Normal/ARAM/Arena record,
  Practice Tool + Custom off, Co-op/Tutorial/rotating follow the unknown rule (record).
  Changes are saved at once by dedicated commands (`modes_set`) and sent to the engine: they
  apply to the next game. The Settings "Save changes" draft never overwrites them.
- Engine: Off → no recorder, no polling (only the process is watched), nothing saved; the status
  message / tray says e.g. "ARAM: recording off for this mode". Clips only → the recorder keeps
  the replay buffer but writes no full video (`RecordOptions.full_video = false`); hotkey clips
  work, and auto-clip event kinds are saved from the replay buffer a few seconds after the event
  (overlapping events merged). Record → as before.
- Every recording's `session.json` has `queue_id`, `mode_name`, `mode_key`, `record_mode`; the
  library shows the mode as a tag and can filter by it. Older recordings use their old mode label.
- Events work in every mode (they come from the Live Client Data API); modes without Baron etc.
  simply have no such markers.

### Instant replays (2026-10-01, owner's request)
Problem: opening a game showed a black player for seconds (28 s for a 33 min game) before the
video or the markers worked. Measured, not guessed (`scripts`/jobs + `--bench-replays`, see
"Replay benchmark" below):
- **Cause 1, the big one: the MP4 layout.** The recorder writes crash-safe *fragmented* MP4
  (a `moof`+`mdat` every second, no full index up front). Chromium's demuxer (FFmpeg's `mov`,
  which ignores the `mfra` index by default) reads **every** `moof` before the first frame:
  1,957 reads spread over 2.6 GB for the owner's 33 min game, 298 for a 5 min one. Reproduced
  on Linux with ffmpeg over a range-logging HTTP server: 1,977 range requests vs 1 for the same
  media as a faststart file.
- **Fix: finalize after the game.** `cv_capture::remux` rewrites the recording as a faststart
  MP4 (ftyp, full `moov` with stts/stss/stsz/stsc/stco-or-co64, then one `mdat`), copying the
  media as is (no re-encode, ~3.5 s for 2.6 GB on the owner's PC). Timestamps are kept exactly
  (a late first sample covers the gap from 0). Done by the maintenance pass after each game
  (after the auto clips), newest first, and for all older recordings; never while a game runs
  (the copy stops and the original stays), never under the open player, only with enough free
  space; written to `<video>.mp4.finalizing`, re-parsed and checked, then swapped in.
  Recording stays fragmented (crash safety); interrupted finalizes leave the original untouched.
- **Cause 2, checked: serving.** Tauri's asset protocol already answers HTTP Range requests (206,
  ≤1 MB per response), so only the needed parts are read; nothing to change. The video URL now
  carries `?v=<file size>` so the player never mixes cached bytes of the file before and after
  it was finalized.
- **Cause 3, checked: codec.** H.264 High (avc1.64002a), hardware-decoded by WebView2
  (`mediaCapabilities`: supported, smooth, powerEfficient). HEVC/AV1 not used.
- **Cause 4: keyframes.** Real recordings had keyframes every 2.0–2.3 s on average and up to
  5.5 s apart (GOP counted in frames at ~52 fps real). Now: forced keyframe every second of time
  (new recordings), and marker jumps **snap to the keyframe before the target** (0–3 s earlier
  than "5 s before", from `video_keyframes`, cached per file), so the frame shows without
  decoding the frames in between, also for old recordings.
- **Cause 5: player setup.** The page waited for the game data before creating the player, the
  `<video>` was re-created on every open, and there was no poster. Now: one shared `<video>`
  element for the whole app (`ui/src/lib/videopool.ts`), moved into the page and kept with its
  data afterwards; hovering a game card for 120 ms starts loading its video (preload=metadata);
  the page renders from the library summary at once (header, player, poster) while the events
  load in parallel; the thumbnail is the poster with a small spinner until the first frame;
  marker clicks before the video is ready are remembered and done as soon as it can seek (the
  playhead moves at once).
- Interrupted games (crash, power cut, killed app) get their partial video back: the maintenance
  pass links the leftover recording to the game (with a warning) and finalizes it.

### Ult tracking (2026-10-01, owner's request)
Rules: nothing may cost in-game FPS / CPU / RAM / input latency; no memory reading or injection
(only the Live Client Data API, League's config files, Data Dragon and our own recording); video
analysis only after the game in the maintenance pass, stopped when a game starts; nothing shown
in game.
- **Layer 1, live (`games/league/src/ult.rs`):**
  - Keybinds from League's `Config\PersistedSettings.json` (authoritative; falls back to
    `input.ini`): smart-cast, cast-with-indicator, self-cast, normal and plain cast of spell 4,
    several binds per action, modifiers, mouse buttons. Owner's PC (16.19): `[r]`, `[Alt][r]`,
    `[Shift][r]`; `[Ctrl][r]` = level up (ignored). Settings > Games > League > Ult key: empty
    (or the old "R") = League's binds; any other key = manual override.
  - Gates (from `/activeplayer` each poll, `/playerlist` only while dead): R not learned
    (`abilityLevel` 0) → ignored; dead → ignored unless `usable_while_dead` (Karthus); chat open
    (Enter/Escape) → ignored; on cooldown → ignored. Cooldown = Data Dragon base cooldown for the
    patch and rank × 100 / (100 + ability haste); presses inside 85% of it after the last
    accepted press are filtered. Data Dragon is fetched in the background at the loading screen
    and cached per patch (`<data>\cache\ddragon\<ver>\<Champ>-R.json`); until it arrives the
    cooldown filter is off.
  - `games/league/ult_rules.json` (built in; `<data dir>\ult_rules.json` overrides it, no code
    change needed): champions with recasts / charges / transforms / resets / stolen ults skip the
    cooldown filter; per-champion delay from press to cooldown; the game modes where the cooldown
    math holds (CLASSIC, ARAM, SWIFTPLAY, PRACTICETOOL; Arena/URF: filter off).
  - Every press (accepted or not, with its reason) is kept in `session.json` `key_presses` for
    layer 2; accepted ones show as "Ult pressed" until the check has run.
- **Layer 2, after the game (`hud.rs`, `verify.rs`, `cv-capture/src/win/frames.rs`):**
  - Finds the game image (letterbox) and the ability bar in 24 frames spread over the game:
    the bottom HUD is anchored at the bottom centre and scales with the window height × HUD
    scale; Q W E R are 38 px squares, 44 px apart at 1080 lines and HUD scale 0. The scale is
    searched (0.8–2.6) by the contrast of the four icon borders against the gaps; confidence from
    the score and how unique it is. No bar / low confidence (other mode, other HUD) → the check
    is skipped, live presses stay, labelled "Ult pressed (unverified)". No mode list.
  - Full scan: the R icon on every keyframe (no other frame decoded): blue cooldown overlay
    share, white digit share, dark share → ready / cooldown / dimmed. A cast = the overlay
    appearing (or coming back over a mostly-uncovered icon) compared with the last non-dimmed
    look; "dimmed" (stun, silence, shop open, dead, not learned) is skipped, so the cooldown
    reappearing after a stun isn't a cast. Then the frames between the two keyframes are decoded
    for the exact cast frame; presses with no cast nearby get their own window (−0.3 s …
    +delay) so short ready windows aren't missed.
  - Matching: a press up to the champion's delay before the cast (closest; first press of a burst
    for recast champions) → **"Ult used"** at the cast frame. A cast without a logged press →
    "Ult used" (mouse / other bind). A press without a cast → **"Ult pressed, no cast"** with the
    live reason, in the "Unconfirmed presses" filter chip, hidden by default.
  - Decoding: Media Foundation with a D3D11 device manager (DXVA hardware decoding), low-latency
    mode, GPU thread priority −7; only the requested region is copied from the decoded NV12
    texture to a small staging texture and converted (BT.601 limited, same code as the Linux
    tests). Software decoder fallback. Thread in background mode.
  - Maintenance: newest first, only finalized videos, re-run when `verify_version` grows
    (`verify::VERSION`), stops as soon as a game starts (done next run), re-reads session.json
    before saving so user edits made meanwhile are kept. Result in `session.json`
    `verification` (status, confidence, counts, time, geometry) and a line on the game page.
- **Measured (owner's PC, RTX 5080):** in the app, 33 min game 3.3 s (verify 2.7 s), 23 min
  2.9–3.1 s, 2 min 1.5 s; ~780 keyframe seeks + ~1,800 decoded frames for 33 min. The owner's
  recordings from 2026-09-30 have keyframes every 2.0–2.5 s (newer ones: 1 s).
- **Tests:** unit tests (input.ini / PersistedSettings parsing, cooldown math, rules file, gates,
  matching, stun handling, HUD location incl. a synthetic 1.5× HUD with letterbox, R states on
  real crops in `games/league/tests/hud/`); `tests/verify_clips.rs` (ignored by default, needs
  the owner's clips, `CV_SAMPLES` / `CV_SAMPLES2`): 12 clips + 7 tricky windows → 9/9 casts,
  0 extra, decoys and a press in the shop end up "no cast". Developer tool:
  `mp4tool ultcheck <session dir> [--write]`.
- **Owner test (2026-10-01, Practice Tool, Ashe, 2 min recording):** the owner had Practice
  Tool's No Cooldowns on, so the ult came back within ~1 s: 141 presses (R spam, Alt+R cancels,
  chat, dead), 34 real casts (ground truth: every frame of the R icon, checked by eye on a frame
  sheet).
  | | markers | false | missed | precision | recall | time error |
  |---|---|---|---|---|---|---|
  | before (every R, 0.75 s debounce, chat) | 56 | 22 | 0 | 61% | 100% | press time (median 22 ms, max 0.93 s) |
  | live layer only | 2 | 0 | 32 | 100% | 6% | — |
  | after (layer 2) | 34 | 0 | 0 | 100% | 100% | 0 frames |
  Lessons, fixed: (1) Ashe's ult icon is blue itself, so a cast is also "the cooldown number
  appears" (`hud::is_cast`); (2) No Cooldowns makes the live cooldown filter wrong, so Practice
  Tool is no longer in `cooldown_filter_game_modes`; (3) the app was restarted mid-game and the
  engine clamped the video offset to 0 (should be −30 s): negative offsets are now kept, presses
  outside the video stay "unverified", and an unreadable stretch no longer fails the whole check
  (it failed with MF_E_INVALID_POSITION on a seek past the end). Check time on the PC: 4.4 s for
  this game (107 press windows). `verify::VERSION` = 2, so older checks are redone.
- **Performance:** the live part only adds `/activeplayer` per poll (and `/playerlist` while
  dead); the check runs only after the game. Previous in-game numbers: 140.7 → 140.3 FPS, 1% low
  102.5 → 100.2 (recording on vs off). A performance test with the ult build is still to do.

### Input tracking + replay overlay (2026-10-01, owner's request, MicroLab-style)
Rules: nothing in or near the game process (no hooks, no injection, no memory reading); nothing
shown during the game; heavy work only after the game; privacy (focused window only, no keys
while the chat is open, key codes only, local only).
- **Core, game-agnostic (`cv-core/src/input/`):** `GameIntegration::input_tracking()` (League
  true, CS2 false), `chat_open()` (League: the ult tracker's Enter/Escape chat state) and
  `mouse_marks()`. The core adds the settings "Record mouse & keyboard input" (default on) and
  "Cursor sample rate" 125 / 250 (default) / 500 Hz to Settings > Games for cursor-based games
  (`input::config_fields`, new config field kind "select").
- **Clock:** `Recorder::clock_base_hns()` = the QPC (100 ns) the recorder subtracts from frame
  times (`rec_start`), so input time = QPC - base = video time. App started mid-game (negative
  offset) needs nothing special: the input never uses the game clock (only the per-minute APM
  chart converts to game minutes).
- **Capture (`cv-capture/src/win/input.rs`, thread "input-capture", below-normal priority, DPI
  aware per monitor v2):** a high-resolution periodic waitable timer (no change to the system
  timer) wakes it at the sample rate while the mouse moves: `GetCursorPos` (stored only when it
  moved, in 1/65536 of the client area) and the five mouse buttons with `GetAsyncKeyState`
  (physical state + "pressed since last call" bit, so a click shorter than a tick isn't missed;
  a change gets the middle of its tick as time); foreground check every tick (focus records =
  gaps); client rect / DWM frame / DPI every 500 ms and on refocus (window records). After 250 ms
  without movement it drops to a 30 Hz `GetLastInputInfo` check; outside the game 10 Hz. The
  game window is found once (every 2 s until found).
- **Decision (measured, owner's PC, 2026-10-01): no mouse Raw Input.** The first version
  registered the mouse for Raw Input (`RIDEV_INPUTSINK`) as specified, but every movement report
  then has to be read: 7.7 µs per report (`GetRawInputBuffer`; `PeekMessage` + `GetRawInputData`
  16.9 µs), i.e. ~0.8 % of a core with a 1000 Hz mouse in motion, vs 0.46 µs per tick for the
  five `GetAsyncKeyState` calls (`GetCursorPos` 5.5-7 µs, `GetForegroundWindow` 0.4 µs). Same
  safety (no hook, nothing in the game), ~17x cheaper; the wheel isn't recorded (the format
  keeps a wheel record for later). Capture thread cost, exact (`QueryThreadCycleTime`; Windows'
  tick-sampled thread times over-count short frequent wake-ups): mouse at rest 0.10 % of one
  core; mouse moving non-stop (SendInput at 500 Hz) 0.18 % at 125 Hz, **0.27 % at 250 Hz**,
  0.50 % at 500 Hz, same under game-like CPU load; almost all of it is `GetCursorPos` itself.
  Before: 0.6-1.3 %.
- **Keys:** the existing Raw Input keyboard thread now sends every key down/up (auto-repeat
  filtered, QPC timestamp) to the engine. Hotkeys and `on_key` see exactly what they saw before
  (named key-downs), so the ult tracking and `key_presses` are unchanged. The engine records a
  key only while the game window is focused and the game's chat is closed before and after it
  (so the Enter that opens/closes chat and everything typed are never stored; a "chat" record
  marks the interval); a release is only stored after its press.
- **File `<recording id>.input`** (game folder, linked in `session.json` `input_file` at once):
  header `CVINPUT\0` + version + flags + rate, then blocks (kind, length, CRC-32): records
  written every 10 s (each block self-contained: absolute time + position, then varint deltas),
  so a crash keeps everything but the last seconds and a torn block is skipped. After the game
  the maintenance pass (after the ult check, newest first, stops when a game starts, background
  priority) computes the Mechanics stats (`session.json` `mechanics`) and the whole-game
  heatmap and rewrites the file deflate-compressed with the heatmap block (checked, then
  swapped in). Worst case 35 min (cursor moving the whole time at 250 Hz, 2.5 clicks + 1.5
  keys/s): 3.57 MB raw, 1.77 MB compressed; on the owner's PC (33 min, 509k records) 2.88 MB raw,
  1.43 MB compressed, stats + heatmap + compression 0.75-0.85 s. Owner's real 16.5 min game:
  92k cursor samples, 1,149 clicks, 1,946 key events, 541 KB raw. RAM while recording: the 10 s
  buffer (16 KB at 250 Hz) + the list of button presses (tens of KB).
- **Ult tie-in:** at the end of the game the mouse-button presses go to
  `GameIntegration::mouse_marks`; League keeps those on buttons bound to the ult (from
  `PersistedSettings.json`, "Button 4/5") as ult key marks (`key` "Mouse 5", reason "mouse",
  not accepted live), so the recording check matches them to casts. `verify::VERSION` not
  bumped: results of existing recordings can't change (they have no input file).
- **Storage:** the file is in the game folder, so deleting a game, the age/size clean-up and
  favorites handle it; "delete the video, keep the clips" also deletes the input file (its
  stats stay) and counts its bytes; interrupted games keep their raw file (torn tail ignored)
  and get it processed like any other; "Save test report" lists each recent game's input file
  summary and stats, never the file.
- **Replay overlay (`ui/src/lib/inputoverlay.ts`, `InputOverlay.svelte`, `Player.svelte`):**
  "Input overlay" chip in the player controls + **I** (works anywhere on the page except text
  fields), options popover (trail 0.25-3 s, clicks, cursor dot, keys strip, heatmap whole game /
  selected range; remembered in localStorage; the toggle itself always starts off). Disabled
  with "No input recorded for this game" for older games. The recording is loaded only on the
  first switch-on (`input_load`: Rust decodes it and sends flat typed arrays, cached for the open
  replay, freed when it closes); the canvas exists only while on; rAF loop redraws only when the
  time/size/options change. Mapping: client -> captured frame (window record) -> the recorder's
  letterbox in the video -> object-fit in the player. Heatmap: one-hue ramp, blurred, cached per
  range.
- **Mechanics (`Mechanics.svelte`):** APM, right-clicks/s, cursor distance (screen widths),
  path efficiency (straight line between consecutive clicks ÷ path, clicks ≤ 3 s apart on one
  stroke), idle time (no input > 1 s, focused time only); per-minute APM bars; drag across them
  for a range (`input_stats`, computed from the recording) which also drives the overlay's
  "selected range" heatmap.
- **Performance test:** three phases now: not recording, recording, recording + input (the
  capture thread's own CPU time, samples and file size are added to the report notes). Fixed on
  the way: a game already being recorded when the test starts made it fail ("already
  recording", and left PresentMon running so the next run had no FPS): the test now asks to
  start a new game; PresentMon gets its own session name + `--stop_existing_session`; the CSV is
  re-read for up to 20 s while PresentMon finishes writing it.
- **Overlay data:** positions sent as i16 (1/16384 of the client) and stroke breaks as an index
  list (v2 payload, ~4 MB for the 33 min worst case instead of 6.6 MB); the analysis no longer
  sorts the whole file (records are in order per thread): read + decode + payload 36 ms for
  509k records (was 100 ms).
- **Tests:** `cargo test -p cv-core` (format round trip, torn tail, chat/keys, stats, idle,
  efficiency, negative offset, payload layout, 35 min size targets, library clean-up of input
  files, engine end to end with chat + mouse marks); League (mouse marks, matching);
  `ui/tests/overlay.test.mjs` (Chromium + mock backend + a 4:3 test video with the game
  letterboxed: alignment, toggling while playing, draw time, options, shortcut, fullscreen,
  ranges, disabled state); `cv-capture/examples/inputtest.rs` on Windows (SendInput along a
  known path into a fake game window recorded by the real recorder: file contents, DPI aware /
  unaware windows at the monitor's scaling, click timing, focus gap, window move, cursor in the
  video frames vs the recorded cursor and the best-fit time shift, capture cost);
  `--bench-replays` with `"overlay": true` (first switch-on, re-on, toggling while playing,
  draw time, marker jumps with the overlay on); `mp4tool fakeinput` makes a realistic recording
  for a benchmark game.

### Ability bubbles (2026-10-02, owner's request, v1.5.0)
Rules: nothing new during the game (no capture work, hooks, memory reading or injection: only the
key and mouse records already in `<game>.input`), nothing shown in game, work only when the
overlay is switched on for a replay.
- **Action keys, game-agnostic (`cv-core/src/input/actions.rs`):** a game declares
  `ActionKey`s (id, label shown in the bubble, optional icon, category, size, colour, the game's
  default key) with their `ActionBind`s (virtual-key code or mouse button + Ctrl/Shift/Alt) via
  `GameIntegration::action_keys()` / `default_action_keys()` / `action_categories()`, and can
  refine how presses are drawn with `action_press_states()`. CS2 declares none.
- **League (`games/league/src/actions.rs`):** Q W E R (`Spell1-4`), D F (`AvatarSpell1-2`),
  item slots 1-6 (`evtUseItemN` + cast variants) and the trinket (`evtUseVisionItem` + variants;
  not `Item7`, which League 16.x uses for another slot: the owner's file has it on Right Arrow and
  "="). Every variant counts: cast, quick cast, quick cast with indicator, self cast, quick +
  self cast (with indicator), normal cast; several binds per action, modifiers, mouse buttons,
  arrow keys. Read with the ult tracking's reader (`ult::read_input_settings`: the
  PersistedSettings.json Input.ini part when it has binds, else `Config\input.ini`), once at the
  loading screen for both. League's defaults when a setting is missing: plain key = cast, Shift =
  quick cast, Alt = self cast; items on 1 2 3 5 6 7, trinket 4. The "Ult key" setting's manual
  key is added to R. Owner's real binds (16.19): plain keys = quick cast, Alt = with indicator,
  Shift = self cast, items on 1-6, trinket on C.
- **Saved per game:** `session.json` `action_keys`, written when the input recording starts
  (binds can change between games). Older recordings use League's defaults.
- **Matching (`actions::presses`):** a key-down (or mouse-button press) whose key **and**
  modifiers exactly match a bind; anything else (Ctrl+Q level-up, Ctrl+1, recall) gives nothing.
  Modifier state comes from the recorded Ctrl/Shift/Alt records and is forgotten when focus
  changes or the chat closes.
- **Exact moment:** the bubble appears on the video frame containing the key-down time (same
  QPC → video clock as the trail): the last frame starting at or before it, from the video's own
  frame times (`remux::frame_times`, read from the MP4 index, cached per file; 7.5 ms for a
  33 min game). Visible from that frame's start until start + fade time.
- **Exact position:** the cursor at the key-down time, interpolated between the two surrounding
  cursor samples (the capture stores a sample only when the cursor moved, so after a longer gap
  the movement is placed in the last sample period; never interpolated across a stroke break),
  mapped with the trail's window/letterbox mapping of that moment. The anchor (a small dot with a
  tail) sits exactly there and never follows the cursor.
- **Label:** the action, not the key: Q/W/E/R, D/F, "1"-"6" for item slots, a ward icon. A small
  hint shows the physical key when it is neither League's default key for that action nor the
  label itself (Q rebound to A → "Q" + "A"; item slot 4 on 5 or on 4 → no hint; the owner's
  trinket on C → hint "C"). Mouse binds show "M4"/"M5".
- **Overlap:** same action (Q Q Q Q) → overlapping at their own positions, no offsets. A bubble of
  another action that would cover the letter of a bubble still on screen when it appears moves
  its body sideways just enough (≤ 2.6 radii; the anchor and tail stay); decided at its birth so
  nothing jumps. Tails and anchor dots are drawn in a layer under all bodies (a slanted tail never
  crosses a letter), bodies oldest first (newest on top). Layout computed once per player size /
  fade / filters (3,194 presses: 1.3 ms).
- **Ult tie-in (`actions::press_states`):** R presses are paired 1:1 with the logged ult presses
  (`key_presses`; the constant offset between the game-clock estimate and the recording clock is
  removed first, then nearest within 120 ms). Checked game: "Ult pressed, no cast" → outlined,
  faded R (hidden while the timeline's "Unconfirmed presses" filter is off), otherwise solid
  ("Ult used"). Unchecked: live-accepted presses solid, filtered ones faded. R presses never
  logged as ult presses (loading screen, key bounce) faded. An R typed in chat has no bubble
  (keys aren't recorded while the chat is open).
- **Drawing (`ui/src/lib/bubbles.ts`):** pop-in (~100 ms scale with a slight overshoot), hold,
  fade-out (the last ~45 %); colour per slot (Q blue, W green, E amber, R purple, D rose, F teal,
  items slate, ward yellow), R and summoners 1.18x, items/ward 0.84x; white bold label with a dark
  outline, dark outline + light inner ring on the body (readable on bright and dark frames, no
  canvas shadows: too slow). Size scales with the video height (8-17 px radius).
- **Loading:** `input_actions` (Rust) is requested together with `input_load` on the first
  switch-on and doesn't hold up the trail: bubbles appear when they arrive (93 ms for the 33 min
  benchmark game, 13 ms for a 2 min one; both share one decode of the input file).
- **Options:** "Ability bubbles" group (on by default) with the game's categories (Abilities,
  Summoners, Items, Ward) and "Fade time" 0.1-3 s (step 0.1, default 1.0), in the overlay
  popover, saved with the other overlay options; changes apply on the next frame.
- **Tests:** Rust (`cv-core` actions: interpolation incl. rest-then-move and stroke breaks,
  key-down → frame incl. variable frame rate, modifiers / level-up combos, focus resets,
  rebinds, mouse binds, hints, saved vs default binds; League: defaults, the owner's ini and his
  PersistedSettings.json binds, every cast variant, rebinds, several binds, mouse and arrow keys,
  missing/unreadable settings files, PersistedSettings first, manual ult key, session.json round
  trip, ult tie-in incl. clock offset and 100 ms R spam; engine: binds saved with the session);
  `ui/tests/bubbles.unit.test.ts` (Node: animation/fade, alive range, same-action overlap,
  nudge with the anchor unchanged, Q spam + W, filters, a 40 min game's layout);
  `ui/tests/bubbles.test.mjs` (Chromium pixel test, 18 checks); `ui/tests/overlay-regression.mjs`
  (bubbles off = the v1.4.0 overlay, pixel for pixel); `ui/tests/make-sample.py` makes the test
  video (4:3, letterboxed game, 30 fps, WebM ms timestamps); `mp4tool binds` / `mp4tool bubbles
  <session> [--detail]` on the owner's PC; `--bench-replays` measures `bubblesMs` and a
  `draw_spam` case (densest 3 s of presses, fade 3 s, playing); `mp4tool fakeinput` adds 3 s of
  Q spam (+ a W) every 120 s and some Ctrl level-ups.
- **Measured on the owner's PC (2026-10-02, job 53):**
  | | before (v1.4.0) | after |
  |---|---|---|
  | replay open, page / first frame (median, 33 min / 2 min game) | 36 / 36 ms, 36 / 36 ms | 36 / 36 ms, 36 / 36 ms |
  | overlay first switch-on (33 min worst case / 2 min) | 120 / 68 ms | 127 / 71 ms |
  | overlay on again | 31-36 / 26-53 ms | 28-36 / 29-37 ms |
  | bubbles ready after switch-on | - | 93 ms (3,194 presses) / 13 ms (204) |
  | draw p95, default options (33 min / 2 min) | 0.2 / 0.3 ms | 0.3 / 0.2 ms |
  | draw p95 / max, everything on | 0.4 / 0.4 ms | 0.4 / 0.7 ms |
  | draw in Q spam: bubbles on screen, p95 / max | - | 44, 0.4 / 0.6 ms |
  | toggling while playing at 1x / 2x / 0.25x | 0 stalls | 0 stalls |
  Final build (job 55, alone): first switch-on 109 / 48 ms, on again 11-19 ms, bubbles ready
  86 / 12 ms, spam p95 0.4 ms (max 1.3 ms), first frame 20 ms median.
  The owner's 5 real games with input: 0-717 presses each, bubble list 0.2-0.8 ms + frame times
  0.1-6.5 ms; all 91 R bubbles match the ult check (28 "Ult used" with a press, 63 "no cast");
  the only logged ult press without a bubble was typed in chat. Key-downs land at most 26 ms
  after their frame's start (frames at 56-59 fps with some longer ones). Ult check on the 5
  games: identical (9/9, 5/5, 10/10 + 1, 34/34 + 107, 2/2 + 1).

### Fullscreen player, timeline zoom, frame stepping (2026-10-02, owner's request, v1.6 part A)
Rules: player-only (nothing during the game); v1.3-v1.5 behaviour unchanged.
- **Problem:** in fullscreen the controls, timeline and chips were *below* the video (flex
  column), so a 16:9 video was scaled into a shorter box: black bars left/right (reproduced at
  4K/150 %: 279 device px per side; owner saw ~110 px in a 2000 px wide screenshot).
- **Layout (`Player.svelte`):** one DOM for windowed and fullscreen (no re-mount, no re-buffer):
  `.chrome` is a CSS grid holding the left controls, right controls, `Timeline` and chips. In the
  window: controls / timeline / chips under the 16:9 video, as before. In fullscreen
  (`.player:fullscreen`, pure CSS so the first fullscreen frame is already right) `.screen` is
  `inset: 0` (the video always fills the screen and never moves) and `.chrome` is absolute at the
  bottom over the video: `color-scheme: dark` (every `light-dark()` token resolves dark inside it,
  readable on any frame), background rgba(6,8,12, opacity) with a soft 12 px gradient at the top,
  opacity 75 % by default (40-100 % in the player settings). Compact: timeline on top (markers
  15 px, ≤ 2 lanes), one row below with controls + chips (scrolling) + right controls: 88 px at
  1080 lines (8.1 %), 6 % at 1440.
- **Drop-down (YouTube-like):** a round down-arrow bubble at the panel's top centre slides it
  down (`transform` + delayed `visibility`, 150 ms); fully down, the bubble is gone too and
  nothing is drawn over the video (Playwright: pixel-identical to the bare video). A 300 × 100
  CSS px zone at the bottom centre fades in an up-arrow circle (100 ms) on hover, hides it 1 s
  after leaving; clicking it slides the panel back. Cursor hidden after 2 s without movement while
  the panel is down. **H** toggles. Entering fullscreen with the panel remembered down: no slide
  (`fsjust` class for two frames). Big play button hidden while the panel is down.
- **Prefs (`lib/playerprefs.ts`, localStorage `cv.player`):** panelDown, fit ("fit" | "fill"),
  panelOpacity. Fit = `object-fit: contain`, Fill = `cover`, fullscreen only (the window keeps
  its 16:9 box).
- **Overlay alignment:** `videoRect(…, fit)` handles contain/cover; drawing is clipped to the
  visible part; the keys strip stays above the panel (`insetBottom`). Bubbles/trail/dot use the
  same mapping, so they stay pixel-aligned in Fit, Fill, panel up or down.
- **Timeline (`Timeline.svelte`, `lib/timelineview.ts`):** a view (start, span) over the video;
  Ctrl+wheel zooms around the cursor (the page's own zoom is prevented), slider (logarithmic) /
  − / + / "whole game" in the controls, shortest view = 20 frames; Shift+wheel or a horizontal
  wheel pans, dragging the empty marker row pans, and an overview bar under a zoomed timeline
  (drag the window / click to jump). Zoomed: a ruler with labels on round game-clock times (min
  64 px apart), minor ticks, one tick per frame once frames are ≥ 3 px apart and frame numbers
  once ≥ 9 px. While playing or stepping the view pages so the playhead stays in view (a pan never
  fights it). Marker lanes are recomputed per view, so markers that overlap at the whole game
  spread out when zoomed; the height is fixed by the whole-game lanes (no jumping).
  **Speed:** markers are drawn on a canvas from pre-drawn sprites (one per kind / steal / size /
  theme); the buttons on top are invisible hit targets (click, hover, focus, screen readers, the
  benchmark) moved only once the view settles (120 ms), so zooming never restyles hundreds of
  elements per frame. The first version moved 600 DOM buttons per frame: 15-20 ms style recalc
  per frame in Chromium (p95), now median 1 ms, p95 4-7 ms in headless software rendering.
- **Frame-exact stepping:** `video_frame_times` (new command; the MP4 index's frame times as f64
  bytes, cached per file) → `frameIndexAt` / `frameSeekTime` (the middle of the frame's interval,
  so rounding never lands on a neighbour) and requestVideoFrameCallback confirms the frame shown
  (`frameNearest(mediaTime)`, frames still from before the seek ignored). Steps queue to an
  absolute target (holding a key or the button never loses or doubles a step). Forward by one:
  first tries playing (muted, 1x) to the next frame and pausing on its rVFC (no keyframe decode);
  if that ever overshoots (no vsync-locked frames, e.g. headless Chromium) it switches to seeks
  for the session. Time shows m:ss.mmm + frame number while paused or zoomed.
- **Tests (Chromium, mock backend, `ui/tests/`):** `make-sample.py` now also makes
  `sample169.webm` / `sample169old.webm` (960×540, 60 fps, frame index written as 14 squares,
  keyframes 1 s / 5.5 s, + ffprobe frame/keyframe lists). `fullscreen.test.mjs` 94/94: 5 screens
  (3840×2160, 4K@150 %, 2560×1440, 2560×1080, 1920×1200) × Fit/Fill × panel up/down × 16:9 and
  4:3 videos: bars exactly as expected (0 for 16:9 on 16:9), video placement (≤ 0.34 source px),
  overlay dot vs the exact cursor ≤ 0.11-0.3 px; panel bubble / hover arrow / 1 s hide / cursor
  hide / H / remembered / double-click / no re-buffering / same `<video>`; enter fullscreen
  median 84 ms (headless software 4K, max 287 ms), leave 45 ms, panel slide 143-175 ms
  (transition 150). `framestep.test.mjs`: 60 steps forward + 60 back across a keyframe all
  consecutive (read from the picture), rVFC and the label agree, Shift = 10, held key = exact
  count, buttons, pause on step, overlay dot ≤ 0.31 px on every stepped frame; zoom anchor 0 px,
  click/playhead mapping exact, 20-frame view, pans, follow while playing (0 of 39 samples out of
  view), 620 markers spread (617 → 0 overlapping), marker click and chips while zoomed.
  `timeline.unit.test.ts` (Node, in CI). v1.5 overlay: pixel-identical to v1.5.0 (16/16),
  overlay 20/20, bubbles 18/18.
- **Measured in headless Chromium (software decode/raster, no GPU):** step forward median
  53-60 ms, back 51-55 ms (1 s keyframes); old 5.5 s keyframes ~290 ms both ways (worst case
  5.4 s after a keyframe). Real numbers on the owner's PC: `--bench-replays` with `"player": true`
  (steps, zoom draw + frame intervals, fullscreen switch, panel), see "Current status".

### Replay benchmark and end-to-end tests (developer tools)
- `Clairvoyance.exe --bench-replays=<config.json>`: opens the given games N times in the real
  window and measures page / first frame / playable / marker jumps / "marker clicked right away"
  (`ui/src/lib/replaybench.ts`, `app/src/bench.rs`); writes JSON and quits.
- `Clairvoyance.exe --ui-test=<config.json>`: end-to-end checks in the app (`ui/src/lib/uitests.ts`):
  game-mode rules with simulated games, storage limit during a game, finalizing, crash recovery.
  Run in a sandbox profile (APPDATA / LOCALAPPDATA / USERPROFILE pointed at a test folder).
- The simulator (`--simulate`, Settings > Advanced) also fakes the **League client**: a lockfile
  (`%LOCALAPPDATA%\Clairvoyance\simulator\lockfile`, protocol `http`) and the LCU endpoints
  `/lol-gameflow/v1/session` and `/lol-game-queues/v1/queues`, with a chosen queue
  (`--simulate-queue=450`, or the Mode picker), so game-mode rules can be tried without playing.
- The simulator refuses to start while the previous simulated game is still closing (its fake
  game process must be gone, or the engine treats the next one as the same process still open
  after its match). Found by the end-to-end tests.
- `mp4tool` (cv-capture example): `info`, `finalize`, `loop` (make a recording of any length from
  a real one), `benchsession`.
- Settings > Advanced > **Save test report**: a zip (log, latest game's session.json, settings
  without Riot ID, report.txt with file layout/keyframes of the latest recordings and the
  responsiveness numbers) in `<recordings>\test-reports\`. No videos.

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
- [ ] Owner's Practice Tool test of the ability bubbles (see "Next steps")

## Known issues
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
- A recording is finalized shortly after its game (seconds); if you open it in those seconds, it
  plays from the fragmented file (slow first frame) and is finalized the next time.
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

## Next steps
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
