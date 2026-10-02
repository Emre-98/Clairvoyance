# Adding a game

Clairvoyance's core (engine, recording, timeline, storage, UI) knows nothing about any
specific game. Each game is a small Rust crate under `games/` that implements one trait,
`GameIntegration` (in `crates/cv-core/src/game.rs`). League (`games/league`) and
Counter-Strike 2 (`games/cs2`) are the two reference implementations.

## 1. Create the crate

```
games/mygame/
  Cargo.toml     # depends on cv-core (+ whatever you need to read the game's API)
  src/lib.rs     # pub struct MyGameIntegration; impl GameIntegration for it
```

Add `"games/mygame"` to `members` in the root `Cargo.toml`, and add the crate to
`app/Cargo.toml`.

## 2. Implement `GameIntegration`

| Method | What to return |
|---|---|
| `id()` | Stable id used in settings and session files, e.g. `"valorant"` |
| `name()` / `short_name()` | Display name / short name for file names (`"Valorant"`) |
| `process_names()` | Lower-case exe names, e.g. `&["valorant-win64-shipping.exe"]` |
| `capture()` | The game's exe (the recorder captures its largest window); set `display_capture_only: true` to always capture the whole monitor instead (e.g. games whose fullscreen window comes out black) |
| `supports_events()` | `false` if the game has no event API (you still get recording, hotkey clips and manual markers) |
| `poll()` | Called ~once per second while the game runs. Return the phase, the in-game clock (`game_time`), **only new** events, player info, stats and the result |
| `start()` / `stop()` | Reset per-match state (and start a local server if the game pushes data, like CS2) |
| `match_only()` | `true` for games you keep open between matches (CS2): recording then starts when a match starts, not when the exe starts |
| `on_key()` | Optional: turn key presses (made while the game is focused) into events, like League's "Ult pressed" |
| `input_tracking()` | `true` for games played with the cursor (League): mouse and keyboard input is recorded for the replay's input overlay and the Mechanics stats (the core adds the "Record mouse & keyboard input" and sample-rate settings). `false` for mouse-look games (CS2) |
| `chat_open()` | With `input_tracking`: whether the game's chat is open right now (from the keys `on_key` saw); no keys are recorded meanwhile |
| `mouse_marks()` | Optional: after the game, turn recorded mouse-button presses into key marks for `verify_recording` (League: ult bound to a side button) |
| `action_keys()` / `default_action_keys()` / `action_categories()` | Optional, with `input_tracking`: the game's "action keys" for the replay's ability bubbles: each action (`ActionKey`: id, label shown in the bubble, category, size, colour, the game's default key for the hint) with every key/mouse bind (`ActionBind`: virtual-key code or mouse button + Ctrl/Shift/Alt; only exact matches give a bubble). `action_keys()` is called after `start()` and saved with the session (binds change between games); `default_action_keys()` is used for older recordings; the categories become the overlay's sub-toggles. League: abilities, summoners, items, ward from League's own settings. Return nothing (CS2) for no bubbles |
| `action_press_states()` | Optional: refine how presses are drawn from what the game knows after the game (League: R presses follow the ult check: "Ult used" solid, "no cast" faded) |
| `config_fields()` / `default_config()` / `configure()` | Optional settings shown in Settings > Games (text, single key, checkbox or a select list) |

Rules that keep the rest of the app working:

- **Never return an event twice.** Keep the ids you've seen (League) or diff counters between
  snapshots (CS2). The engine also drops duplicate ids, but don't rely on it.
- **`game_time` must be in seconds on the same clock as your events' `game_time`.** The engine
  uses it to compute the video offset (`video position = game time + offset`). If the game has
  no clock, return seconds since `start()` (CS2 does this).
- Map everything to the shared `EventKind`s (kill, death, assist, multikill, objective, round, …)
  so the timeline colours, filters, auto-clips and callouts work without UI changes.
- No memory reading and no injection: use official APIs only (anti-cheat safe).

## 3. Register it

In `app/src/games.rs`:

```rust
pub fn all() -> Vec<Box<dyn GameIntegration>> {
    vec![
        Box::new(cv_game_league::LeagueIntegration::new()),
        Box::new(cv_game_cs2::Cs2Integration::new()),
        Box::new(cv_game_mygame::MyGameIntegration::new()),
    ]
}
```

That's it: detection, recording, the timeline, clips, the library and the settings page pick
it up automatically.

## 4. Test it

- Unit-test the translation from the game's data to `GameEvent`s with recorded JSON
  (see `games/league/src/events.rs` and `games/cs2/src/tests.rs`).
- `crates/cv-core/src/engine/tests.rs` shows how to run the whole engine against a fake
  game and a fake recorder.

## Notes on specific games

- **Counter-Strike 2** (done): official Game State Integration. The module writes
  `gamestate_integration_clairvoyance.cfg` into CS2's cfg folder (found through Steam);
  restart CS2 once after the first install. Captures the whole monitor (its exclusive-fullscreen window
  can come out black with window capture).
- **Valorant**: no official live event API, and Vanguard forbids reading game data. Add it
  with `supports_events() = false`: you get automatic recording, hotkey clips and manual
  markers.
- **Dota 2**: has Game State Integration like CS2 (kills, deaths, Roshan, game clock), so a
  module would look very similar to `games/cs2`.
