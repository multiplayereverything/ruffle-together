# Players

| Call | Kind | Answer |
|---|---|---|
| `coop.players` | shared | The number of players, 2 to 8 |
| `coop.name(slot)` | shared | That player's name (what they joined with) |
| `coop.localSlot` | **local** | This PC's slot |
| `coop.isHost` | **local** | `true` on the host's PC |
| `coop.ping(slot)` | **local** | This PC's round trip to that player in ms: 0 to itself, `undefined` until measured (about 2 s). Friend to friend goes through the host, so it's both trips added |
| `coop.version` | shared | The API version: 2 |
| `coop.has(feature)` | shared | `true` if this build has the feature |

## Features for `coop.has`

`players`, `mouse`, `name`, `camera`, `hud`, `label`, `keys`, `buttons`, `typed`, `options`, `ping`, `events`,
`chat`, `clicks`, `zones`, `private`, `hover`

Since 1.0.4: `pointers` (one mouse per player), `clicker`, `hovered`, `cursors`.

Check a feature before using it, so the game still runs on an older Ruffle Together:

```actionscript
if (Coop.room.has("clicker")) {
    ready[Coop.mouse.clicker()] = true;   // whoever clicked
} else {
    ready[0] = true;                      // before 1.0.4: only the host clicks
}
```

In the library, `coop.players` is `Coop.players.count()`, `coop.localSlot` is `Coop.players.local()`, and
`coop.version` and `coop.has` are in `Coop.room`.

## Using local answers safely

`localSlot` and `isHost` are different on every PC. Use them only for what this PC shows:

```actionscript
// fine: text in a field that exists on every PC
youText.text = (slot == Coop.players.local()) ? "YOU" : "";

// wrong: the hero is moved on one PC only, and the games split
if (slot == Coop.players.local()) hero.x += 5;
```
