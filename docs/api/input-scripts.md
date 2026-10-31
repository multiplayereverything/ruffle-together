# Input scripts

Input scripts drive players without a keyboard or mouse, for [replays and labs](../make/testing.md). One command a
line:

| Line | Meaning |
|---|---|
| `p0 UP\|FIRE` | Player 0 holds these buttons from now on (any button names joined with `\|`, or `0` for none) |
| `wait 25` | 25 frames pass with what is held now |
| `move 100 50` | The host's mouse moves to stage pixel (100, 50) |
| `move 100 50 2` | Player 2's mouse moves there |
| `click 275 300 1` | Player 1's mouse: one frame pressed at (275, 300) |
| `type 1 Sam{Enter}` | Player 1 types "Sam", then Enter. 8 characters a frame; `{Backspace}` too |
| `press 100 50 2` | Player 2's mouse moves there and holds its button. No frame passes, so several players can press in the same frame |
| `release 2` | Player 2 lets go. No frame passes |

Slots are 0 to 7; the host is 0. Leaving out the slot of a mouse line means the host.

```text
# both players walk right, player 2 jumps, then player 2 clicks Start
p0 RIGHT
p1 RIGHT
wait 30
p1 RIGHT|UP
wait 10
p0 0
p1 0
click 400 300 1
wait 60
```
