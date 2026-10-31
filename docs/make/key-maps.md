# Key maps

A key map tells Ruffle Together which keys in the game belong to which player. Each player presses their own
normal keys, and the key map turns them into that player's keys in the game. Pass it with
`--coop-keys my-game.keys`; every player must use the same one, and a different one is refused by name.

## An example

```text
# Two players: arrows and Z/X for player 1, WASD and T/Y for player 2
slot 0: UP=38 DOWN=40 LEFT=37 RIGHT=39 FIRE=90 ALT=88
slot 1: UP=87 DOWN=83 LEFT=65 RIGHT=68 FIRE=84 ALT=89
local: UP=38,87 DOWN=40,83 LEFT=37,65 RIGHT=39,68 FIRE=90,84 ALT=88,89
```

`slot N:`
: What player N's buttons press **in the game**, as Flash key codes. Slots go 0 to 7, in order, and the number of
  slots is how many players the game can have. Slot 0 is the host.

`local:`
: The keys a player presses **on their own PC** for each button. Several keys per button, separated by commas.
  Here both arrows and WASD work for everyone, whichever player they are.

`shared`
: Every slot presses the same keys. For hot-seat games where players take turns on one set of keys
  (Territory War).

`#`
: A comment.

## Buttons

`UP DOWN LEFT RIGHT FIRE ALT KEY1 KEY2 KEY3 KEY4 VIEW QUIT YES NO MENU`

The names are only labels for a button; what matters is the key codes on each side. The same names are used by
[`coop.buttons`](../api/input.md) and in [input scripts](../api/input-scripts.md).

## Key codes

| Keys | Codes |
|---|---|
| Letters A to Z | 65 to 90 |
| Digits 0 to 9 | 48 to 57 |
| Numpad 0 to 9 | 96 to 105 |
| Numpad `* + - . /` | 106, 107, 109, 110, 111 |
| Arrows left, up, right, down | 37, 38, 39, 40 |
| Space, Enter, Esc, Backspace, Tab | 32, 13, 27, 8, 9 |
| Shift, Ctrl, Alt | 16, 17, 18 |
| Page Up, Page Down, End, Home | 33, 34, 35, 36 |
| Insert, Delete | 45, 46 |
| `` ; = , - . / ` `` | 186 to 192 |
| `[ \ ] '` | 219 to 222 |

Numpad keys are sent with the numpad location, so games that check for the numpad see it.

:::{tip}
Only the keys in the key map reach the game. If a game needs P for pause or M for mute, give it a button. `/` opens
the chat unless the key map gives it to the game.
:::

## The mouse

By default only the host's mouse is in the game. A key map can let friends' mice in without touching the SWF:

```text
mouse: zones
mouse 2: host
zone 1: _root.players.p2
private: _root.pcb opened-by _root.players.*.instance*
hover: own
cursors: auto
```

| Line | What it does |
|---|---|
| `mouse: host` | Only the host's mouse is in the game (the default) |
| `mouse: buttons` | Friends' mice too, but only on things the game made clickable (what shows the hand cursor) |
| `mouse: zones` | A friend's mouse only on clickable things inside their own zone; the host's everywhere else |
| `mouse: all` | Everyone's mouse, anywhere |
| `mouse N: <mode>` | The same for one friend (N is 1 to 7), over what `mouse:` says. Since 1.0.4 |
| `zone N: <clip>` | The buttons inside that clip belong to player N, for example their card on a character screen. Only while the clip is on show |
| `private: <clip> opened-by <pattern>` | A pop-up belongs to the player who opened it: only they see and click it. While it's someone's, nobody else may click the buttons that open it |
| `hover: own`, `shared` | How each screen draws the game's buttons: by its own player's mouse (the default) or as the game has them. See [`coop.hover`](../api/mouse.md#how-buttons-look) |
| `cursors: auto`, `on`, `off` | When friends' cursors show. Since 1.0.4 |

:::{dropdown} In 1.0.3.9
`hover:` took `host` (the default), `own` or `off`, and there was no `mouse N:` or `cursors:`. Since 1.0.4
`hover: host` means `shared`, and `hover: off` is gone.
:::

To find a button's path for a zone, start with `mouse: buttons` and click it once. The log
(`%LOCALAPPDATA%\ruffle\log\ruffle.log`) says, for example, `player 2 clicks _root.players.p2.b10 (463, 272)`.

## Built-in key maps

The [games list](../play/games.md) has the maps that come with Ruffle Together. Their files are in
[`desktop/src/coop/keys/`](https://github.com/multiplayereverything/ruffle-together/tree/coop/desktop/src/coop/keys)
and make good examples. A file with the same name and text as a built-in map counts as that map.

## Setting the key map from the game

A game made for Ruffle Together can set its own key map with [`coop.keys(text)`](../api/input.md), and change it per
screen (one map for menus, another for a match). Then nobody needs `--coop-keys`.
