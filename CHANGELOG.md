# Changelog

Newest first. `scripts/release.ps1` adds a section for each release; the section becomes the
release notes on GitHub and in the in-app "Update available" card.

## [Unreleased]

- Timeline: towers, inhibitors and objectives now say who got them, which lane and tier, and how much gold you got; kills and objectives show the champions involved
- New 'Completed <item>' markers with the item's icon, cost and components. Pressing UNDO in the shop doesn't leave a wrong marker, selling an item keeps it
- New summoner spell markers ('Flash', 'Ignite'...), checked against the recording after the game so presses on cooldown don't count
- A faint APM chart behind the timeline markers; hover it to see your actions per minute at that moment
- Items and Summoner spells have their own filter chips; icons show in the event list too
- Scoreboard of all 10 players (champion, level, KDA, CS, items, summoner spells) under the player, showing the moment you're watching and following along while you scrub. Press O to see it over the video, or hold Tab in fullscreen. Games recorded before this version show your own final numbers

## [1.7.1] - 2026-10-03

- Your game is ready a few seconds after it ends: the recording makes itself instantly playable the moment it stops (no more waiting for it to be prepared), so it opens and jumps to markers at once, also after a long game
- Recording now stops 2 seconds into the victory / defeat screen (was 6), and leaving a game is noticed faster; the thumbnail is made right away
- Replays (watching a .rofl or 'Watch' in match history) are no longer recorded; the tray says 'Replay: not recorded'. Spectating someone's game isn't recorded either, unless you turn on Settings > Games > League > 'Record games you spectate'
- Settings > Advanced > Simulate: try a replay or spectating too

## [1.7.0] - 2026-10-03

- Any window size works: nothing overlaps or gets cut off, from a small window up to 4K at 100/125/150 % scaling. Player controls that don't fit move into a new '...' More controls menu, and menus always stay on screen
- The window can now be made smaller (940 x 560)
- Input overlay: one 'Trail & bubbles' slider (0.25-3 s) instead of two; an ability bubble now disappears on the same frame as the piece of cursor trail under it. Your old setting is kept
- Game cards show the full champion name; long texts show in full when you hover them

## [1.6.1] - 2026-10-02

- Gaming laptops with two GPUs: recording now uses the NVIDIA/AMD encoder, and that GPU can go back to sleep after a game

## [1.6.0] - 2026-10-02

- Fullscreen without black bars: the video fills the screen and the controls, timeline and filters sit over its bottom as a see-through panel (opacity 40-100 % in the new player settings)
- True fullscreen: the small arrow (or H) slides the controls away; hover the bottom centre to bring them back. Fit / Fill for screens of another shape
- Zoom the timeline down to single frames: Ctrl + mouse wheel over it, the zoom slider or + / -; time ruler with frame ticks; Shift + wheel or drag to pan
- Frame-by-frame: , and . (Shift: 10 frames, hold to repeat) or the buttons next to play, on the recording's real frames; time with milliseconds and the frame number
- One ult = one 'Ult used' marker: commanding Tibbers / Daisy / Shaco's clone and recasts (Ahri, Zed, Jhin's shots...) are 'Ult recast'; Jayce, Nidalee, Elise and Udyr swaps are 'Form swap' (both under their own filter chips, hidden by default); recast bubbles on the input overlay are smaller and outlined
- Older games are re-checked in the background, after games, never while you play

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
