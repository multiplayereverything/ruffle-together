# Ruffle Together

Version 1.0.4 (build id `rc-1.0.4#<commit>`; players must use the same build).

Play a local-multiplayer Flash game online with up to 3 friends. Every PC runs the same game in this Ruffle
build; only the key presses travel, and every frame runs on every PC with every player's keys. Everyone gets
the same small input delay (the host picks it from the ping: 1 frame on a LAN), and the games are compared
every 25 frames: if they ever differ, every window says so ("THE TWO GAMES ARE OUT OF SYNC: restart the level").

Games with keys: Territory War, 2 to 4 players hot-seat (`--coop-keys territory-war`), Bad Ice-Cream, 2 players (`BadIceCream_coop.swf`, made by the Bad Ice-Cream mod's
`build.py --no-net`; the default), and Gun Mayhem 2, 2 to 4 players (`--coop-keys gun-mayhem-2`).
Every player needs the **same game file** and the **same Ruffle Together build**; anything else is refused
when they connect.

## Play (v1: command line)

Host, friends connect to your IP (LAN, Radmin VPN, or a forwarded port):

```
ruffle_desktop --coop-host 7801 --coop-name Liam BadIceCream_coop.swf
```

Friend:

```
ruffle_desktop --coop-join <host IP>:7801 --coop-name Sam BadIceCream_coop.swf
```

Through the central relay with a room code (no IP needed):

```
ruffle_desktop --coop-host-relay 165.232.82.11:25569 BadIceCream_coop.swf      (prints the room code)
ruffle_desktop --coop-join-relay 165.232.82.11:25569 --coop-code ABCDE BadIceCream_coop.swf
```

The relay needs the `ruffle-coop` game in mods-info first.

- Each player uses their own normal keys: arrows or WASD to move. Bad Ice-Cream: Space, Enter or F to shoot.
  Gun Mayhem 2: Z or T to shoot, X or Y for a grenade. The host is player 1, friends follow in join order.
- `--coop-players N` (host): start with N players (2-4, as many as the game has keys for); the game starts
  once all have joined. `--coop-keys <game>`: every player must give the same.
- Every player has their own mouse, but by default only the host's is in the game. A game's key map (or the game)
  lets friends' mice in where they belong, e.g. each player on their own card of a character screen; there they
  hover and click like the host's, several players at once (see the co-op API below).
