# Limits

- **Up to 8 players** (as many as the game has), and nobody can join a game that has already started.
- **The game runs at the speed of the slowest PC** and the slowest connection.
- **If the host leaves, the game ends.** A friend leaving doesn't end it.
- **Only the keys in the key map reach the game.** P, M and the like don't, unless the map names them. `/` opens the
  chat unless the map gives it to the game.
- **During a session** there is no menu, no fullscreen, and no opening another file. The stage keeps the game's own
  size; the window can still be resized.
- **Sounds end by the frame clock**, not the audio device, so `onSoundComplete` and a sound's `position` are the same
  on every PC. Timeline stream sounds still follow the device.
- **Games that loop over a `Dictionary` keyed by objects may drift.** The sync check warns if they do.
- **Network loads** (`URLLoader`, `Loader` from the web) don't happen in a session.
- **Windows only** for now. Linux and macOS can build from source, but that isn't tested. A browser version is in the
  works.
