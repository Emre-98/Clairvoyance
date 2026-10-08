# Clairvoyance privacy policy

Last updated: 2026-10-08

Short version: **Clairvoyance has no accounts, no cloud, no telemetry, no analytics and no ads.
Your recordings, timelines, clips and input data stay on your PC.** Nothing about you or your
games is ever sent to the developer or anyone else.

## What Clairvoyance stores, and where

Everything below is stored only on your own computer:

| What | Where |
| --- | --- |
| Recordings, their timelines (`session.json`) and clips | your save folder (default `Videos\Clairvoyance\`) |
| Thumbnails | `%LOCALAPPDATA%\Clairvoyance\Thumbnails\` |
| Settings (including the Riot ID you entered, if any) | `%APPDATA%\Clairvoyance\` |
| Log file, downloaded tools (ffmpeg, PresentMon), temporary "Fit for Discord" copies | `%LOCALAPPDATA%\Clairvoyance\` |
| Performance-test reports | `<save folder>\perf-tests\` |

- **Video and sound**: only the game window (or the whole screen, if you choose that) and only the
  game's own sound. Your microphone is recorded only if you turn it on, on a separate track.
- **Mouse and keyboard (input overlay, APM)**: recorded only while the game window is focused and
  never while the in-game chat is open, as key codes and cursor positions (never text you type).
  Saved with each game on your PC. You can turn it off in Settings > Games > League.
- **Your League keybinds** are read from League's own settings files and saved with each game, so
  the overlay can show which ability a key cast.

You can delete any of this at any time: delete a game or clip in the app, or the folders above.
Uninstalling Clairvoyance leaves your recordings in place so you don't lose them by accident.

## What Clairvoyance connects to

Clairvoyance talks to the game on your own PC only:

- League of Legends: the Live Client Data API and the League client's local API (both on
  `127.0.0.1`, i.e. your own computer), to know when a game starts and what happens in it.
- Counter-Strike 2: Game State Integration, which sends match events to a listener on
  `127.0.0.1`.

It makes these internet requests, none of which contain your recordings, your Riot ID, your
input data or anything else about you or your games:

| Server | Why | When |
| --- | --- | --- |
| `github.com` (Emre-98/Clairvoyance-releases) | check for updates (`latest.json`); download the update | at start-up and every 4 hours (never during a game; can be turned off); the download only after you click **Update now** |
| `ddragon.leagueoflegends.com` (Riot) | champion, item and summoner spell names and pictures | when the app needs data for a new League patch |
| `static.developer.riotgames.com` (Riot) | the list of League game modes | when the mode list is refreshed |
| `github.com` (BtbN/FFmpeg-Builds) | the free ffmpeg tool for clip export | once, the first time you export or share a clip |
| `github.com` (GameTechDev/PresentMon) | Intel's PresentMon for FPS measurement | only if you run Settings > Performance test |

Like any internet request, these servers see your IP address and a normal request header. Their
own privacy policies apply (GitHub, Riot Games).

**Share** puts a clip on your clipboard; nothing is uploaded. Where you paste it (e.g. Discord) is
up to you.

## Anti-cheat

Clairvoyance captures with Windows Graphics Capture and reads only Riot's and Valve's official
game APIs. It injects nothing into the game, draws no overlay over it and reads no game memory.

## Children

Clairvoyance collects no personal data from anyone, children included.

## Changes

If this policy changes, the new version is published with the release that changes it, and the
date at the top is updated.

## Contact

Questions: open an issue at https://github.com/Emre-98/Clairvoyance-releases/issues.
