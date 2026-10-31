# The mouse

:::{admonition} Since 1.0.4
:class: tip

This page describes **one mouse per player**, new in 1.0.4. What 1.0.3.9 did instead
is in the fold-outs marked "In 1.0.3.9".
:::

## Every player's mouse

Every player has a mouse of their own in the game. Ruffle runs hover, press, release and drag for each one, with
the game's own events (`ROLL_OVER`, `CLICK`, `onRelease`...), so the game's buttons work for every player
without any change to the game.

- Inside a mouse handler, `stage.mouseX` / `_xmouse` is the mouse that caused it.
- A `startDrag` follows the mouse that started it.
- Several players can press and hold at once.
- An object rolls over for the first mouse on it and rolls out for the last one.

By default only the **host's** mouse is in the game. Every PC still knows every player's mouse through
[`coop.mouseX`](input.md), but a friend's mouse is in the game only where the rules below let it be. Anywhere
else it is outside the stage, so it neither hovers nor clicks.

:::{dropdown} In 1.0.3.9
Flash had one mouse. A friend's click moved the game's mouse to their point for that click and back to the host's
after, so hover effects could flicker. Clicks took turns: the host's went at once, and friends' clicks that landed
together waited in a queue and followed one a frame. There was no `coop.mouse.clicker`, `coop.mouse.hovered`,
`coop.mouse(slot, mode)` or `coop.mouse.cursors`.
:::

## Who may use their mouse

| Call | Kind | Effect |
|---|---|---|
| `coop.mouse(mode)` | shared | Where every friend's mouse is in the game (below). Resets `coop.mouse(slot, mode)` |
| `coop.mouse(slot, mode)` | shared | The same for one friend (slot 1 to 7), over what `coop.mouse(mode)` says: `coop.mouse(2, "host")` keeps player 3 out |
| `coop.zone(slot, path)` | shared | The buttons inside the clip at `path` (`"_root.players.p2"`) belong to that player, such as their own card on a character screen. Nobody else's mouse may click them, the host's included. Only while that clip is on show, so other screens are untouched. When zones nest, the most exact clip wins |
| `coop.zone(slot, x, y, w, h)` | shared | The same for a box in stage pixels. A box applies on every screen, so prefer a clip |
| `coop.zone(slot)` | shared | Clears that player's zones |

### Modes

`"host"` (the default)
: Friends' mice aren't in the game.

`"zones"`
: A friend's mouse is in the game only on something clickable inside their own zone, like their own card on a
  character screen. The host's is everywhere else.

`"buttons"`
: On anything the game made clickable: what shows the hand cursor (buttons, button-mode clips, links).

`"all"`
: Anywhere.

## Who clicked

| Call | Kind | Answer |
|---|---|---|
| `coop.mouse.clicker` | shared | Inside a mouse handler (`CLICK`, `MOUSE_DOWN`, `onPress`, `onRelease`...), the slot whose mouse caused it. `undefined` elsewhere |
| `coop.mouse.hovered(slot)` | shared | The path of what that player's mouse is on (`"_root.menu.sword"`), as of the last frame. `undefined` while it is on nothing clickable or out of the game |
| `coop.clicked(slot)` | shared | `true` on the frame that player pressed their left button, whether or not the click reached the game |

One "Ready" button for everyone:

```actionscript
readyButton.addEventListener(MouseEvent.CLICK, function (e:MouseEvent):void {
    ready[Coop.mouse.clicker()] = true;
});
```

A shop that shows what each player points at:

```actionscript
for (var slot:int = 0; slot < Coop.players.count(); slot++) {
    tooltips[slot].text = describe(Coop.mouse.hovered(slot));
}
```

## How buttons look

| Call | Kind | Effect |
|---|---|---|
| `coop.hover(mode)` | picture only | How each screen draws Button symbols while friends have mice |
| `coop.mouse.cursors(mode)` | picture only | When friends' cursors show on each screen |

`coop.hover` modes:

`"own"` (the default)
: Each screen draws buttons as **its own player's** mouse sees them, in the game's own Up, Over and Down art (AS3
  `SimpleButton` and AS2 buttons). Your buttons light up under your mouse, not under a friend's.

`"shared"`
: As the game has them: every player's hover on every screen. (`"host"` means the same.)

Hover made by the game's **code** (a clip that moves on `ROLL_OVER`) is game state, so it shows on every screen
either way.

`coop.mouse.cursors` modes:

`"auto"` (the default)
: A friend's cursor shows only while it is over something that player may click.

`"on"`
: Always.

`"off"`
: Never.

A cursor that hasn't moved for 3 seconds fades out.

:::{dropdown} In 1.0.3.9
`coop.hover` took `"host"` (the default: the game's own hover followed the host's mouse), `"own"` (the same, plus
each PC drew Button symbols by its own player's mouse) and `"off"` (no hover at all). `"off"` is gone since
1.0.4.
:::

## Private screens

A pop-up that only one player sees and uses: a player editor, a shop, an inventory. Every PC runs the same game, so
the pop-up is open in everyone's game. What changes is the picture and the mouse, the same way on every PC, so it
can't desync.

| Call | Kind | Effect |
|---|---|---|
| `coop.only(path)` | shared | The clip at `path` (`"_root.pcb"`) belongs to whoever's click opened it, while it is on show: only their PC draws it, only they click it, and everyone else's mouse goes through it to what is underneath |
| `coop.only(path, pattern)` | shared | The same, and while it is someone's, nobody else may click the buttons that open it (`"_root.players.*.instance*"`: `*` is any one name, `instance*` any name starting so). A refused player sees "Bob has that open: wait until they close it" |
| `coop.only(slot, path)` | shared | The same, always that player's |
| `coop.shared(path)` | shared | Everyone sees and clicks it again |

The player who opened the pop-up clicks it even outside their zone.

:::{note}
The pop-up is still open in everyone's game. A game that freezes the screen behind its pop-ups freezes it for
everyone, and `BitmapData.draw` of the stage still sees the pop-up on every PC.
:::

## Finding a button's path

Turn friends' mice on (`mouse: buttons`) and click the button once. The log
(`%LOCALAPPDATA%\ruffle\log\ruffle.log`) says, for example, `player 2 clicks _root.players.p2.b10 (463, 272)`.

The mouse debug overlay, `--coop-mouse-debug` (since 1.0.4), shows every player's mouse, what it is on, and
whether the rules let it be there.

## Example: a character screen

Chaos Faction 2's built-in key map gives player 2 their own card (except its Keyboard 2 drop-down, which stays the
host's), and the Player Editor to whoever opens it:

```text
mouse: zones
zone 1: _root.players.p2
zone 0: _root.players.p2.combo
private: _root.pcb opened-by _root.players.*.instance*
hover: own
```

The same from the game's code:

```actionscript
Coop.mouse.setMode("zones");
Coop.zones.give(1, "_root.players.p2");
Coop.zones.give(0, "_root.players.p2.combo");
Coop.screens.onlyOpenedBy("_root.pcb", "_root.players.*.instance*");
Coop.mouse.setHover("own");
```
