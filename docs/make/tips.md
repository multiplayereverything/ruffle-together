# Tips for game makers

## Staying in sync

- **Gameplay reads only shared answers.** `localSlot`, `isHost` and `ping` differ between PCs. A "YOU" text over
  your hero is fine; moving the world to follow your hero isn't (use a camera).
- **Don't show, hide, move or create display objects from local answers.** The sync check compares every object's
  position, visibility, depth and frame.
- **Call `coop.camera` for every slot**, not just your own. Every PC then holds the same cameras and draws with its
  own.
- **Read `coop.events` once a frame.** It returns what happened since the last call, so reading it twice in a frame
  gives the second reader nothing.
- **Avoid looping over a `Dictionary` keyed by objects.** Its order can differ between PCs. The sync check warns if
  it does.
- **The clock, `Math.random` and timers are already safe.** Ruffle Together runs them by the frame clock, so you don't
  need to change them.

## Input

- **Use `coop.buttons(slot)` to add players to a single-player game.** It's much simpler than faking keyboard keys
  for each hero.
- **To aim with every player's mouse, read `coop.mouseX/mouseY(slot)`**, slot 0 included. Outside mouse handlers,
  `_xmouse` and `stage.mouseX` are the host's.
- **Let menus work for everyone without code changes** (since 1.0.4): `mouse: buttons` gives friends their own
  mouse on the game's buttons, and `coop.mouse.clicker` says who clicked a shared button such as "Ready".
- **Give each player their own buttons with zones**, such as their card on a character screen, so nobody can
  change someone else's pick.
- **`on(keyPress "a")` buttons work.** Each player's key press types its game key's character, from any key map,
  so menu shortcuts work in co-op, replays and scripts.
- **Find a button's path from the log.** Turn on `mouse: buttons`, click the button once, and the log names it.
  Since 1.0.4, `--coop-mouse-debug` shows every player's mouse and what it is on.

## Picture

- **A HUD clip must be a direct child of the root** (`_root.hud`). Marked clips stay on screen with every camera,
  even when the game scrolls by moving `_root`.
- **Labels follow each PC's camera.** Ruffle draws them; the game never sees them.

## Old SWFs

- **SWF 6 and 7 hide `ExternalInterface`.** The AS2 library unhides it for you. Without the library:

  ```actionscript
  ASSetPropFlags(_global, "flash", 0, 4096);
  ASSetPropFlags(flash.external.ExternalInterface, null, 0, 4096);
  ```

## Testing

- **Replay twice and compare.** If the same script gives two different files, something isn't deterministic yet.
- **Test with 1 player against the original** before adding players: it proves the co-op build didn't change the
  game.
- **Test a friend leaving.** The game must play on with the players who are left.
