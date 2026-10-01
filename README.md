# Clairvoyance

A lightweight game recorder for Windows in the style of Ascent/Outplayed. It records every
League of Legends (and Counter-Strike 2) match automatically and puts your kills, deaths,
assists, objectives, steals and ult presses on a clickable timeline.

- Has its **own built-in recorder**: nothing else to install. It captures the game with Windows
  Graphics Capture (nothing injected into the game) and encodes on your graphics card
  (NVENC / AMD AMF / Intel Quick Sync), never the CPU. Records only the game's sound (not
  Discord or Spotify), plus your mic on a separate track if you turn it on.
- Recordings survive a crash or power cut (fragmented MP4: you only lose the last seconds).
- While you play, Clairvoyance itself is a small background process in the tray: below-normal
  priority, no overlay, nothing injected into the game; its window is paused while hidden.
- Uses only official game APIs (Riot's Live Client Data API, CS2 Game State Integration),
  so it's safe with Vanguard and VAC.
- No accounts, no cloud, no telemetry.

See **PLAN.md** for the spec, decisions and milestone status.

## Install (you or a friend)

1. Download `Clairvoyance_<version>_x64-setup.exe` from the
   [latest release](https://github.com/Emre-98/Clairvoyance/releases/latest) and run it
   (no admin rights needed). It installs for your Windows user and adds a Start menu entry.
2. Clairvoyance keeps itself up to date: it checks GitHub for a new version when it starts and
   every few hours (never while you're in a game) and asks before installing.
3. The first-run setup shows your graphics card and its encoder and can record a 5-second
   test. Then it asks for your Riot ID (optional) and save folder.

Windows 11 has everything else built in. On Windows 10 (version 2004 or newer for game-only
audio) the installer downloads the Microsoft Edge WebView2 Runtime if it's missing.

**Coming from GameRecorder?** That was this app's old name. The first start moves your settings,
games, clips and downloaded tools over; the Home page then offers to uninstall the old app.

## Using it

- Start a League game. Recording starts at the loading screen and stops after the match.
  The window stays available, so you can alt-tab to it in the loading screen; the tray icon
  turns red. Closing the window hides it to the tray (paused, reopens instantly).
- In game: **F8** saves the last 30 seconds as a clip, **F9** adds a marker. Change them in
  Settings > Hotkeys. Your ult key (default R) is logged as "Ult pressed".
- Afterwards open the game from **Games**: click any marker (or event in the list) to jump
  to 5 seconds before it; **N / P** = next / previous event, **Space** play/pause,
  **← / →** ±5 s, **F** fullscreen. Filter chips hide or show event types.
- **Input overlay** (button under the video, or **I**): your cursor trail, clicks (left blue,
  right red), cursor dot, the keys you pressed and a cursor heatmap, drawn over the replay. Off
  every time a replay opens; options next to the button. The game page also shows
  **Mechanics**: APM (and per minute), right-clicks per second, cursor distance, path
  efficiency and idle time; drag across the APM chart for a part of the game. Mouse and
  keyboard are recorded only while the game window is focused, never while the chat is open,
  as key codes (never text), and stay on your PC. Nothing is shown during the game. Turn it
  off in Settings > Games > League.
- **Create clip** opens the clip editor: drag the handles on the timeline and save. The first
  export downloads ffmpeg (about 100 MB, once).
- Files: `Videos\Clairvoyance\<date>_<time>_League\` holds the video
  (`2026-09-30_League_Ahri_Win.mp4`), `session.json` (the timeline) and `clips\`.
  Thumbnails are kept separately in `%LOCALAPPDATA%\Clairvoyance\Thumbnails\` (made after
  each game, never while you play).
- **Storage** (Settings > Storage): recordings are kept under a size limit (100 GB by default).
  When they grow past it, the oldest are deleted first, with their clips, timeline and
  thumbnail. Star a game (favorite) or pin a clip ("keep") and it's never deleted
  automatically. The page shows what was removed; automatic clean-up can be turned off.
- **Game modes** (Settings > Game modes): choose per League mode whether it's recorded:
  Record, Clips only (timeline + clips, no full video) or Off. E.g. Ranked and Normal on, ARAM
  and Arena off. The list comes from the League client and Riot, so new and rotating modes
  appear by themselves ("New mode detected"); quick presets: Everything, Ranked only,
  Ranked + Normal. The tray says when a game isn't recorded and why.
- **Look**: Settings > Appearance: Dark, Light or Match Windows (follows your Windows setting).
- **Updates**: when a new version is out, a card "Update available" appears in the sidebar with
  what's new; click **Update now** and Clairvoyance updates and restarts. It never checks or
  installs during a game. Settings > General & updates has "Check now" and an off switch.

### Try it without playing

Settings > Advanced > **Simulate a League game** plays a scripted 3-minute match (kills, a
triple kill, a stolen Herald, towers, Baron, a win) against a fake game API and records your
desktop. You can also start `Clairvoyance.exe --simulate` (or `--simulate=90` for a
90-second match).

## Checking the performance impact

- **Recorder self-test** (no game needed): double-click `dist\tools\recorder-selftest.exe`.
  It records 10 seconds of your screen and writes `selftest-output\selftest-report.txt`
  (encoder used, frames, dropped frames, bitrate, CPU/GPU use) plus the test video next to it.
  Same test in the app: Settings > Recorder > "Test the recorder" (5 s).
- **Performance test in a real game**: Settings > Performance test. Start it, then start a
  League game (Practice Tool is fine) and just play or stand still. It measures FPS (average,
  1% low, frame times, with Intel's PresentMon, which asks for admin once), CPU and GPU use,
  first without recording and then with the built-in recorder, and shows the difference. The report is saved in `<save folder>\perf-tests\`.
- The sidebar shows Clairvoyance's own CPU and RAM. Every game's page shows the average and
  peak CPU/RAM Clairvoyance used during that game. Target: under 1% CPU and ~150 MB RAM
  while recording (without the window open); idle near 0% CPU.
- Task Manager > Performance > GPU: while recording, the "Video Encode" graph shows the
  encoder working; the "3D" graph should barely change.

## Troubleshooting

- **Recording is black:** Settings > Recording > "Capture the whole screen instead of the game window".
- **"No hardware H.264 encoder found":** update your graphics driver. (Very old GPUs without a
  video encoder can't record.)
- **Discord/music is in the recording:** your Windows is older than Windows 10 2004, so game-only
  audio isn't available and the whole desktop sound is recorded.
- **Nothing happens when a game starts:** check the tray tooltip, then the log file
  (Settings > Advanced > Log file).
- **CS2 isn't detected as in a match:** restart CS2 once after installing Clairvoyance
  (it reads the Game State Integration config at launch).

## Building from source

Layout:

```
crates/cv-core        game-agnostic core: events, sessions, engine (state machine), settings, library
crates/cv-capture     built-in recorder: Windows Graphics Capture, hardware H.264, WASAPI, fMP4 muxer
crates/cv-mock-league fake League API used by the simulator and tests
games/league          League of Legends module (Live Client Data API)
games/cs2             Counter-Strike 2 module (Game State Integration)
app                   Tauri 2 desktop app (Rust): tray, raw-input hotkeys, commands, windows APIs
ui                    Svelte 5 + TypeScript front end
docs/ADDING_A_GAME.md how to add another game
```

On Windows: install Rust (https://rustup.rs) and Node.js 20+, then

```
cd ui && npm ci && npm run build && cd ..
cargo build --release -p clairvoyance
```

The exe is `target\release\clairvoyance.exe`. `cargo test` runs the tests.
`scripts/build-windows.sh` is the cross-build (Linux → Windows) used for test builds (a portable
zip in `dist\`). Releases (installer + auto-update files) are built by GitHub Actions: see
**RELEASING.md**.
