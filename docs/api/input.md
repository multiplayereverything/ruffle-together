# Input

| Call | Kind | Answer |
|---|---|---|
| `coop.buttons(slot)` | shared | That player's held buttons, as bits (below) |
| `coop.button(slot, name)` | shared | `true` while that player holds the button (`"FIRE"`, `"KEY1"`...); `undefined` for an unknown name |
| `coop.mouseX(slot)`, `coop.mouseY(slot)` | shared | That player's mouse in stage pixels. Each PC's cursor goes through its own camera, so these are game coordinates. (0, 0) until it moves |
| `coop.mouseDown(slot)` | shared | `true` while that player's left button is down |
| `coop.clicked(slot)` | shared | `true` on the frame that player pressed their left button, whether or not the click reached the game |
| `coop.mouse.clicker` | shared | Inside a mouse handler (`CLICK`, `onRelease`...), the slot whose mouse caused it; `undefined` elsewhere. Since 1.0.4 |
| `coop.mouse.hovered(slot)` | shared | The path of what that player's mouse is on (`"_root.menu.sword"`), as of the last frame; `undefined` while on nothing clickable. Since 1.0.4 |
| `coop.typed(slot)` | shared | What that player typed this frame: `"\b"` for Backspace, `"\n"` for Enter. Calling it tells Ruffle Together the game wants every typed character for the next 60 frames |
| `coop.keys(text)` | shared | Sets the game's own key map, in the [`.keys` format](../make/key-maps.md). Used from the next frame on every PC; held keys are let go first; the same map again changes nothing. `true` if it parsed |

## Button bits

| Button | Bit | Button | Bit |
|---|---|---|---|
| UP | 1 | KEY3 | 256 |
| DOWN | 2 | KEY4 | 512 |
| LEFT | 4 | VIEW | 1024 |
| RIGHT | 8 | QUIT | 2048 |
| FIRE | 16 | YES | 4096 |
| ALT | 32 | NO | 8192 |
| KEY1 | 64 | MENU | 16384 |
| KEY2 | 128 | | |

```actionscript
var held:int = Coop.input.buttons(1);
if (held & 16) fire(1);          // player 2 holds FIRE
if (Coop.input.held(1, "LEFT")) walkLeft(1);
```

`coop.buttons` is the easy way to add players to a single-player game: read each slot's buttons and move each
character, instead of faking keyboard keys.

In the library these are `Coop.input.buttons`, `held`, `typed`, `setKeys` and `Coop.mouse.x`, `y`, `down`,
`clicked`, `clicker`, `hovered`.

## The game's own mouse

Outside mouse handlers, `_xmouse`, `_ymouse`, `stage.mouseX` and `stage.mouseY` follow the **host's** mouse on every
PC. A game where every player aims with the mouse reads each slot through the API, slot 0 included.

Since 1.0.4 every player has their own mouse in the game where the rules allow it: inside a handler,
`stage.mouseX` / `_xmouse` is the mouse that caused it, and `coop.mouse.clicker` says whose it was. See
[The mouse](mouse.md).

## Typing

What players type goes into the game's selected text box on every PC, in player order. While no text box is
selected, typed text only reaches `coop.typed`. See [What every game gets](automatic.md).

## Switching key maps

A game can switch maps per screen, for example FIRE = Enter in menus and FIRE = Space in a match:

```actionscript
Coop.input.setKeys(inMenu ? MENU_KEYS : MATCH_KEYS);
```