- `--coop-delay N` (host): a fixed input delay in frames. Without it the host pings the friends when they
  connect and picks the smallest delay that runs smoothly (about 1 frame per 67 ms of ping; with 3-4
  players a friend's keys go through the host, so the two worst pings count together).
- While everyone joins, every window (the friends' too) shows the room code (or the port), who has joined and how
  many the game needs, or why it can't start. Friends who join early wait as long as the last one takes.
  During the game the window title shows the room code, your player number, and "waiting for…", "… left" or
  "OUT OF SYNC".
- Saves: everyone starts from the host's co-op save for this game (its solo save the first time), so every PC
  has the same unlocked levels. Whenever the game saves, every PC writes the session's saves to its own co-op
  save, `%LOCALAPPDATA%\ruffle\SharedObjects\coop\<game file>.save` (under `--save-directory` if given), so any
  player can host next time and carry on. Solo saves are never changed. Delete the file to start the co-op game
  over. Once the games are OUT OF SYNC nothing more is saved.
- `--coop-save coop|solo|new` (host): which save everyone starts from. `coop` (the default) is the co-op save
  as above; `solo` is the host's solo save even if it has a co-op one; `new` starts the game as if new. With
  `solo` or `new` the host's co-op save is replaced the first time the game saves (each friend's too).
- **Typing:** names and chat typed in the game's own text boxes reach the game for everyone (any key, capitals,
  Backspace, Enter, paste). While no text box is selected, only your mapped keys type: each types the game
  key it presses (so menus' keyPress buttons work, also in scripts and replays).
- **Chat:** `/` opens a chat box; Enter sends, Esc closes. Your keys don't reach the game while it's open.
- **Leaving:** a friend who leaves or drops out doesn't stop the game: everyone else plays on ("Sam left").
  If the host leaves, the game ends.
- `--coop-option NAME=VALUE` (host): a setting the game reads with `coop.option` (for mods that support it).
- One Ruffle window at a time: opening another game closes the one still open (per cache folder; give each
  window its own `--cache-directory` to run several on one PC, as the labs do).
- Every 5 seconds the log (`%LOCALAPPDATA%\ruffle\log\ruffle.log`) gets a "Ruffle Together timing" line:
  frame rate, time per frame, and how long the game waited for the other PC's keys. The last 5 runs stay as
  `ruffle.1.log` (the one before) to `ruffle.5.log`.
- When a game stops, every PC's log says why: "X left (closed the game)", "X left (Ruffle crashed: ...)",
  "X's Ruffle is still running, but its game hasn't moved past frame N" (it froze), "nothing at all from X"
  (stopped or network down), or a connection that closed without a goodbye. The PC that froze logs it too.

## For game builders: key maps

`--coop-keys` takes a built-in map name or the path of a key map file (every player must use the same map; a
different one is refused by name). Built in: bad-ice-cream, gun-mayhem-2, territory-war, bow-man, smileys-war,
stealth-assassin, stealth-hunter-2, intrusion, intrusion-2, air-battle, ultimate-crab-battle, fireboy-watergirl, chaos-faction-2 (the files are in `desktop/src/coop/keys/`).

```
# comment
shared                                    every slot presses the same keys (a hot-seat game)
slot 0: UP=38 DOWN=40 LEFT=37 RIGHT=39 FIRE=90 ALT=88
slot 1: UP=87 DOWN=83 LEFT=65 RIGHT=68 FIRE=84 ALT=89
local: UP=38,87 DOWN=40,83 LEFT=37,65 RIGHT=39,68 FIRE=90,84 ALT=88,89
```

- `slot N:` (0-7, in order): what that player's buttons press in the game, as Flash key codes. The number of
  slots is how many players the game can have.
- `local:`: the keys a player presses on their own PC for each button (key codes; several per button).
- Buttons: UP DOWN LEFT RIGHT FIRE ALT KEY1 KEY2 KEY3 KEY4 VIEW QUIT YES NO MENU (the same names as in input scripts).
- Key codes: letters 65-90, digits 48-57, numpad 0-9 96-105 and * + - . / 106/107/109/110/111 (sent with the
  numpad location), Space 32, arrows 37-40, Enter 13, Shift 16, Ctrl 17, Alt 18, Esc 27, Backspace 8, Tab 9,
  PageUp/Down 33/34, End/Home 35/36, Insert/Delete 45/46, ; = , - . / ` [ \ ] ' 186-192 and 219-222.

## For game builders: the co-op API (AS2 and AS3)

**The full reference, every call included, is [coop-api.md](coop-api.md).** The basics:

A game made for Ruffle Together can ask about the players through `ExternalInterface`, the same way in AS2
(`flash.external.ExternalInterface`) and AS3 (`flash.external.ExternalInterface`):

| Call | Returns |
|---|---|
| `ExternalInterface.call("coop.players")` | number of players in the session (2-4) |
| `ExternalInterface.call("coop.localSlot")` | this PC's player slot (0 = host). **Never for gameplay**, see below |
| `ExternalInterface.call("coop.mouseX", slot)` | that slot's mouse x, in stage pixels (the same space as `_root._xmouse` / `stage.mouseX`) |
| `ExternalInterface.call("coop.mouseY", slot)` | that slot's mouse y |
| `ExternalInterface.call("coop.mouseDown", slot)` | `true` while that slot's left mouse button is down |
| `ExternalInterface.call("coop.mouse.clicker")` | inside a mouse handler, the slot whose mouse caused it |
| `ExternalInterface.call("coop.version")` | the API version (2) |
| `ExternalInterface.call("coop.has", feature)` | `true` if this build has the feature: `"camera"`, `"pointers"`, `"clicker"`, ... (the full list is in coop-api.md) |
| `ExternalInterface.call("coop.name", slot)` | that player's name |
| `ExternalInterface.call("coop.isHost")` | `true` on the host's PC. **Never for gameplay**, like `localSlot` |
| `ExternalInterface.call("coop.camera", slot, x, y, zoom)` | sets that slot's camera: stage point (x, y) at the middle of that player's screen, `zoom` 1 = normal. `("coop.camera", slot)` turns it off |
| `ExternalInterface.call("coop.hud", path)` | the clip at `path` (e.g. `"_root.hud"`) is drawn without the camera |
| `ExternalInterface.call("coop.label", slot, path, text)` | a name tag above the clip at `path`, on every PC; `""` removes it |
| `ExternalInterface.call("coop.keys", text)` | the game's own key map (the `.keys` format below), used from the next frame |

**Easier: the library.** `tools/coop-api/as3/Coop*.as` (AS3: copy every file) and `tools/coop-api/as2/Coop.as`
(AS2, any SWF version: `#include "Coop.as"`; it does the SWF 6/7 unhide step for you) group every call:
`Coop.players.count()`, `Coop.players.name(slot)`, `Coop.mouse.x(slot)`, `Coop.mouse.clicker()`,
`Coop.camera.setPosition(slot, x, y)`, `Coop.view.label(slot, path, text)`, `Coop.input.setKeys(text)` and the rest
(the groups are listed in coop-api.md). Outside Ruffle Together they fall back to single-player answers (1 player,
slot 0, no camera), so one SWF works everywhere. The older flat `RuffleTogether.as` still works.

- **In co-op or not:** `ExternalInterface.available && ExternalInterface.call("coop.players") > 0`. In plain Ruffle
  or Flash Player this is false: use the normal mouse and treat this PC as slot 0.
- **AS1/AS2 SWFs older than version 8:** `ExternalInterface` is hidden from them, so `call` returns nothing. Unhide
  the package and the class first:
  `ASSetPropFlags(_global, "flash", 0, 4096); ASSetPropFlags(flash.external.ExternalInterface, null, 0, 4096);`
  (found by the Bow Man session, SWF 6).
- Every PC's mouse travels with its keys, so every answer except `localSlot` is the same on every PC for the same
  frame. A slot whose mouse hasn't moved yet reads (0, 0), up. A slot number outside 0..players-1 returns undefined.
- **Every player's mouse:** the game's own mouse events (`onRelease`, `CLICK`, roll over and out) come from every
  player whose mouse is in the game (`coop.mouse`, zones); inside a handler `_xmouse` / `stage.mouseX` is that
  player's and `coop.mouse.clicker` says who. Outside handlers `_xmouse` / `stage.mouseX` is the host's. A game that
  aims with the mouse for every player reads each slot through the API, slot 0 included.
- **`localSlot` rule:** it is different on every PC, so nothing the game simulates may depend on it, and it must
  not move, show/hide, create or remove display objects: the sync check compares every object's position,
  visibility, depth and frame on every PC, and a difference shows OUT OF SYNC. Safe: the text of a text field that
  exists on every PC (for example "YOU" over your player), colour/alpha tints, sounds. Don't move the world to
  follow the local player: use `coop.camera` instead.
- **Cameras (one per player):** call `coop.camera` for **every** slot each frame (or whenever it changes), not
  just your own: every PC then holds the same cameras, and each PC draws with its own slot's. The camera only
  changes the picture on screen; the game itself (positions, hit tests, `BitmapData.draw` of the stage, the sync
  check) never sees it. Each PC's mouse goes through its own camera, so `coop.mouseX/Y(slot)` and the host's
  normal mouse stay in game coordinates.
- **HUD:** mark score bars and menus with `coop.hud(path)` so they stay put. A HUD clip must be a direct child of
  the root (`_root.hud`, `root.hud`); the mark lasts as long as the clip.
- **Labels:** Ruffle draws them over the game (white text with a shadow, centred above the clip's top edge),
  following each PC's camera; the game never sees them. Paths are `_root` / `root` then instance names with dots.
- **`coop.keys`:** the map replaces `--coop-keys` from the next frame, on every PC at once (held keys are let go
  first; the same map again changes nothing). A game can switch maps per mode, e.g. menus vs. a match.
- In a session `ExternalInterface.available` is true and every other call returns undefined; there is no
  JavaScript.
- Input scripts drive any slot's mouse: `move X Y slot`, `click X Y slot` (the host's when the slot is left out).

## Testing (no windows, no real keyboard)

- `--coop-replay <swf> --coop-script <file> --coop-out <file>`: runs the game headless and writes its
  fingerprint every frame. The same script must give the same file every time.
- `--coop-headless` with the host/join flags and `--coop-script`: a whole session without a window; prints
  `COOP RESULT ...` (sync, input delay).
- `python tools/coop-lab/lab.py --swf <swf> [--players 4 --keys gun-mayhem-2]`: players on one PC, through
  a local relay with lag and directly.
- `python tools/coop-lab/windows_lab.py --swf <swf>`: two real windows, checks that every frame reaches the
  screen, with and without mouse-move messages (posted to the two windows only).
- `COOP_TEST_SWF=<swf> cargo test -p ruffle_desktop --release coop::` runs the session tests on the game.

## Known limits (v1)

- Only the keys in the game's key map reach the game as keys (P, M and the like don't, unless a map names
  them); `/` opens the chat unless the map gives it to the game.
- If the host leaves, the session ends for everyone (a friend leaving doesn't end it).
- During a session there is no menu, no fullscreen, and no opening another file; the stage keeps the game's
  own size (the window can still be resized).
- Sounds end by the game's frame clock, not the PC's audio device: `onSoundComplete` / `soundComplete` and a
  sound's `position` are the same frame on every PC (and on a PC without sound). Timeline stream sounds still
  follow the device. Games that loop over a Dictionary keyed by objects may drift; the sync check warns if they do.
- `_xmouse` / `mouseX` are the same on every PC whatever the window's size (rounded at the stage's own pixels).
- Network loads (URLLoader, Loader from the web) don't happen in a session.
