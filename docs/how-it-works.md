# How it works

Ruffle Together doesn't send the game over the network. Every player runs the **whole game** on their own PC, and
only the **inputs** travel: key presses, the mouse and typing. If every PC starts the same game and runs every frame
with the same inputs, every PC ends up with the same game. This is called **lockstep**, and it is how many
fighting and strategy games do online play.

```text
 Liam's PC                         Sam's PC
 ┌──────────────────────┐          ┌──────────────────────┐
 │ frame 100            │          │ frame 100            │
 │  Liam: RIGHT, FIRE   │ ◄──────► │  Liam: RIGHT, FIRE   │
 │  Sam:  UP            │  inputs  │  Sam:  UP            │
 │ → the same game      │   only   │ → the same game      │
 └──────────────────────┘          └──────────────────────┘
```

This is why games work without being changed: the game never knows it is online. It just sees a few more
players pressing keys on one keyboard, the way it was made to be played.

## A frame runs only with everyone's input

A frame runs on a PC only once every player's input for that frame has arrived. To hide the network's lag, each
input is scheduled a few frames ahead: what you press now happens a moment later on every PC at the same frame,
yours included. That small wait is the **input delay**.

- The host measures the ping when friends connect and picks the smallest delay that runs smoothly: about one
  frame per 67 ms of ping, and 1 frame on a LAN.
- Everyone gets the same delay, so nobody has an advantage.
- With 3 or more players, friends' inputs go through the host, so the two worst pings count together.
- If an input is late, the game waits for it instead of guessing. That is why the slowest connection sets the
  pace for everyone.

## Nothing else may differ

Lockstep works only if the game does exactly the same thing on every PC. Flash games read plenty that differs
between PCs, so Ruffle Together hides all of it from the game:

| The game asks for | Normally | In Ruffle Together |
|---|---|---|
| The time (`getTimer`, `Date`, timers) | The PC's clock | A **frame clock**: the same time on every PC for the same frame |
| A random number (`Math.random`) | Different on every PC | The same sequence on every PC |
| When a sound ends | The PC's audio device | The frame clock, so a PC without sound agrees too |
| Script time limits | The PC's speed | The frame clock |
| The window size, `_xmouse` | Each window's own | The stage's own size, the same everywhere |
| The mouse and the keys | This PC's | Every player's, from the inputs, on the same frame |

Network loads (from the web) don't happen during a session.

## Checking that the games agree

Every 25 frames the PCs compare a **fingerprint** of their games: every object's position, visibility, depth and
frame. If they ever differ, every window says OUT OF SYNC and each PC writes a report of exactly what differs, so the
cause can be found. A game that does something Ruffle Together doesn't yet hide shows up here instead of quietly
going wrong.

## Things only your PC sees

Some features change only the picture your PC draws, never the game, so they can differ between PCs safely:

- each player's own **camera** (a game can follow each player separately), and a **HUD** that stays on screen;
- **name tags** over the players, friends' **cursors**, and the chat;
- **private screens**: a pop-up that only the player who opened it sees and clicks. It is still open in everyone's
  game; the other PCs just don't draw it.

The game never sees any of this. The fingerprint check doesn't either.

## Saves

When the game saves, every PC saves the same thing to its own **co-op save**, so the save stays the same on every
PC and anyone can host next time. Everyone starts from the host's co-op save, so every PC begins in the same state.

## The relay

Friends usually can't connect to each other directly. The central relay forwards the inputs between the players
in a room, so nobody needs a public IP or port forwarding. A room code is how friends find the host's room. The
relay only passes inputs along; it never runs the game.

A heartbeat runs alongside the inputs, so every PC can tell a frozen game from a crash, a closed window or a
dropped connection, and says which in its log.
