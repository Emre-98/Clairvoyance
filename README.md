# GameRecorder

A lightweight game recorder for Windows in the style of Ascent/Outplayed. It records every
League of Legends (and Counter-Strike 2) match automatically and puts your kills, deaths,
assists, objectives, steals and ult presses on a clickable timeline.

- Has its **own built-in recorder**: nothing else to install. It captures the game with Windows
  Graphics Capture (nothing injected into the game) and encodes on your graphics card
  (NVENC / AMD AMF / Intel Quick Sync), never the CPU. Records only the game's sound (not
  Discord or Spotify), plus your mic on a separate track if you turn it on.
- Recordings survive a crash or power cut (fragmented MP4: you only lose the last seconds).
- OBS Studio can be used instead, as a backup (Settings > Recorder).
- While you play, GameRecorder itself is a small background process in the tray: below-normal
  priority, no overlay, nothing injected into the game; its window is paused while hidden.
- Uses only official game APIs (Riot's Live Client Data API, CS2 Game State Integration),
  so it's safe with Vanguard and VAC.
- No accounts, no cloud, no telemetry.

See **PLAN.md** for the spec, decisions and milestone status.

## Install (you or a friend)

1. Run `dist\GameRecorder-Setup-<version>.exe` (no admin rights needed), or unzip
   `dist\GameRecorder-<version>-portable.zip` anywhere and run `GameRecorder.exe`.
2. The first-run setup shows your graphics card and its encoder and can record a 5-second
   test. Then it asks for your Riot ID (optional) and save folder.

Windows 11 has everything else built in. On Windows 10 (version 2004 or newer for game-only
audio) the installer tells you if the Microsoft Edge WebView2 Runtime is missing.

### Using OBS instead (optional)

Settings > Recorder > **OBS Studio (backup)**. Install OBS (https://obsproject.com/download);
the setup there turns on OBS's WebSocket server (close OBS first when it asks) and tests the
connection. If OBS opens its "Auto-Configuration Wizard" the very first time, click Cancel:
GameRecorder configures OBS itself, in its own OBS profile and scene collection called
"GameRecorder" (your own scenes aren't touched, and your profile is restored after each game).

## Using it

- Start a League game. Recording starts at the loading screen and stops after the match.
  The window stays available, so you can alt-tab to it in the loading screen; the tray icon
  turns red. Closing the window hides it to the tray (paused, reopens instantly).
- In game: **F8** saves the last 30 seconds as a clip, **F9** adds a marker. Change them in
  Settings > Hotkeys. Your ult key (default R) is logged as "Ult pressed".
- Afterwards open the game from **Games**: click any marker (or event in the list) to jump
  to 5 seconds before it; **N / P** = next / previous event, **Space** play/pause,
  **← / →** ±5 s, **F** fullscreen. Filter chips hide or show event types.
- **Create clip** opens the clip editor: drag the handles on the timeline and save. The first
  export downloads ffmpeg (about 100 MB, once).
- Files: `Videos\GameRecorder\<date>_<time>_League\` holds the video
  (`2026-09-30_League_Ahri_Win.mp4`), `session.json` (the timeline) and `clips\`.

### Try it without playing

Settings > Advanced > **Simulate a League game** plays a scripted 3-minute match (kills, a
triple kill, a stolen Herald, towers, Baron, a win) against a fake game API and records your
desktop. You can also start `GameRecorder.exe --simulate` (or `--simulate=90` for a
90-second match).

## Checking the performance impact

- **Recorder self-test** (no game needed): double-click `dist\tools\recorder-selftest.exe`.
  It records 10 seconds of your screen and writes `selftest-output\selftest-report.txt`
  (encoder used, frames, dropped frames, bitrate, CPU/GPU use) plus the test video next to it.
  Same test in the app: Settings > Recorder > "Test the recorder" (5 s).
- **Performance test in a real game**: Settings > Performance test. Start it, then start a
  League game (Practice Tool is fine) and just play or stand still. It measures FPS (average,
  1% low, frame times, with Intel's PresentMon, which asks for admin once), CPU and GPU use,
  first without recording and then with the built-in recorder (and OBS, if you tick it),
  and shows the difference. The report is saved in `<save folder>\perf-tests\`.
- The sidebar shows GameRecorder's own CPU and RAM. Every game's page shows the average and
  peak CPU/RAM GameRecorder used during that game. Target: under 1% CPU and ~150 MB RAM
  while recording (without the window open); idle near 0% CPU.
- Task Manager > Performance > GPU: while recording, the "Video Encode" graph shows the
  encoder working; the "3D" graph should barely change.

## Troubleshooting

- **Recording is black:** Settings > Recording > "Capture the whole screen instead of the game window".
- **"No hardware H.264 encoder found":** update your graphics driver. (Very old GPUs without a
  video encoder can use OBS instead.)
- **Discord/music is in the recording:** your Windows is older than Windows 10 2004, so game-only
  audio isn't available and the whole desktop sound is recorded.
- **OBS (if you use it) isn't reachable:** Settings > Recorder > Test connection.
- **Nothing happens when a game starts:** check the tray tooltip, then the log file
  (Settings > Advanced > Log file).
- **CS2 isn't detected as in a match:** restart CS2 once after installing GameRecorder
  (it reads the Game State Integration config at launch).

## Building from source

Layout:

```
crates/gr-core        game-agnostic core: events, sessions, engine (state machine), settings, library
crates/gr-capture     built-in recorder: Windows Graphics Capture, hardware H.264, WASAPI, fMP4 muxer
crates/gr-obs         OBS recorder over obs-websocket v5 (backup)
crates/gr-mock-league fake League API used by the simulator and tests
games/league          League of Legends module (Live Client Data API)
games/cs2             Counter-Strike 2 module (Game State Integration)
app                   Tauri 2 desktop app (Rust): tray, raw-input hotkeys, commands, windows APIs
ui                    Svelte 5 + TypeScript front end
installer             NSIS installer script
docs/ADDING_A_GAME.md how to add another game
```

On Windows: install Rust (https://rustup.rs) and Node.js 20+, then

```
cd ui && npm ci && npm run build && cd ..
cargo build --release -p gamerecorder
```

The exe is `target\release\gamerecorder.exe`. `cargo test` runs the tests.
`scripts/build-windows.sh` is the cross-build (Linux → Windows) that produces the installer and
the portable zip in `dist\`.
