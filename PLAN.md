# GameRecorder: Project Plan

> **For Claude:** read this whole file at the start of every session. After finishing a
> milestone, tick it off below, add any decisions to "Decisions made", and update "Current status".

## Current status
- **Stack chosen: Tauri 2 (Rust + Svelte web UI).** The C#/WPF skeleton is replaced (see Decisions).
- **The built-in recorder is the only recorder** (OBS support was removed on 2026-09-30, see Decisions):
  Windows Graphics Capture + the GPU's hardware H.264 encoder (NVENC / AMF / Quick Sync) +
  per-process game audio + an optional mic track, written as crash-safe fragmented MP4, with an
  in-memory replay buffer for clips.
- Milestones 1–13 are code-complete. Tested on Claude's build machine: unit tests, the engine
  end-to-end against a fake League API, the MP4 muxer
  (decoded with ffmpeg, including a file cut off mid-write), and the UI in a browser.
  The built-in recorder's Windows code compiles but **has not run on real Windows hardware yet**.
- The Windows installer, the portable zip and `dist\tools\recorder-selftest.exe` are in `dist\`.
- **First owner test (2026-09-30):** the built-in recorder crashed the app as soon as it started
  (self-test, in-app test and a Practice Tool game). Cause: game-audio setup freed memory it
  didn't own (a PROPVARIANT pointing at stack data was dropped → heap corruption). Fixed; crashes
  and panics are now written to the log, and the self-test writes a step-by-step log.
- **Second owner test (2026-09-30):** the self-test works: 1080p60 from a 4K screen, 600/600 frames,
  0 dropped, NVENC, game-only audio, 0.4% CPU, the file and the replay clip decode cleanly. The
  performance test got a baseline only (League ~141 FPS, capped) because the game was left after
  the first phase. Fixed from it: FPS windows were misaligned with PresentMon's clock; the test
  now stops with a clear message if you leave the game; Riot's `playerlist` `items` can be an
  object (it broke KDA/CS/gold parsing), now accepted.
- **Third owner test (2026-09-30), Practice Tool, 2 × 2 min, RTX 5080 / i9-9900K, 4K → 1080p60:**
  recording worked (game window, game-only audio, 0 dropped frames, NVENC). GameRecorder: 0.48%
  CPU, 182 MB RAM while recording (76 MB idle; the 30 s replay buffer is most of the difference),
  GPU encode engine 7%. FPS (analysed by hand from PresentMon; the app missed it, see below):
  steady play 140.7 → 140.3 avg (−0.3%), 1% low 102.5 → 100.2, p99 frame time 8.8 → 9.0 ms.
  League is CPU-bound (GPU busy < 1 ms/frame); its FPS dips to ~70 during heavy moments happened
  in both phases, i.e. gameplay, not recording.
  Fixed from it: PresentMon only writes its file when it exits, so the app now waits for you to
  leave the game before analysing (the report had "no FPS"); CSV BOM handled; the recorder
  captured ~51 fps instead of 60 (frame pacing aliasing 141 fps down) — now a fixed 60 fps grid.
- **Waiting for the owner**: (1) run `dist\tools\recorder-selftest.exe`; (2) run
  Settings > Performance test in a real League game (Practice Tool is fine) and share the report.
- Old C# files (`src\`, `GameRecorder.sln`) are no longer used and can be deleted.
- Next: fix whatever the owner's tests show, then nice-to-haves (auto-update, Dota 2 module).

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
- Each game is its own crate under `games/` implementing `GameIntegration` (in `crates/gr-core/src/game.rs`).
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
- `crates/gr-core`: game-agnostic core. `GameIntegration` trait (one module per game),
  `Recorder` trait, `GameEvent`/`EventKind`, `GameSession` (with `video_offset`), settings,
  library/retention, and the **engine** (state machine: idle → game detected → recording → saved).
  No game-specific code.
- `games/league`, `games/cs2`: one crate per game. `app/src/games.rs` registers them.
- `app`: Tauri shell (tray, windows, commands for the UI, Win32 bits). `ui`: Svelte front end.

### Built-in recorder (crate `gr-capture`, default)
- Implements the `Recorder` trait from `gr-core`, so the engine can be tested with a fake.
- **Capture: Windows Graphics Capture** of the game window (free-threaded frame pool, no yellow
  border, cursor on), with monitor capture as fallback (setting, window not found in 8 s, or no
  frames for 6 s). No hooks, no DLLs, nothing in the game process. Frames are capped at the target
  fps in the capture callback.
- **Colour conversion + scaling on the GPU:** D3D11 video processor BGRA → NV12 (BT.709 limited),
  letterboxed to the output size, into a ring of 8 textures. Frames are dropped (never queued up)
  if the encoder falls 5 behind, so the game can't be slowed down.
- **Encode: Media Foundation hardware H.264 MFT** (async, D3D11-aware) on the same GPU as the
  capture, preferring the vendor's own (NVENC → AMF → Quick Sync by adapter). VBR, 2 s GOP, no
  B-frames, low-latency mode. Bitrate: 12 Mbps (Standard) / 20 Mbps (High) at 1080p60, scaled by
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
  a clean close.
- **Replay buffer in memory:** the last N seconds of encoded packets (capped in bytes), trimmed at
  keyframes. Saving a clip writes a normal MP4 (moov first) from it, with no re-encode.
- Encoders that only publish SPS/PPS in the output format (not in-stream) get them prepended to
  keyframes from `MF_MT_MPEG_SEQUENCE_HEADER`.
- Crash logging: a panic hook and an unhandled-exception filter write the thread and exception
  code to the log before the process dies (release builds use panic=abort).
- Screenshots (thumbnail at 1:30) are the next captured frame saved as JPEG through WIC.
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
- `Videos\GameRecorder\<date>_<time>_<Game>\` per game: video
  (`2026-09-30_League_Ahri_Win.mp4`, renamed after the match), `session.json`, `thumb.jpg`
  (screenshot at 1:30 game time), `clips\`.
- Settings: `%APPDATA%\GameRecorder\settings.json`. Logs: `%LOCALAPPDATA%\GameRecorder\logs\`.
  ffmpeg (downloaded on demand): `%LOCALAPPDATA%\GameRecorder\ffmpeg\`.
- Retention: delete games older than X days and/or keep under X GB, oldest first; favorites
  and the game being recorded are never deleted.

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
- Built by cross-compiling from Linux (`scripts/build-windows.sh`): distro rustc 1.91 with
  `-Zbuild-std` for `x86_64-pc-windows-gnu`. The exe needs `WebView2Loader.dll` next to it,
  so the "portable" build is a zip of the exe + dll. Main download: the NSIS installer
  (per-user, no admin, Start menu + desktop shortcut, uninstaller that keeps recordings).
- Dev tools: `GameRecorder.exe --simulate` / Settings > Advanced plays a fake League match.

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
- [ ] Owner's test on Windows: self-test + performance test in a real League game (see README)
- [ ] Nice-to-have: auto-update via GitHub Releases

After each milestone: explain how to test it and how to measure its performance impact.

## How to work with the owner
- **Build everything yourself.** This is not a learning project: write all the code and make the
  technical decisions without stopping to teach or asking the owner to write code.
- Only ask the owner when you need him (testing in a real League game, approving installs,
  or a big change to the spec).
- After each milestone, give brief test steps plus a performance check, then continue.
