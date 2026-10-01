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
- **Waiting for the owner**: play a real League game with v1.1.x and check the timeline,
  thumbnail and Settings > Advanced > Responsiveness numbers (see "Known issues").

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
  registered only while a game runs, only counted while the game window is focused.
  Ctrl+R (level up) is ignored; Enter/Escape track the chat so typing "r" doesn't count;
  held-key auto-repeat ignored. Labelled "Ult pressed".
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

## Next steps
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
