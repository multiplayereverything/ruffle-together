# Ruffle Together co-op API (version 2)

Every call a Flash game (AS2 or AS3) can make to Ruffle Together, and what Ruffle Together does for a game
without being asked. The user guide is [ruffle-together.md](ruffle-together.md); the code is
`desktop/src/coop/api.rs` (answers), `frame.rs` (the Stepper), `mice.rs` (every player's mouse),
`session.rs` (the network), `core/src/coop_mice.rs` (one mouse per player in Ruffle's core) and
`core/src/coop_view.rs` (camera and HUD).

## How to call it

Use the library: [tools/coop-api/as3](../tools/coop-api/as3) (copy every `Coop*.as` file into the game's
source) or [tools/coop-api/as2/Coop.as](../tools/coop-api/as2/Coop.as) (`#include "Coop.as"`, any SWF
version). Both have the same names:

```actionscript
var n = Coop.players.count();
Coop.camera.setPosition(1, hero2.x, hero2.y);
if (Coop.mouse.clicker() == 1) { ... }   // inside a button's CLICK / onRelease
```

| Group | Calls |
|---|---|
| `Coop.players` | `count()`, `name(slot)`, `local()`, `isHost()`, `ping(slot)` |
| `Coop.input` | `buttons(slot)`, `held(slot, "FIRE")`, `typed(slot)`, `setKeys(text)` |
| `Coop.mouse` | `x(slot)`, `y(slot)`, `down(slot)`, `clicked(slot)`, `hovered(slot)`, `clicker()`, `setMode(mode)`, `setModeFor(slot, mode)`, `setHover(mode)`, `setCursors(mode)` |
| `Coop.zones` | `give(slot, path)`, `giveBox(slot, x, y, w, h)`, `clear(slot)` |
| `Coop.screens` | `only(path)`, `onlyFor(slot, path)`, `onlyOpenedBy(path, pattern)`, `share(path)` |
| `Coop.camera` | `setPosition(slot, x, y)`, `setZoom(slot, zoom)`, `setView(slot, x, y, zoom)`, `off(slot)` |
| `Coop.view` | `pinHud(path)`, `label(slot, path, text)` |
| `Coop.room` | `version()`, `has(feature)`, `option(name)`, `options()`, `events()` |
| `Coop` | `active()`, `call(name, args...)` (any raw call below) |

Outside Ruffle Together the library answers like a single-player game (1 player, slot 0, no camera), so one
SWF works everywhere. The tables below list what each call does under its raw name: every call is
`ExternalInterface.call(name, args...)`, which the library makes for you. The older flat helper
(`RuffleTogether.as`) still works for mods that use it.

- **In a session or not:** `ExternalInterface.available && ExternalInterface.call("coop.players") > 0`.
- **SWF 6 and 7 (AS1/AS2):** `ExternalInterface` is hidden; unhide it first (the AS2 library does this):
  `ASSetPropFlags(_global, "flash", 0, 4096); ASSetPropFlags(flash.external.ExternalInterface, null, 0, 4096);`
- **Slots** are players 0 to `players - 1`; 0 is the host. A slot outside that range answers `undefined`.

## The one rule: shared or local

| Kind | Meaning | Use it for |
|---|---|---|
| **shared** | The same answer on every PC on the same frame | Anything, gameplay included |
| **local** | Differs between PCs | Only what this PC shows: text, colours, sounds. Never gameplay, never moving, showing or creating display objects |
| **picture only** | Changes only what each PC draws | Cameras, HUD, labels: the game never sees them |

Every PC runs the same game; the sync check compares every object's position, visibility, depth and frame. A
local value that reaches gameplay makes the PCs' games split (the window title says OUT OF SYNC).

## Calls

### Players

| Call | Kind | Answer |
|---|---|---|
| `coop.players` | shared | Number of players, 2 to 8 (as many as the game's key map has slots for) |
| `coop.name(slot)` | shared | That player's name (what they joined with) |
| `coop.localSlot` | **local** | This PC's slot |
| `coop.isHost` | **local** | `true` on the host's PC |
| `coop.ping(slot)` | **local** | This PC's round trip to that player in ms (0 to itself; `undefined` until measured, about 2 s). Friend to friend goes through the host, so it's both trips added |
| `coop.version` | shared | API version: 2 |
| `coop.has(feature)` | shared | `true` if this build has it: `players`, `mouse`, `name`, `camera`, `hud`, `label`, `keys`, `buttons`, `typed`, `options`, `ping`, `events`, `chat`, `clicks`, `zones`, `private`, `hover`, `pointers`, `clicker`, `hovered`, `cursors` |

### Input

| Call | Kind | Answer |
|---|---|---|
| `coop.buttons(slot)` | shared | That player's held buttons as bits: UP 1, DOWN 2, LEFT 4, RIGHT 8, FIRE 16, ALT 32, KEY1 64, KEY2 128, KEY3 256, KEY4 512, VIEW 1024, QUIT 2048, YES 4096, NO 8192, MENU 16384 |
| `coop.button(slot, name)` | shared | `true` while that player holds the button (`"FIRE"`, `"KEY1"`...); `undefined` for an unknown name |
| `coop.mouseX(slot)`, `coop.mouseY(slot)` | shared | That player's mouse in stage pixels (each PC's cursor goes through its own camera, so these are game coordinates). (0, 0) until it moves |
| `coop.mouseDown(slot)` | shared | `true` while that player's left button is down |
| `coop.clicked(slot)` | shared | `true` on the frame that player pressed their left button (whether or not the click reached the game) |
| `coop.mouse.clicker` | shared | Inside a mouse handler (`CLICK`, `MOUSE_DOWN`, `onPress`, `onRelease`...), the slot whose mouse caused it; `undefined` elsewhere. One "Ready" button for everyone: `ready[clicker] = true` |
| `coop.mouse.hovered(slot)` | shared | The path of what that player's mouse is on (`"_root.menu.sword"`), as of the last frame; `undefined` while it is on nothing clickable or out of the game |
| `coop.typed(slot)` | shared | What that player typed this frame, `"\b"` for Backspace and `"\n"` for Enter. Calling it tells Ruffle the game wants every typed character for the next 60 frames (see "Typing" below) |
| `coop.keys(text)` | shared | The game's own key map, in the `.keys` format (`"slot 0: UP=38 FIRE=32\n..."`), used from the next frame on every PC; held keys are let go first; the same map again changes nothing. `true` if it parsed |

`coop.buttons` is the easy way to add players to a single-player game: read each slot's buttons and move
each character, instead of faking keyboard keys.

### Every player's mouse

Every player has a mouse of their own in the game: Ruffle runs hover, press, release and drag for each one,
with the game's own events (`ROLL_OVER`, `CLICK`, `onRelease`...). Inside a handler, `stage.mouseX` /
`_xmouse` is the mouse that caused it, and a `startDrag` follows the mouse that started it. Several players
can press and hold at once. An object rolls over for the first mouse on it and out for the last one.

By default only the host's mouse is in the game (every PC still knows every player's mouse through
`coop.mouseX` and friends). A friend's mouse is in the game only where the rules let it be; anywhere else it
is outside the stage, so it neither hovers nor clicks:

| Call | Kind | Effect |
|---|---|---|
| `coop.mouse(mode)` | shared | Every friend's mouse: `"host"` (the default): not in the game. `"zones"`: only on something clickable inside their own zone (below), like their own card on a character screen; the host's is everywhere else. `"buttons"`: on anything the game made clickable (what shows the hand cursor: buttons, button-mode clips, links). `"all"`: anywhere. Resets `coop.mouse(slot, mode)` |
| `coop.mouse(slot, mode)` | shared | One friend's mouse (slot 1 to 7), over what `coop.mouse(mode)` says: `coop.mouse(2, "host")` keeps player 3 out |
| `coop.zone(slot, path)` | shared | The buttons inside the clip at `path` (`"_root.players.p2"`) belong to that player: their own card on a character screen. Nobody else's mouse may click them, the host's included. It only applies while that clip is on show, so other screens are untouched. When zones nest, the most exact clip wins (`zone 0: _root.players.p2.combo` gives a card's drop-down back to the host) |
| `coop.zone(slot, x, y, w, h)` | shared | The same for a box (stage pixels). A box applies on every screen, so prefer a clip. `coop.zone(slot)` clears that player's zones |
| `coop.hover(mode)` | picture only | How each screen draws Button symbols while friends have mice. `"own"` (the default): as its own player's mouse sees them, in the game's own Up, Over and Down art (AS3 `SimpleButton` and AS2 buttons; an AS2 state with a movie clip the button doesn't show right now is drawn as the game has it). `"shared"`: as the game has them, every player's hover on every screen. Hover made by the game's code (a clip that moves on `ROLL_OVER`) is game state, so it shows on every screen either way |
| `coop.mouse.cursors(mode)` | picture only | Friends' cursors on each screen: `"auto"` (the default): only while one is over something that player may click; `"on"`: always; `"off"`: never. A cursor that hasn't moved for 3 seconds fades out |

A key map sets the same without changing the SWF: `mouse: zones`, `mouse 2: host`, `zone 1: _root.players.p2`,
`hover: own`, `cursors: on`. Chaos Faction 2's built-in map does this for its character screen.
To find a button's path for a zone, turn friends' mice on (`mouse: buttons`) and click it once: the log
(`%LOCALAPPDATA%\ruffle\log\ruffle.log`) says e.g. `player 2 clicks _root.players.p2.b10 (463, 272)`. The mouse
debug overlay (`--coop-mouse-debug`) shows every player's mouse, what it is on, and whether the rules let it be
there.

### Private screens

A pop-up that only one player sees and uses: a player editor, a shop, an inventory. Every PC runs the same
game, so the pop-up is open in everyone's game; what changes is the picture and the mouse, the same way on every
PC (no desync).

| Call | Kind | Effect |
|---|---|---|
| `coop.only(path)` | shared | The clip at `path` (`"_root.pcb"`) belongs to whoever's click opened it, while it is on show: only their PC draws it, only they click it, and everyone else's mouse goes through it to what is underneath (their cursor isn't shown over it either) |
| `coop.only(path, pattern)` | shared | The same, and while it is someone's nobody else may click the buttons that open it (`"_root.players.*.instance*"`: `*` is any one name, `instance*` any name starting so). For a game that has one of the pop-up for everyone (one Player Editor): a second player opening theirs would take over the first one's. A refused player sees "Bob has that open: wait until they close it" |
| `coop.only(slot, path)` | shared | The same, always that player's |
| `coop.shared(path)` | shared | Everyone sees and clicks it again |

A key map does the same without changing the SWF: `private: _root.pcb opened-by _root.players.*.instance*`
(Chaos Faction 2's Player Editor and its Edit Player buttons). It works with friends' mice (`mouse: zones` or
`buttons`): the player who opened the pop-up clicks it even outside their zone. Limits: the pop-up is still open in everyone's game, so a game that freezes the screen behind
its pop-ups freezes it for everyone; `BitmapData.draw` of the stage still sees it on every PC.

### Host settings

| Call | Kind | Answer |
|---|---|---|
| `coop.option(name)` | shared | The host's `--coop-option NAME=VALUE` as text, or `undefined` |
| `coop.options` | shared | Every option, as an object |

The host picks them when starting (the launcher can add them); a friend's own are ignored. The SWF decides
what each name means and its default. For a launcher dropdown, list them in the mod's mods-info entry.

### Camera, HUD, labels (picture only)

| Call | Effect |
|---|---|
| `coop.camera(slot, x, y, zoom)` | That player's camera: the stage point (x, y) at the middle of their screen, `zoom` 1 = normal (default 1). Call it for **every** slot (each frame, or when it changes): every PC holds all cameras and draws with its own slot's |
| `coop.camera(slot)` | Turns that player's camera off |
| `coop.hud(path)` | The clip at `path` stays fixed on screen. It must be a direct child of the root (`"_root.hud"`, `"root.hud"`) |
| `coop.label(slot, path, text)` | A name tag above the clip at `path`, drawn by Ruffle over the game on every PC; `""` removes it |

The camera only changes the final picture on screen: positions, hit tests, `BitmapData.draw` of the stage and
the sync check never see it. Paths are `_root` / `_level0` / `root` and then instance names joined by dots.

### Events and chat

| Call | Kind | Effect |
|---|---|---|
| `coop.events` | shared | A list of what happened since the last call, oldest first (at most 100 kept): `"join N"` (every player at the start), `"leave N"` (a friend left; from this frame they hold nothing), `"chat N text"` (a chat line). Read it once per frame |

## What every game gets without calling anything

| Feature | What happens |
|---|---|
| **Key maps** | Each player's keys press that player's keys in the game (`--coop-keys NAME` or a `.keys` file; 13 built in) |
| **Typing** | What players type goes into the game's selected text box on every PC (names, chat), in player order, Backspace and Enter included. While no text box is selected, typed text only reaches `coop.typed`; instead each player's key press types its game key's character (from any key map, `coop.keys` included, and in scripts and replays), so `on(keyPress "1")` buttons work but stray keys can't trigger a game's shortcuts. A paste becomes the sender's clipboard text (64 characters at most); copy is ignored and cut deletes |
| **Leaving** | A friend who leaves or drops out no longer stops the game: everyone else plays on and sees "Sam left". Losing the host still ends it |
| **Private screens** | With `private: <clip>` in the key map, a pop-up only the player who opened it sees and clicks |
| **Friends' cursors** | Off by default. While friends have mice (`mouse: zones`, `buttons` or `all`), a player's mouse shows in the other windows (an arrow in their colour with their name, picture only) only while it is over something that player may click, e.g. their own card on a character screen (`cursors:` changes this) |
| **Chat** | `/` opens a chat box (unless the key map gives `/` to the game); Enter sends, Esc closes. Keys don't reach the game while it's open. Lines show bottom left and reach the game as `chat` events |
| **Same game everywhere** | Clock, `Date`, `Math.random`, timers, sound endings, script time limits and window size are the same on every PC |
| **Save** | Everyone starts from the host's co-op save (`--coop-save coop\|solo\|new`), and every PC keeps the session's saves in its own co-op save, so anyone can host next time |
| **Sync check** | Every 25 frames; the title says OUT OF SYNC and each PC writes a report of what differs |
| **Discord** | "Ruffle Together: Playing GAME, room ABCDE (2 of 2)" on the profile of anyone who shares their activity. A Join button for relay rooms comes once the join page is set (see `desktop/src/coop/discord.rs`) |

## Join links

`ruffle-together://join/<mod id>/<room code>` (for example `ruffle-together://join/gm2/ABCDE`). The launcher
owns the link: it finds the player's copy of the game and starts Ruffle with `--coop-join-relay` and
`--coop-code`. Discord's Join button opens `JOIN_PAGE/<mod id>/<room code>`, a web page that opens the link
(Discord buttons only allow https). The launcher passes `--coop-link-id <mod id>` so a host's profile gets
the button.

## Command line (for launchers and tests)

| Flag | Meaning |
|---|---|
| `--coop-host PORT`, `--coop-host-relay RELAY` | Host directly or through the relay (prints the room code) |
| `--coop-join ADDRESS`, `--coop-join-relay RELAY --coop-code CODE` | Join |
| `--coop-players N` | Host: 2 to 8 players (as many as the key map has slots for) |
| `--coop-keys NAME_OR_FILE` | The key map (every player the same) |
| `--coop-name NAME` | This player's name |
| `--coop-option NAME=VALUE` | Host: a game setting for `coop.option` (repeat for more) |
| `--coop-link-id ID` | The game's mods-info id, for Discord's Join button |
| `--coop-delay N` | Host: a fixed input delay in frames |
| `--coop-replay SWF --coop-script FILE --coop-out FILE` | Test: run headless and write every frame's fingerprint |
| `--coop-headless`, `--coop-frames N` | Test: a whole session without a window |
| `--coop-mouse-debug` | Test: show every player's mouse, what it is on, and whether the rules let it be there |

## Demo

[tools/coop-api/demo](../tools/coop-api/demo/README.md) is a small AS3 game that uses every call above and shows
the answers on screen, with a key map and input scripts for replays, headless sessions and two real windows.

## Input scripts (tests and labs)

```text
p0 UP|FIRE        player 0 holds these from now on (any button name, or 0)
wait 25           25 frames with what is held now
click 275 300 1   player 1's mouse: one frame pressed at stage pixel (275, 300)
move 100 50 2     player 2's mouse moves (no player: the host's)
type 1 Sam{Enter} player 1 types "Sam" then Enter (8 characters a frame; {Backspace} too)
press 100 50 2    player 2's mouse moves there and holds its button, no frame passes (so several
release 2          players can press in the same frame); release lets go, no frame passes
```
