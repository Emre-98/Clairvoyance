# Changelog

Newest first. `scripts/release.ps1` adds a section for each release; the section becomes the
release notes on GitHub and in the in-app "Update available" card.

## [1.5.0] - 2026-10-02

- Ability bubbles on the input overlay: every ability, summoner spell, item or ward you use pops up as a small bubble on the replay exactly where your cursor was, on the exact frame, and fades out
- Your League keybinds are read from League's own settings and saved with each game: rebound keys still show the ability (with the key as a small hint), Ctrl+Q level-ups and typing in chat never show a bubble
- Your ult follows the ult check: casts are solid, presses without a cast are faded and only shown with 'Unconfirmed presses' on
- Overlay options: Abilities / Summoners / Items / Ward toggles and a fade time slider (0.1 to 3 s)

## [1.4.0] - 2026-10-02

- Input overlay: press I (or the button under the video) to see your cursor trail, clicks (left blue, right red), the keys you pressed and a cursor heatmap on top of the replay. Options next to the button
- Mechanics on the game page: APM (and per minute), right-clicks per second, cursor distance, path efficiency and idle time; drag across the APM chart for a part of the game
- Mouse and keyboard are recorded only while League is focused, never while the chat is open, as key codes (never text), and stay on your PC. Turn it off or change the sample rate in Settings > Games > League
- An ult bound to a mouse side button is now matched to its press
- Performance test: a third phase with input recording, and it no longer fails when a game is already being recorded

## [1.3.0] - 2026-10-01

- Accurate ult tracking: your ult keys are read from League's settings (quick cast, indicator, self-cast), and presses before level 6, while dead, in chat or on cooldown are ignored
- After each game the recording is checked: real ult casts are marked 'Ult used' at the exact moment, also casts the keyboard log missed
- Presses without a cast are kept as 'Unconfirmed presses' (hidden; click the chip under the video to show them)
- Older recordings are checked too, in the background, while no game is running
- Events stay in the right place when Clairvoyance is started in the middle of a game

## [1.2.0] - 2026-10-01

- Replays open instantly: the first frame shows in about 0.1 s instead of up to 30 s; recordings are finalized for instant playback after each game (older ones too)
- Marker jumps land in under 0.2 s; markers clicked while the video is still loading are remembered
- Thumbnail shown while a replay loads; games start loading when you hover them
- Keyframes every second in new recordings
- A game interrupted by a crash or power cut keeps its video
- Settings > Advanced: Save test report; the simulator can play any game mode
- Clash is grouped with events, not Ranked

## [1.1.0] - 2026-10-01

- Game modes: choose per League mode (Ranked, Normal, ARAM, Arena, rotating modes, TFT...) whether it's recorded: Record, Clips only or Off (Settings > Game modes).
- The exact queue comes from the League client when the game starts, so a mode that's off is never recorded; the tray says why.
- New and rotating modes appear by themselves with a New badge; modes that disappear keep your choice.
- Library cards show the game mode, and the library can filter by mode.

## [1.0.0] - 2026-10-01

- GameRecorder is now **Clairvoyance**. Your settings, games, clips and tools are moved over on
  the first start, and the Home page offers to uninstall the old app.
- **Updates itself** from GitHub Releases: "Update available" card with release notes and an
  "Update now" button (never during a game). Settings > General & updates.
- **Dark, Light and Match Windows** themes (Settings > Appearance), switched instantly.
- **Storage limit** (default 100 GB) with automatic clean-up of the oldest recordings; favorites
  and clips marked "keep" are never deleted. Settings > Storage shows what was removed.
- Thumbnails live in their own folder and are made after the game, never while you play.
- Faster everywhere: the library opens from a cache, long lists only draw what's on screen,
  thumbnails load as you scroll, keyframes every second for instant timeline jumps.
- OBS support removed: the built-in recorder is the only recorder.
