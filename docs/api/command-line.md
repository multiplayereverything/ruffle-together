# Command line

For launchers, scripts and tests. The SWF's path comes last.

## Playing

| Flag | Meaning |
|---|---|
| `--coop-host PORT` | Host directly on that port |
| `--coop-host-relay RELAY` | Host through the relay; prints and shows the room code |
| `--coop-join ADDRESS` | Join directly (`IP:PORT`) |
| `--coop-join-relay RELAY --coop-code CODE` | Join through the relay with a room code |
| `--coop-players N` | Host: 2 to 8 players (as many as the game has); the game starts once all have joined |
| `--coop-keys NAME_OR_FILE` | The key map. Every player must use the same |
| `--coop-name NAME` | This player's name |
| `--coop-save coop\|solo\|new` | Host: which save everyone starts from |
| `--coop-option NAME=VALUE` | Host: a game setting for `coop.option` (repeat for more) |
| `--coop-delay N` | Host: a fixed input delay in frames instead of the automatic one |
| `--coop-link-id ID` | The game's mods-info id, for Discord's Join button |

The central relay is `165.232.82.11:25569`.

## Testing

| Flag | Meaning |
|---|---|
| `--coop-replay SWF --coop-script FILE --coop-out FILE` | Run headless from an input script and write every frame's fingerprint |
| `--coop-frames N` | Stop after N frames |
| `--coop-headless` | A whole session (with the host or join flags) without a window; prints `COOP RESULT ...` |
| `--coop-script FILE` | The [input script](input-scripts.md) |
| `--cache-directory DIR` | Give each window its own, to run several on one PC |
| `--coop-mouse-debug` | Show every player's mouse, what it is on, and whether the rules let it be there. Since 1.0.4 |

## Files

| File | What it is |
|---|---|
| `%LOCALAPPDATA%\ruffle\log\ruffle.log` | The log; `ruffle.1.log` to `ruffle.5.log` are the runs before |
| `%LOCALAPPDATA%\ruffle\SharedObjects\coop\<game file>.save` | The co-op save (under `--save-directory` if given) |
