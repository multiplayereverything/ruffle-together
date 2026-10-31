# The co-op API

A Flash game, AS2 or AS3, can ask Ruffle Together about the players and change how co-op works for it. This is
version **2** of the API.

## How to call it

Use the [`Coop` library](library.md). It has the same names in AS2 and AS3, grouped by what they do:

```actionscript
var n:int = Coop.players.count();
Coop.camera.setPosition(1, hero2.x, hero2.y);
if (Coop.mouse.clicker() == 1) { ... }   // inside a button's CLICK / onRelease
```

Outside Ruffle Together it answers like a single-player game (1 player, slot 0, no camera), so one SWF works
everywhere.

Underneath, every call is `ExternalInterface.call(name, args...)`, the same in AS2 and AS3. The reference pages list
each call under that raw name:

```actionscript
var players = ExternalInterface.call("coop.players");
var p2Fire = ExternalInterface.call("coop.button", 1, "FIRE");
```

- **In a session or not:** `ExternalInterface.available && ExternalInterface.call("coop.players") > 0`. In plain
  Ruffle or Flash Player this is false.
- **SWF 6 and 7 (AS1/AS2):** `ExternalInterface` is hidden. Unhide it first (the AS2 library does this):
  `ASSetPropFlags(_global, "flash", 0, 4096); ASSetPropFlags(flash.external.ExternalInterface, null, 0, 4096);`
- **Slots** are players 0 to `players - 1`. Slot 0 is the host. A slot outside that range answers `undefined`.
- In a session there is no JavaScript: any other `ExternalInterface.call` returns `undefined`.

## The one rule: shared or local

Every PC runs the same game. Each answer is one of these kinds:

| Kind | Meaning | Use it for |
|---|---|---|
| **shared** | The same answer on every PC on the same frame | Anything, gameplay included |
| **local** | Differs between PCs | Only what this PC shows: text, colours, sounds. Never gameplay, and never moving, showing or creating display objects |
| **picture only** | Changes only what each PC draws | Cameras, HUD, labels: the game never sees them |

The sync check compares every object's position, visibility, depth and frame. A local value that reaches gameplay
makes the PCs' games split, and the window title says OUT OF SYNC.

## Every call

| Call | Kind | Library | Page |
|---|---|---|---|
| `coop.players`, `coop.name`, `coop.localSlot`, `coop.isHost`, `coop.ping` | shared, except `localSlot`, `isHost`, `ping` (**local**) | `Coop.players` | [Players](players.md) |
| `coop.buttons`, `coop.button`, `coop.typed`, `coop.keys` | shared | `Coop.input` | [Input](input.md) |
| `coop.mouseX`, `coop.mouseY`, `coop.mouseDown`, `coop.clicked`, `coop.mouse.clicker`, `coop.mouse.hovered` | shared | `Coop.mouse` | [Input](input.md), [The mouse](mouse.md) |
| `coop.mouse`, `coop.hover`, `coop.mouse.cursors` | shared / picture only | `Coop.mouse` | [The mouse](mouse.md) |
| `coop.zone` | shared | `Coop.zones` | [The mouse](mouse.md) |
| `coop.only`, `coop.shared` | shared | `Coop.screens` | [Private screens](mouse.md#private-screens) |
| `coop.camera` | picture only | `Coop.camera` | [Camera, HUD, labels](picture.md) |
| `coop.hud`, `coop.label` | picture only | `Coop.view` | [Camera, HUD, labels](picture.md) |
| `coop.version`, `coop.has`, `coop.option`, `coop.options`, `coop.events` | shared | `Coop.room` | [Players](players.md), [Host settings](host-settings.md), [Events](events.md) |

`coop.mouse.clicker`, `coop.mouse.hovered`, `coop.mouse(slot, mode)` and `coop.mouse.cursors` are new in
1.0.4.

Many features need no call at all: see [What every game gets](automatic.md).
