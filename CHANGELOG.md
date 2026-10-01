# Changelog

Newest first. `scripts/release.ps1` adds a section for each release; the section becomes the
release notes on GitHub and in the in-app "Update available" card.

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
