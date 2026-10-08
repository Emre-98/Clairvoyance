# Clairvoyance

A lightweight game recorder for Windows. It records every League of Legends (and Counter-Strike 2)
match automatically and puts your kills, deaths, assists, objectives, steals and ult presses on a
clickable timeline.

This repository holds only the official releases (installer, update files and checksums). The
source code is private.

## Download

1. Download `Clairvoyance_<version>_x64-setup.exe` from the
   [latest release](https://github.com/Emre-98/Clairvoyance-releases/releases/latest) and run it
   (no admin rights needed).
2. Optional: check the download. In PowerShell, `Get-FileHash .\Clairvoyance_<version>_x64-setup.exe`
   must show the SHA-256 listed in that release's notes.
3. Clairvoyance keeps itself up to date from this page and asks before installing an update.

Downloads from anywhere else aren't official.

## Why you can trust Clairvoyance

- **Your games stay on your PC.** No accounts, no cloud, no telemetry, no analytics, no ads.
  Recordings, timelines, clips and input data are never uploaded. The only internet requests are
  the update check, Riot's public game data and the free tools you ask for (ffmpeg for clips,
  PresentMon for the performance test). The full list is in [PRIVACY.md](PRIVACY.md).
- **Safe with anti-cheat.** Only the official game APIs (Riot's Live Client Data API, CS2 Game
  State Integration) on your own PC, recording with Windows Graphics Capture. Nothing is injected
  into the game, no overlay over it, no game memory read.
- **Your keyboard is not logged.** Mouse and key presses (for the input overlay) are recorded only
  while the game window is focused, never while the chat is open, as key codes (never text), and
  stay on your PC. You can turn it off.
- **Every update is signed** with the developer's key; the app refuses an update that isn't.
- **SHA-256 checksums** for every file in each release's notes.

## Questions, bugs, ideas

Open an [issue](https://github.com/Emre-98/Clairvoyance-releases/issues).

## License

Copyright (c) 2026 Emre-98. All rights reserved. Free for personal use; see [LICENSE](LICENSE).
