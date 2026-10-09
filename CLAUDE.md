# Clairvoyance (Tauri 2: Rust backend + Svelte 5 UI, Windows game recorder)

Read PLAN.md first (status, performance rules, known issues, next steps). Open docs/HISTORY.md
only for background on a feature.

## Where things live
- League events: games/league/src/events.rs, lib.rs. Ult / HUD checks: games/league/src/hud.rs, ult.rs
  (summoner D / F slots too), verify.rs. Item chips: items.rs (+ ddragon.rs data), gold: gold.rs,
  summoner chips: summoners.rs. Scoreboard: crates/cv-core/src/scoreboard.rs, ui/src/components/Scoreboard.svelte
- Timeline UI: ui/src/components/Timeline.svelte, ui/src/lib/eventmeta.ts
- Mechanics / APM: ui/src/components/Mechanics.svelte. Player: ui/src/components/Player.svelte
- Encoder: crates/cv-capture/src/win/video_enc.rs (codec / rate-control choice: encopts.rs; muxer
  for H.264 / HEVC / AV1: mp4.rs). Settings: crates/cv-core/src/settings.rs. Encoder measurement:
  scripts/encoder-compare.ps1 (owner's PC), scripts/encoder-test/ (Linux, software stand-ins)
- Deadlock: games/deadlock (signals.rs: match detection from Steam's log + the console log; lib.rs: the
  game; paths.rs: Steam / game folders; diag.rs: the "log match
  signals" developer option), wired in app/src/devopts.rs; plan and owner steps in PLAN.md "Deadlock"
- Mock game: crates/cv-mock-league. Game-integration trait: crates/cv-core/src/game.rs (optional
  capabilities: CursorInput, RecordingCheck, WatchDetection, ModeRules), docs/ADDING_A_GAME.md
- Clip export with the overlay: crates/cv-capture/src/overlay_export.rs, app/src/export.rs,
  ui/src/components/ClipEditor.svelte. Timeline filters: ui/src/lib/timelinefilters.ts
- Copy protection: crates/cv-seal (tamper seal, `cv-seal keygen | seal | verify`), app/src/integrity.rs
  (start-up check), cold-chunk obfuscation in ui/vite.config.ts. Setup in RELEASING.md
- Share / Fit for Discord: crates/cv-capture/src/share.rs (size ladder, ffmpeg), app/src/share.rs
  (command, clipboard), ui/src/components/ShareButton.svelte, ShareFitToggle.svelte

## Rules
- Core crates (crates/cv-core, cv-capture) contain no game-specific code; that lives in games/*.
- Windows-only code (crates/cv-capture/src/win, app/) can't run in the Linux cloud container. It is
  compiled and linted by CI's `windows` job, and here by cross-checking:
  `apt-get install -y gcc-mingw-w64-x86-64`, `rustup target add x86_64-pc-windows-gnu`, then
  `cargo clippy --target x86_64-pc-windows-gnu --workspace --all-targets -- -D warnings`
  (needs `ui/dist`: run `npm --prefix ui run build` first). Running it needs the owner's PC.
- Stay within the PLAN.md "Performance rules". Never launch anything Riot (see PLAN.md).

## Workflow
After a milestone: update PLAN.md "Current status" and CHANGELOG.md, then say in a few lines how to
test it and how to check its performance impact.

## Testing (commands confirmed to work in the Linux container)
Fast (every small change):
- UI unit tests: `node --experimental-strip-types --test ui/tests/*.unit.test.ts` (no npm script; first `npm ci --prefix ui`)
- UI type check: `npm --prefix ui run check` (svelte-check)
- Rust, per crate: `cargo test -p cv-core` (settings, library, modes, engine, session logic),
  `-p cv-game-league` (events, ult/HUD/verify, binds, queues; biggest suite), `-p cv-game-cs2`,
  `-p cv-game-deadlock` (Valve file parsing, the signal logger),
  `-p cv-capture` (portable parts: fragmented MP4 muxer), `-p cv-mock-league` (mock server)
- Rust lint (CI gate, Rust 1.99 pinned in ci.yml): `cargo fmt --all --check` and
  `cargo clippy -p cv-core -p cv-game-league -p cv-game-cs2 -p cv-game-deadlock -p cv-capture -p cv-mock-league --all-targets -- -D warnings`;
  run them with the CI version (`cargo +1.99 ...`): newer clippy versions add lints
Slow (only before pushing, once):
- `cargo test --workspace --exclude clairvoyance` (plain `--workspace` fails on Linux: app/ needs GTK)
- `npm --prefix ui run build`, then what CI runs (.github/workflows/ci.yml): rustfmt, clippy, cargo
  test -p cv-core -p cv-game-league -p cv-game-cs2 -p cv-game-deadlock -p cv-capture, the UI check, build and unit
  tests above, and the Windows clippy (cross-check above)
- Browser e2e (playwright-core, Chromium, mock backend): `VITE_MOCK=1 npx vite` in ui/, then
  `node tests/layout.test.mjs http://localhost:5173 <outdir> --quick` (also bubbles, fullscreen,
  framestep, overlay, filters, export, timelinechips, scoreboard, share tests in ui/tests/; `python3 ui/tests/make-sample.py` makes
  their sample videos first). Needs `ln -s /opt/node-tools/node_modules/playwright-core
  ui/node_modules/`. Here layout --quick passed 42/63; the other 21 page states time out (timing/
  network in the container), so treat failures there as unverified and check on the owner's PC.
Can't run on Linux / need real League: everything under crates/cv-capture/src/win (capture, encoder,
input, thumbnails), app/ (Tauri shell, updater), ignored tests, the replay benchmark and
end-to-end tests with the real recorder, and anything involving the League client or a live game.
