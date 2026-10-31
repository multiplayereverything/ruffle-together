# The Coop library

The `Coop` library wraps every call, grouped by what it does, with the same names in AS2 and AS3:

AS3
: Copy every `Coop*.as` file from
  [`tools/coop-api/as3/`](https://github.com/multiplayereverything/ruffle-together/tree/coop/tools/coop-api/as3) into
  the game's source folder.

AS2 (any SWF version)
: `#include "Coop.as"` from
  [`tools/coop-api/as2/`](https://github.com/multiplayereverything/ruffle-together/tree/coop/tools/coop-api/as2). It
  unhides `ExternalInterface` on SWF 6 and 7 for you.

**Outside Ruffle Together every method is safe:** `Coop.active()` is false, there is 1 player, slot 0, and calls
that change something do nothing. Mouse calls read the game's own mouse in AS2 and return `NaN` in AS3 (use the
game's own mouse). One SWF works everywhere.

## Groups

| Group | Methods | Calls |
|---|---|---|
| `Coop.players` | `count()`, `name(slot)`, `local()`, `isHost()`, `ping(slot)` | [Players](players.md) |
| `Coop.input` | `buttons(slot)`, `held(slot, "FIRE")`, `typed(slot)`, `setKeys(text)` | [Input](input.md) |
| `Coop.mouse` | `x(slot)`, `y(slot)`, `down(slot)`, `clicked(slot)`, `hovered(slot)`, `clicker()`, `setMode(mode)`, `setModeFor(slot, mode)`, `setHover(mode)`, `setCursors(mode)` | [Input](input.md), [The mouse](mouse.md) |
| `Coop.zones` | `give(slot, path)`, `giveBox(slot, x, y, w, h)`, `clear(slot)` | [Zones](mouse.md#who-may-use-their-mouse) |
| `Coop.screens` | `only(path)`, `onlyFor(slot, path)`, `onlyOpenedBy(path, pattern)`, `share(path)` | [Private screens](mouse.md#private-screens) |
| `Coop.camera` | `setPosition(slot, x, y)`, `setZoom(slot, zoom)`, `setView(slot, x, y, zoom)`, `off(slot)` | [Camera](picture.md) |
| `Coop.view` | `pinHud(path)`, `label(slot, path, text)` | [HUD, labels](picture.md) |
| `Coop.room` | `version()`, `has(feature)`, `option(name)`, `options()`, `events()` | [Players](players.md), [Host settings](host-settings.md), [Events](events.md) |
| `Coop` | `active()`, `call(name, args...)` | Any raw call by its name |

A few methods answer a little differently from the raw call, to be easier to use:

- `Coop.mouse.hovered(slot)` returns `null` instead of `undefined` when the mouse is on nothing.
- `Coop.mouse.clicker()` returns `-1` outside a mouse handler.
- `Coop.camera` remembers each player's position and zoom, so `setZoom` keeps the position and `setPosition` keeps
  the zoom.

## AS3

```actionscript
// the Coop*.as files are next to the game's own classes
if (Coop.active()) {
    for (var slot:int = 0; slot < Coop.players.count(); slot++) {
        heroes[slot].move(Coop.input.buttons(slot));
        Coop.camera.setPosition(slot, heroes[slot].x, heroes[slot].y);
    }
}
```

## AS2

Put `#include "Coop.as"` at the top of the frame or class that uses it, then:

```actionscript
if (Coop.active()) {
    for (var slot = 0; slot < Coop.players.count(); slot++) {
        _root["hero" + slot].move(Coop.input.buttons(slot));
    }
}
```

:::{admonition} Since 1.0.4
:class: tip

The `Coop` library comes with 1.0.4. Its calls for features 1.0.3.9 already had work there too; `clicker`,
`hovered`, `setModeFor` and `setCursors` need 1.0.4.
:::

:::{dropdown} The older `RuffleTogether.as`
`RuffleTogether.as` is the first, flat version of the library. It still works, so mods that use it don't need to
change.

| Method | Same as |
|---|---|
| `active()` | `Coop.active()` |
| `players()`, `localSlot()`, `isHost()`, `name(slot)`, `ping(slot)` | `Coop.players.*` |
| `version()`, `has(feature)`, `option(name)`, `options()`, `events()` | `Coop.room.*` |
| `buttons(slot)`, `button(slot, name)`, `typed(slot)`, `keys(text)` | `Coop.input.*` |
| `mouseX(slot)`, `mouseY(slot)`, `mouseDown(slot)`, `clicked(slot)`, `mouse(mode)`, `hover(mode)` | `Coop.mouse.*` |
| `zone(slot, x, y, w, h)`, `zoneClip(slot, path)`, `zoneOff(slot)` | `Coop.zones.*` |
| `only(path)`, `onlyFor(slot, path)`, `onlyOpenedBy(path, pattern)`, `shared(path)` | `Coop.screens.*` |
| `camera(slot, x, y, zoom)`, `cameraOff(slot)` | `Coop.camera.*` |
| `hud(path)`, `label(slot, path, text)` | `Coop.view.*` |
:::
