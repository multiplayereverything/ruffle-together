# Testing without anyone playing

Ruffle Together can run a game with scripted players, headless, so you can prove co-op works before asking
friends to play.

## 1. Replays

Run the game headless from an [input script](../api/input-scripts.md) and write a fingerprint of every frame:

```console
ruffle_desktop --coop-replay Game.swf --coop-keys my-game.keys --coop-script two-players.txt --coop-frames 1500 --coop-out run1.txt
ruffle_desktop --coop-replay Game.swf --coop-keys my-game.keys --coop-script two-players.txt --coop-frames 1500 --coop-out run2.txt
```

**The two files must be identical.** If they aren't, the game does something that changes from run to run, and it
will go out of sync online.

An input script looks like this:

```text
wait 60
p0 RIGHT
p1 LEFT|FIRE
wait 30
p0 0
click 400 300 1
wait 100
```

## 2. A whole session, headless

Two players through the relay, no windows:

```console
ruffle_desktop --coop-headless --coop-frames 1500 --coop-script two-players.txt --coop-players 2 --coop-host-relay 165.232.82.11:25569 Game.swf
ruffle_desktop --coop-headless --coop-frames 1500 --coop-script two-players.txt --coop-join-relay 165.232.82.11:25569 --coop-code ABCDE Game.swf
```

Each prints a `COOP RESULT` line. Both must say `desync_at=none`.

## 3. The labs

From a checkout of the [`coop` branch](https://github.com/multiplayereverything/ruffle-together/tree/coop):

| Tool | What it does |
|---|---|
| `tools/coop-lab/lab.py --swf Game.swf --players 4 --keys my-game.keys` | 2 to 8 headless players on one PC, through a local relay with fake lag, and directly |
| `tools/coop-lab/windows_lab.py --swf Game.swf` | Two real windows; checks that every frame reaches the screen |

To run several windows on one PC yourself, give each its own `--cache-directory`.

## 4. The API demo

[`tools/coop-api/demo/`](https://github.com/multiplayereverything/ruffle-together/tree/coop/tools/coop-api/demo)
is a small AS3 game that uses every co-op call and shows the answers on screen. It comes with a key map and input
scripts for 2 and 4 players, for replays, headless sessions and real windows.

## 5. Real people

Last, play it with a friend on another PC. The labs catch most problems, but only real networks and real hands
catch the rest.
