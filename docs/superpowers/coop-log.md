# Ruffle Together log

Plan: docs/superpowers/plans/2026-10-28-ruffle-together.md · Spec: docs/superpowers/specs/2026-10-28-ruffle-together-design.md

## 2026-10-28
- Branch `coop` from upstream 07d3e4306. Release build OK (`cargo build -p ruffle_desktop --release`, 4m55s).
- Baseline: `cargo test -p ruffle_frontend_utils --features navigator` 137/138 on upstream: `test_socket_fail`
  fails on this Windows PC before any change (pre-existing).
- Task 1: Nagle off (`connect_tcp` in frontend-utils navigator). `test_socket_nodelay` RED (missing fn) -> GREEN.
- Task 11 (done early): Bad Ice-Cream `build.py --no-net` -> `BadIceCream_coop.swf`, Poki fix only, 0 bicnet,
  SHA-256 4635ddfc255df0a9682d3fab3b7c2942035a498c1e131cdb062436d5d4593494.
- Tasks 2-9 done (frame clock, fingerprint, lockstep, messages, connections incl. the real relay, keys, save).
  SWF regression suite after the frame clock: 4291 passed, 0 failed.
- Task 4 determinism on Bad Ice-Cream (script desktop/tests/coop/bic-two-players.txt: menus -> 2P -> level 1,
  both players moving/firing): same script at 0 ms and 30 ms random delay per frame -> identical fingerprints for
  all 1900 frames. Different start date (Math.random seed) -> differs from frame 784 (first random use).
- Found: a renderer that can't draw changes the game (title screen at frame 308 swaps a clip for a drawn bitmap),
  so replay and sessions both use the real GPU renderer. Risk to watch: games that read drawn pixels back could
  differ between different GPUs; the sync check would catch that.
- Task 10: sessions run in the desktop player (CLI flags, window title shows room/status). Session tests on the
  real game: in sync for 1000 frames; a forced desync is reported on both PCs at the same frame; a friend
  leaving stops the host within 6 s; a different game file is refused; paced at the game's frame rate.
- Task 12 lab (two windowless players, scripted input, 5400 frames = 3 min each, Bad Ice-Cream at 30 fps,
  input delay 3 frames = 100 ms):
  | mode | desync | host felt delay (median / p95) | friend felt delay |
  |---|---|---|---|
  | local relay, 40 ms one-way lag + 0-10 ms jitter | none | 145 / 154 ms | 113 / 117 ms |
  | direct | none | 121 / 125 ms | 114 / 118 ms |
- Not yet: two real PCs, the windowed session by hand, the relay `ruffle-coop` game in mods-info (needs OK).
- Final review (fresh reviewer): fixed the 2 critical and 7 of 9 important findings, each with a test where one
  is possible (friend's cursor outside the window, exact host mouse, full preload, saves in another folder,
  stray connections, refused-friend retry, player settings in the handshake, window/fullscreen/menu/open
  blocked, no network loads, message boxes). Not fixed, documented: sound end/position feeding back into
  games (Bad Ice-Cream doesn't use it), Dictionary object-key order (upstream). All decisions:
  docs/superpowers/coop-decisions.md.
- After the fixes: desktop 86/86 (with COOP_TEST_SWF), core 129/129, SWF suite 4291 passed / 0 failed,
  lab 900 frames both modes in sync (felt delay ~101 ms direct, 101-113 ms through a laggy relay).
- Known limits for v1: Bad Ice-Cream keys only (P pause, M mute and Esc never reach the game), window title
  is the only status display, the windowed session hasn't been run by hand or on two real PCs yet.
- First real windowed test (two ruffle_desktop windows on this PC, scripted keys): frames never started, the
  session's no-loads navigator also blocked the game file itself (the windowless tests load it differently).
  Fixed with SessionNavigator (only the first fetch goes through; test only_the_game_file_loads...). Then:
  direct, 75 s: both windows identical fingerprints at frames 24, 774, 1524, no warnings. Live relay
  195.60.166.57:25569 with room code Z9ELK, 60 s: identical at 24, 774, 1524.
- mods-info: ruffle-coop pushed (025f6f5, user OK); the live relay accepted it 9 minutes later.
- Release v1.0.0 published (user asked): tag v1.0.0 on 0b9247d, asset RuffleTogether-1.0.0-windows-x64.zip (ruffle_desktop.exe, README.md, LICENSE.md), SHA-256 F1886A7E...6590, re-downloaded and hash checked. Build id rc-1.0.0#0b9247df45. Not a prerelease. mods-info ruffle-coop later updated to 1.0.0 (checked 2026-10-28).
- User's first two-PC test (LAN): in sync, but "freezing a lot, choppy, slow motion"; solo play smooth on the same PCs.
  Added a timing line to the log every 5 s (fps, event-loop gaps, frame cost, time waiting for the other PC's
  input and the longest wait). Two windows on this PC, direct: 30.0 fps, frame cost avg 0.3-6.7 ms, 0 ms waiting.
  Through a local relay with 30 ms + 0-120 ms jitter: 30 fps average but the host waited for input 100% of the
  time (longest 150 ms), the friend ~15% (longest 75 ms). Finding: the host needs the ROUND TRIP under the input
  delay (100 ms) because the friend starts one trip later; the friend only needs the one-way trip. Over that, the
  picture follows packet arrival (freeze, then several frames at once). Nagle is off on every socket (checked).
  Not yet known: what the user's LAN does (Wi-Fi?). Test build with the timing line: test-build/ (not committed),
  sha256 cac226fd...cbb9; asked the user for both PCs' ruffle.log and a ping.
- Release v1.0.1 (user chose "Release v1.0.1" to get the timing build into the launcher): commit d125698 (version
  1.0.1, timing line, docs), desktop tests 88/88 with COOP_TEST_SWF, lab direct 450 frames in sync. Tag v1.0.1, asset
  RuffleTogether-1.0.1-windows-x64.zip SHA-256 4285b186...3c7d (exe 6b280c71...9b75), marked latest; releases/latest
  returns it, re-downloaded and hash checked. Live relay 195.60.166.57:25569 with the downloaded exe, room V49FG, 300
  frames: in sync, felt delay host 134 / friend 100 ms (the round-trip asymmetry again). mods-info 1a33560:
  ruffle-coop latest/min 1.0.1, protocol rc-1.0.1 (pushed from a fresh clone; the global-mods-server/mods-info clone
  has other sessions' uncommitted work and was left alone). Asked the Multiplayer Mods Launcher session to update
  installs that already have 1.0.0 (its installer only fetches when ruffle_desktop.exe is missing).
- Launcher 1.0.33 (launcher session, verified as latest release): updates an installed Ruffle Together to the newest
  ruffle-coop release on open (SHA-256 checked, records players\ruffle-together\.version; a 1.0.0 copy without it
  updates once). Its test took 1.0.0 to v1.0.1 from the real release. 1.0.32 had fixed joining a Bad Ice-Cream room
  (room code was passed as a -P FlashVar -> host refused the friend for different player settings).
- First real two-PC log (1.0.1, host = this PC, RTX 3080, over Radmin VPN 26.x; friend's ping to host 9-11 ms):
  host steady 30.0 fps, its own frame cost avg 0.4-12 ms (one 154 ms hitch), event-loop gaps <= 34 ms, but it
  waited for the friend's input 100% of the time (longest 37-52 ms, 393 ms at start). The host runs each frame
  as soon as the friend's input arrives, so its picture follows the friend's PC and the network. Friend's log not
  yet received.
- ROOT CAUSE of the two-PC lag (both logs: game logic a steady 30.0 fps on both PCs, in sync, frame cost 1-12 ms,
  friend never waited, host waits <= ~50 ms): the picture, not the game. Stepper draws each frame (player.render),
  which clears needs_render, so about_to_wait's render_movie_if_needed never asked the window to repaint; the screen
  only updated when a key, mouse move or egui forced a repaint. Measured with a new "shown per second" count in the
  timing line: two windows 30.0 game fps, 0.0 shown per second. Fix (app.rs): request_redraw whenever the session
  ran a frame. After: 30.0 shown per second in both windows for 55 s, same fingerprints at 24/774/1524;
  desktop tests 88/88. Not fixed (minor at 10 ms ping): the host always waits on the friend's input because the
  friend starts later; the host's picture follows packet arrival.
- User played two local windows on 1.0.2-pre (redraw fix): "way better" but input delay and some stutter. Their
  log: perfect while idle; late frames 9-16 per 5 s exactly when pictures shown rose to 40-56/s (their keys and mouse).
  Cause: egui asks for a repaint on every input event; each present waits for vsync and starves the frame loop.
  Fix: PictureGate (session.rs) shows one picture per new game frame (or after 100 ms stale); render_movie_if_needed
  no longer asks for egui repaints while a session runs (else a busy loop: 48k loop passes per 5 s).
  New windowed lab tools/coop-lab/windows_lab.py: two real windows, scripted keys, modes quiet and pressure
  (WM_MOUSEMOVE posted to the two Ruffle windows only, 125/s); limits fps >= 29.5, shown 28-32.5/s, late <= 3
  per run, <= 5000 loop passes per 5 s, equal fingerprints. Proven to fail: current build before the loop fix
  (busy loop 48k), gate switched off (pressure: 60 shown/s, friend 89 late frames, loop passes ~300). After both
  fixes: quiet PASS x3 of 4 (one run had a single ~500 ms stall in both windows at once, unexplained), pressure
  PASS. Input delay in the lab ~101 ms (3 frames). lab.py relay at 5 ms + 5 ms jitter (Radmin-like), 900 frames:
  delay 3 host 122 / friend 101 ms median; delay 2 67 / 67; delay 1 34 / 59 (friend waits). Desktop tests 89/89.
- Input delay (user chose: host measures the ping and picks the delay). Found first: the PC that runs ahead read
  its keys when a frame was due, then waited for the other PC, adding the wait to its delay (direct, ~0 ping:
  delay 1 host 67 / friend 34, delay 2 host 94 / friend 67). Test both_players_feel_the_input_delay_setting RED
  (host 100 ms at 2 frames, friend started 200 ms late) -> keys are read as the frame runs -> GREEN; lab direct
  and Radmin-like: delay 1 = 34/34, delay 2 = 67/67. Delay 0 is no longer allowed (host clamps to 1).
  Ping/Pong messages: the host pings the friend 5 times after accepting it; delay = ceil((worst ping + 15 ms) /
  2 frames), 1..10, fallback 3 if no answer; --coop-delay N still fixes it. Rule measured with lab.py relay lag:
  delay 1 clean to ~50 ms ping, stalls at 60-70 (20.6-23.6 s for 20 s); delay 2 clean at 100-110, edge 120-130,
  stalls 140-150 (22.4 s); delay 3 clean at 160-170. (A first rule, (d+1) frames, fit only the delay-1 data and
  picked 5 frames on the live relay; corrected.) Auto picks: direct 1, 5 ms lag 1, 30 ms 2, 50 ms 2-3. Live relay
  195.60.166.57:25569 (room ET5HD): ping 168 ms -> 3 frames, felt 100/100 ms, 450 frames in 15.1 s, in sync
  (1.0.1: host 134 / friend 100). windows_lab both modes PASS, input delay 35 ms. Desktop tests 92/92.
- Release v1.0.2 (user asked for the best build to test with friends; earlier OK to publish 1.0.2): commit
  bf48ad2, tag v1.0.2, RuffleTogether-1.0.2-windows-x64.zip SHA-256 142190dd...755a, latest; re-downloaded, hash
  matches. mods-info c392a26: ruffle-coop latest/min 1.0.2, protocol rc-1.0.2. Launcher 1.0.33+ updates on open.
- Gun Mayhem 2 + up to 4 players (user: "make gun mayhem 2 work", "it should work up to 4 players"). Asked the
  "Gun Mayhem 2: modes and sync" session (local_aafeb7ce-4d41...) for a clean co-op SWF, its sync risks, a key answer
  and a menu script. Here: --coop-keys bad-ice-cream|gun-mayhem-2 (Game enum; GM2 = its savedata2.controlarray
  defaults: P1 arrows+Z/X, P2 WASD+T/Y, P3 numpad / 7 8 9 + * -, P4 numpad 5 1 2 3 + 0 .; numpad keys sent with
  KeyLocation::Numpad so AS2 sees 96-111; locally Z/T shoot, X/Y grenade). keys=<game> joins the handshake's
  player settings (mismatch refused by name). --coop-players N (2..game max). Protocol: Hello has a random id (host
  0); Start lists every player (id:name) in slot order; Refuse names the joiner's id (it was sent to every friend,
  which would kick accepted friends); friends only check the host's Hello. Host waits for N-1 friends, pings all.
  Test four_players_stay_in_sync GREEN; mutation (host starts with 1 friend) -> FAIL. lab.py --players/--keys:
  3 and 4 players direct 34 ms, relay 20 ms lag 67 ms, in sync. 3 players at 40 ms lag ran 26.4 s for 20 s on
  the single worst ping: a friend's keys reach another friend via the host -> path_ping = two worst pings added
  (test) -> 20.0 s, 134 ms. Live relay 4 players on this PC (room GFFW5): ping path 321 ms -> 6 frames, 200 ms,
  in sync, full speed. Desktop tests 97/97.
  GM2 session answer (read from decomp, nothing built: on hold until the user tells it): AS1 random(n) at 185
  sites (covered: same AvmRng, seeded from the frame-clock locale); _quality changes particle spawns + pDepth
  (covered: quality is a compared setting, no context menu in a session, save Q shared); Esc/Space/Home pause/quit
  (never sent: only mapped keys); it will force controlarray defaults for all 4 slots in its build. Replay now feeds
  as many players as the game has keys for. windows_lab PASS both modes, tests 97/97.
- GM2 co-op SWF from the GM2 session (build.py --coop, sha256 080b5f9b...d901, original + controlarray defaults for
  4 slots at arena start, no netcode). Verified here: --coop-replay 3300 frames with its 4-human script
  (gm2-mod/lab/ruffle-together-4p.txt) twice -> identical (8e30f66c...); screenshots 1800/3290 show a 4-human
  Avalon match with kills, grenades, last lives. lab.py/windows_lab.py gained --script (lab.py had always used
  Bad Ice-Cream's script, so its GM2 runs never left the menu). Real-match lab, 3300 frames (GM2 is 35 fps):
  2p relay 20 ms lag 61/62 ms, 2p direct 33/33, 4p relay 92-93 ms, all in sync, 94.3-94.5 s. The 4p direct run
  failed to start: gunmayhem2_coop.swf was deleted from gm2-mod/dist at 20:06:57 by an online rebuild; asked the
  GM2 session to restore it outside dist/.
- GM2 session published the same file (sha 080b5f9b...) as flash-games games/gunmayhem2/gunmayhem2_online.swf
  (commit 5fe43fe, user's request there); the launcher's GM2 entry now downloads it, but released Ruffle Together
  1.0.2 has no --coop-keys / 4 players -> GM2 online is broken for players until 1.0.3 + a launcher update.
  On a private copy: lab 4p direct 36-37 ms, 3p relay 40 ms lag 119-120 ms, 3p direct 35 ms, all 3300 frames in
  sync, 94.3-94.6 s. windows_lab limits now follow the game's own rate (GM2 35 fps read as a failure at 30's
  limits) and print late frames per 5 s on failure. GM2 windows: pressure 35/35, 1 late; quiet 1st run 4 late +
  34.4 fps min (not repeated), then 2 runs PASS (2-3 late = menu/map loads).
  Stable GM2 test copy (no build touches it): ai-work-flash-games/gm2-mod/ruffle-together/gunmayhem2_online.swf
  (sha 080b5f9b...) + ruffle-together-4p.txt. dist/gunmayhem2_online.swf can be the old relay build (build.py
  without --coop).
- Release v1.0.3 (user: "push, publish release 1.0.3, bump mods-info, and ask the launcher session ..."): commit
  d21ad3d, tests 97/97, tag v1.0.3, RuffleTogether-1.0.3-windows-x64.zip SHA-256 a510def9...0a7c, latest,
  re-downloaded hash matches. Live relay with the downloaded exe: Bad Ice-Cream 2p (room ZAEG4) 100/100 ms in sync;
  GM2 4p with the flash-games file (sha 080b5f9b..., room UTDJE) 1500 frames in sync, 43.3-43.5 s (42.9 at 35 fps),
  path ping 328 ms -> 7 frames (all 4 on this PC). mods-info decbe2b: ruffle-coop 1.0.3, game names both games.
  Asked the launcher session to switch GM2 to Ruffle Together (online id ruffle-coop, --coop-keys gun-mayhem-2,
  host --coop-players picker, no FlashVars/--tcp-connections, drop the dist fallback, guide without F9).
- Launcher 1.0.34 (launcher session, verified latest + launcher.json): GM2 online id ruffle-coop; Play online host
  --coop-players 2, "Host a room for 3/4 players" and direct host 2/3/4 under Other ways; joins with --coop-keys
  gun-mayhem-2; no FlashVars/--tcp-connections; swf only GAMES_DIR. Its test: --launch gunmayhem2:host4 gave the
  expected command line. Not played in a match there; not tried on two PCs.
- User's first GM2 game through the relay (2 players, room P9GFE): mid-match both windows "<other> stopped
  answering" at once. Friend's log (RTX 3050, player 2): delay 4 frames (high ping), frequent waits up to 448 ms
  before; cut off before the end. Relay checked: star forwarding is lossless and ordered (throttles at 600 msg/s,
  never drops); TCP doesn't lose lines; so not a lost message. Both giving up together = the link stalled both ways
  > GIVE_UP (5 s) while connected. Test a_long_stall_is_waited_out (friend silent 8 s, connection open) RED
  ("tester stopped answering") -> GIVE_UP 60 s (a leaving player is still noticed at once by the closed
  connection; friend_leaving test < 6 s still passes) + a WARN "the game waited N s" after any stall > 2 s -> GREEN.
  Tests 98/98. Not confirmed: the host's log (other PC) for what stalled. GM2 also floods the log with AVM1 "Cannot
  delete property onEnterFrame" warnings (1.5 MB per game).
- Release v1.0.3.1 (user: "its not a huge one so like 1.031"; four-part version so it sorts after 1.0.3 and before
  a future 1.0.4: the launcher's compareVersions and the mods-info schema both take up to 4 parts). Commit 789cddb,
  tests 98/98, zip SHA-256 bfa87cc6...ce26, latest, re-downloaded hash matches. Live relay GM2 2p with the
  downloaded exe (room SQ9DL): ping 160 ms -> 4 frames, 116/116 ms, 1500 frames in 42.9 s, in sync, no stall
  lines. mods-info 430e0f5: ruffle-coop 1.0.3.1.
- Waiting screen in the window (user: replace the room-code pop-up with something like Ruffle's own UI). The
  handshake now runs on a thread after the window opens (headless keeps the blocking path); coop::Lobby (room,
  port, joined names, needed, error) is filled by Session::start and drawn by gui.rs show_lobby (egui) until the
  session is set, then app.rs loads the game as before (on_metadata resizes the window). Room-code and
  can't-start message boxes removed. Checked with PrintWindow captures (no input): host on the live relay showed
  room H6HY8, "Waiting for players: 1 of 2", Player 1: Liam; a friend joined -> host window switched to the game
  (room H6HY8 - player 1); wrong code ZZZZ9 -> "Can't start the game / No room with code ZZZZ9 ..." in the
  window. Tests 98/98, windows_lab both modes PASS.
- Release v1.0.3.2 (user: "push, publish release 1.0.3.2 and bump mods-info"): commit f1fc8d9, tests 98/98, zip
  SHA-256 8ebf601e...491f, latest, re-downloaded hash matches. Live with the downloaded exe: windowed host showed
  room USST9 + "Waiting for players: 1 of 2"; a friend joined by code, both started (4 frames, 162 ms ping), host
  window switched to the game.
- One window at a time (user: "close ruffle if they trying to open another one"): main.rs close_previous_window
  before the session starts (so a direct host's port is free): reads <cache>/window.pid, closes that process only if
  tasklist shows it is ruffle_desktop (pids are reused), waits up to 2 s, writes its own pid. Applies to every
  windowed start (sessions and plain Ruffle); headless skipped; labs use their own --cache-directory. Checked: A
  hosting port 7846 then B in the same cache folder -> A closed (log line), B hosted 7846 with no error; C in another
  folder stayed open. Tests 98/98, windows_lab quiet PASS. Not released.
- Release v1.0.3.3 (user: "push, publish release 1.0.3.3 and bump mods-info"): commit 4a2da85, tests 98/98,
  zip SHA-256 421f17fa...accf, latest, re-downloaded hash matches. Live with the downloaded exe: a solo window was
  closed by a host window started in the same cache folder (log line); the host and a friend from another folder
  met in room SVMT2 and started (4 frames, 162 ms ping).
- Territory War key map (request from the "Territory War online mod" session): --coop-keys territory-war, hot-seat:
  4 identical slots with SHARED keys (a game key is down while any player holds it; GameKeys.shared, events on the
  OR of all slots). New buttons KEY1-KEY4 VIEW QUIT YES NO (bits 64..8192 of the u16 that already travels; script
  names too). Keys: arrows, Space=FIRE, X=ALT, 1-4, V, U, Y, N (top-row digits, Standard location -> codes 49-52);
  local keys are exactly those (no WASD). Tests first (shared keys, local keys, names, script) RED -> GREEN;
  desktop 100/100. Replay of territory-war-279.swf 800 frames with FIRE/KEY1/VIEW twice: identical (title screen
  only; a match needs the TW session's script). Its onSoundComplete flag only gates a sound: no fingerprint issue.
  Not released.
- Co-op API for SWFs (request relayed by "Flash game multiplayer mod folders": every Flash game moves onto Ruffle
  Together; mouse-aimed games need every slot's mouse + the local slot). Design: ExternalInterface, the same in AS2
  and AS3: coop.players, coop.localSlot, coop.mouseX/mouseY/mouseDown(slot) (stage pixels). coop/api.rs
  (CoopApi + provider), Stepper installs the provider on the first frame and updates it from each frame's inputs
  before the frame runs; every PC now sends its mouse (only slot 0 still drives Ruffle's own mouse: _xmouse,
  stage.mouseX and events stay the host's). Scripts: move/click X Y [slot]. Tests first: API answers; Ruffle's own
  AS2 + AS3 ExternalInterface test SWFs reach the provider through the Stepper; 3-player session where each PC
  sends its own mouse -> every PC has identical per-slot values; script slots. RED -> GREEN, 103/103.
  GM2 4p replay hash unchanged 8e30f66c (GM2 doesn't use EI); GM2 with 4 slots' mice moving/clicking twice
  identical; BIC twice identical; lab.py (new --exe) with mice: 2p and 4p relay (20 ms lag) and direct in sync
  (4p direct 60.2 s vs 57.1: another session's 4 Ruffle processes were running). localSlot rule documented: the
  fingerprint covers every object's position/visibility/depth/frame, so per-PC moves/visibility/objects show OUT
  OF SYNC; per-PC camera not supported. Built while another session's processes held target/release
  ruffle_desktop.exe: checks used target/release/deps/ruffle_desktop.exe. Not released.
- Key map FILES (7 key-map requests at once: bow-man, stealth-hunter-2, smileys-war, stealth-assassin, intrusion,
  air-battle and intrusion-2 aliases): keymap.rs now parses small text maps (slot N: BUTTON=keycode..., local:,
  shared); the three old maps became built-in files (desktop/src/coop/keys/*.keys) and all 10 are built in;
  --coop-keys takes a name or a file path; the handshake compares name#sha256(text). Core: pub coop_key_codes
  (physical + logical key code of a key) to check the table and map local keys. Tests: every one of the 80 codes
  round-trips through Ruffle's physical AND logical mapping; bad files name the line; a file loads; every
  existing key test kept. 107/107. Replays identical to before for BIC (fa623659), GM2 4p (8e30f66c) and TW
  (8f0d87b8); GM2 from a .keys file = built in. Test build: test-build/ruffle_desktop.exe + *.keys (target/release
  exe locked by another session's processes). Not released.
  Spec for game sessions: side-session-notes/.../ruffle-together-api-and-keymaps.md; sent to the mod-folders parent
  (queued) and to Territory War, Smileys War, Stealth Assassin (UA series), Air Battle, Intrusion 1, Intrusion 2,
  Stealth Hunter 2 (queued). Bow Man's reply was NOT delivered (its workspace needs the user's trust approval).
- Ultimate Crab Battle key map (request from its session): --coop-keys ultimate-crab-battle, P1 arrows + A, P2
  IJKL + U, P3 TFGH + R, P4 numpad 8456 + 0; every slot also KEY1-3 = 1-3, ALT = Ctrl, YES = Y, NO = N. Local:
  arrows / W S D (no A), FIRE = Space, Enter, Z, F. Test ultimate_crab_battle_keys; desktop 108/108. Commit 048c81d.
- Release v1.0.3.4 (user: "1.0.3.4", after the Stealth Hunter 2 session relayed its ask; the 1.0.4 content under a
  1.0.3.x number): commit 141b54c, tests 108/108, zip SHA-256 dc05f145...6581, latest, re-downloaded hash matches,
  exe reports 141b54c. Live relay with the downloaded exe, headless BIC 2p (room S7WZV): 750 frames, desync_at=none
  both, felt delay median 101 ms. mods-info 3e963cd/df644b4 (1.0.3.4) and 434f083 (game text). Repo About text
  now describes Ruffle Together; homepage -> releases/latest.
- Smileys War windows_lab desyncs (both found; headless lab/replay had stayed in sync). Method: replay env hooks
  COOP_REPLAY_WINDOW=1 (random extra renders + resizes between frames, like a real window) and COOP_REPLAY_AUDIO=1
  (this PC's audio device): the untouched SmileysWar.swf + k2p2m1.txt diverged at frame 888 (window) and 1115 (audio).
  1) Window size: core `local_mouse_position` (_xmouse/_ymouse, AS3 mouseX/mouseY) rounds the mouse through device
  pixels with `stage.view_matrix().a`, so each window size gave a different _xmouse (the game's cursor clip at
  8040,7040 vs 8044,7034; the V2 List's dragScroll reads _ymouse -> tran_mc tween vs none). Fix: ratio 1 in a
  session (frame clock set). 2) Audio: event sounds started in a session get a FrameClockSound (start, in offset,
  loop length x loops): update_sounds and Sound.position / SoundChannel.position follow the frame clock, so
  onSoundComplete / soundComplete fire at the same frame on every PC (streams still follow the device).
  NullAudioBackend now gives each sound its own instance handle (it gave all the null handle, so a PC without
  audio attached every SoundChannel to the first sound). Tests: core session_mouse_is_the_same_in_every_window_size
  and a_sound_ends_after_its_loops_on_the_frame_clock; desktop sounds_complete_on_the_frame_clock (Ruffle's AVM2
  soundchannel_soundcomplete SWF, no audio device); both RED with the fix disabled. Smileys War replay: plain =
  window = audio = both, 2500 frames. Baselines: old test-build exe vs new identical files for BIC (1900 f), GM2 4p
  (3300 f, sha 8e30f66c) and TW (2000 f). Tests: desktop 109/109, core lib 131/131, SWF suite 4285 ok / 0 failed.
  windows_lab both modes with the untouched SWF: in sync (no OUT OF SYNC, fingerprints equal) but FAIL on late
  frames/fps: 8 other sessions' Ruffle processes were running. Not released.
- Smileys War desyncs fixed by the "Find the Smileys War windows_lab desyncs" session (c51a48a, merge ad4cf70):
  `_xmouse` the same in every window size; sounds end on the frame clock.
- Intrusion 2 4-player desync at frame 24 (its session's report): Ruffle's 15 s script limit is real time, so on a
  busy PC a long first-menu script was stopped on some PCs only. Reproduced with 32 busy processes (3/3 desync),
  sessions now never stop a script (0356d0b): 2/2 in sync. (Loading child movies fully each frame was tried first
  and did NOT help; reverted.)
- keyPress buttons (Ultimate Crab Battle's report): sessions only sent key down/up, and AS2 `on(keyPress "1")` and
  text fields hear the typed character (TextInput); a real keyboard missed it too (the window dropped TextInput).
  A slot's key press now also types its character (4a3cd49). Crab Battle confirmed on its real menu.
- Why a game stopped (user's friend NickBin's game stalled mid-round in SH2/GM2/UA; the friend's log only said "the
  host left"): the session is a static never dropped, so no Bye went out on close. Now a goodbye on window close
  and from the panic hook (with the crash text, also logged), an Alive heartbeat thread every 1 s (frozen game vs
  nothing at all), a stall explained after 5 s, slow (>2 s) or stuck frames logged on the PC itself, and the last
  5 logs kept (ruffle.1.log...). 7b764fe, 115/115. Still unknown: why NickBin's Ruffle stopped (needs its log).
- Release v1.0.3.5 (user: "release coop as 1.0.3.5", then "Without the API"): commit b3f0766 (API round 2, merged
  into local coop by its session meanwhile, is NOT in it), tests 115/115, replays BIC fa623659 / GM2 8e30f66c /
  TW 2p 3000 frames ad6fe22d, zip SHA-256 bf527367...e0ea, latest, re-downloaded hash matches, exe reports
  b3f0766. Live relay (room TVCBY) with the release exe: 750 frames in sync both, 101 ms median. mods-info
  4e6077d. GitHub README (master 2a1d7fb) now describes Ruffle Together.

## 2026-10-29 (relay move)
- The "waiting for host" stalls were the relay host, not Ruffle Together: relay 1.2's logs on Kinetic
  (195.60.166.57) showed both clients' traffic stopping at about 71 s and a reset at 122 s; the same relay locally
  and a direct connection ran for minutes. The relay now runs on DigitalOcean Amsterdam, 165.232.82.11:25569
  (droplet `mods-relay`, systemd `mods-relay`): two Stealth Hunter 2 windows ran 7 min with no drop (user: smooth).
  mods-info servers.json points there (8c79139); Kinetic stays up for older clients.
- Lobby (f21493d): the host sends waiting friends a Lobby message every 2 s, so an early friend no longer gives
  up after 30 s; friends see who is in. Co-op saves (ae82f69): every PC writes the session's saves to
  `<save dir>/coop/<game file>.save`, solo saves untouched, frozen after a desync; host picks the start with
  `--coop-save coop|solo|new` (08ee769). MENU button, SH2 Esc (08fc160, not checked in-game).
- Release v1.0.3.6 (user: "release it"): commit 1c5da5c on claude/ultimate-crab-battle-keys (API round 2 NOT in
  it), tests 123/123, replays BIC fa623659 / GM2 8e30f66c / TW ad6fe22d, live relay 165.232.82.11 room EMSHH 1500
  frames in sync with the release exe, zip SHA-256 d44451fa...bc69, latest, re-downloaded hash matches. mods-info
  918d218. Merged into local coop (with API round 2): 131/131.
- API rounds 2 and 3 seen working (2026-10-30): `tools/coop-api/demo/` (CoopDemo.swf, every `coop.*` call on
  screen; README there). Replay twice identical (0e27a771); two headless players through 165.232.82.11 room Z9CBB
  1500 frames desync_at=none; two real windows (test-build exe, script only) showed labels, per-slot cameras, the
  HUD, typed name board, key-map switch, game pause ("Paused") and Esc pause ("Paused by Ann"), events, options,
  ping. Found: with a co-op camera, shapes, text and bitmaps outside the original stage were culled (only
  containers showed), so a big world drew no rings, cursors or text there. Fix: `Stage::view_bounds` is the stage
  plus what the camera shows.
- Pausing removed (user: "i dont see a reason with coop.pause, get rid of it completely"): no PAUSE button, Esc is an
  ordinary key again, no `coop.pause`, no pause/resume events, no "Paused by" overlay. 147/147, replays unchanged.
- Release v1.0.3.7 (user: "yes" after the API demo in two real windows): commit 0da417f = local coop 864e0c9
  (API rounds 2 and 3, play on when a friend leaves, chat, Discord join, fireboy-watergirl map, camera culling
  fix; pausing removed) + version. Tests 147/147, replays BIC fa623659 / GM2 8e30f66c / TW ad6fe22d, live relay
  room HD659 1500 frames in sync with the release exe, zip SHA-256 ff93d7ab...6efb, latest, re-downloaded hash
  matches. mods-info 7a97236.
- keyPress buttons again (Dad 'n Me's report via the Flash folder session): in --coop-replay an A press didn't
  trigger on(keyPress "a") menu buttons. The typing change (933c613) had removed 4a3cd49's "a key press types
  its character", so only a real keyboard's own character could reach them, and only for the launch map's local
  keys (not a game's coop.keys map, not scripts or replays). Now each slot's key press types its game key's
  character again, from any map, while no text box is selected; while one is, only what players typed goes in
  (no double letters). Untouched DadNMe.swf, A presses only: 1.0.3.7 stays on PLAY, the fix reaches the park.
  Tests 148/148, replays BIC fa623659 / GM2 8e30f66c / TW identical to the 1.0.3.7 exe.
- Release v1.0.3.8 (user: "Test in windows first", then release): commit 908e2cd = eca5b3a (keyPress from any key
  map) + version. Windows: DadNMe_coop.swf host + friend, A presses only: both windows in the park, same
  fingerprints (1.0.3.7: still on PLAY). API demo: replay twice = 1.0.3.7 (ec7d97f6), headless pair on the relay
  room UB63M desync_at=none, two windows on the relay room UUMBX 5274 frames same fingerprint. Tests 148/148,
  replays BIC fa623659 / GM2 8e30f66c / TW ad6fe22d, live relay room NAZD9 in sync, zip SHA-256
  630847fe...8a59, latest, re-downloaded hash matches. mods-info 8f47aff.
- Discord presence on (app id 1565524080453755040): two demo windows through the relay, the user's profile showed
  "Ruffle Together / Playing CoopDemo / room 9V8V6 (2 of 2)". Asks for the art asset "logo" (not uploaded yet: a
  "?" shows until it is). Join button still off (no join page yet).
- Built-in key map `chaos-faction-2` (Chaos Faction 2 session's request, its file verbatim): 2 slots, every
  player presses the game's solo keys; P2 travels as numpad codes the mod maps to player 2; only P1 pauses
  (Space). A built-in map and the same `.keys` file get the same id (name + text hash), so both can meet.
  Tests 149/149, replays BIC fa623659 / GM2 8e30f66c / TW ad6fe22d; ChaosFaction2_coop.swf match2.txt 3000
  frames: built-in and file both 1748d03d. Not released yet (next release).
- Friends' clicks (user: "1 + 2 + 5 (but whenever the game allows the click) and maybe 9"): key map `mouse: host|buttons|all`
  and `zone N: X,Y WxH`, API `coop.mouse`, `coop.zone`, `coop.clicked`; friends' cursors drawn while friends may
  click. "buttons" = Ruffle's hand-cursor test at the friend's point. Test SWF desktop/tests/coop/clicks; demo:
  Bob clicks Red, Ann Blue, Ann refused on Bob's switch (his zone), Bob's switch ON, Bob refused on empty floor,
  Green: both windows' logs identical (frames 147-302). Replay twice 3fa99a8d, relay room Q6DSQ 1500 frames
  desync_at=none. Tests 153/153, replays BIC/GM2/TW unchanged. Found: a game's coop.keys reset the click rules;
  now a map changes them only if it has `mouse:`/`zone` lines.
- Chaos Faction 2 character screen (user: "do on chaos faction 2", "Yes, add them"): built-in map gets `mouse: buttons`
  and zones (P1 card host, P2 card below its header Bob, the P2 drop-down host). Replay (match2 + picks): Ann flips
  hers, Bob flips his, each refused on the other's card, Bob refused on the drop-down, Bob may pick for P3 (open);
  into the fight, replays identical (ed74c5a5), relay room FPLU4 3600 frames desync_at=none, two windows same.
  The mod's own mod/chaos-faction-2.keys (used by the launcher until the next release) needs the same lines.
- User tried it: "the other player can click anything. i wanted this to work only on their character selection."
  New `mouse: zones`: a friend's clicks count only in their own zone (on clickable things); the host everywhere but
  others' zones. Chaos Faction 2's map: `mouse: zones`, `zone 1: 302,190 194x163` (P2 card below the drop-down).
  Replay: Bob's own card changes, his P3 click is refused now.
- User: "weird bugs with the mouse ... the host can't click". Cause: one click at a time, and a friend's click could
  stay "held" forever (release outside their window, or a right click whose release Ruffle's menu swallows, since
  every button counted as a press), so the host's clicks were dropped. Fix: the host's click always goes through
  (a held friend's click is let go first), only the left button presses, a window losing focus lets go of its
  button. Test: a friend stuck down doesn't block the host. 153/153, replays unchanged.
- User: the host couldn't click Start!/Settings/Level Editor (Credits worked). Cause: key-map zones were fixed boxes on
  every screen; Bob's character-card box covered the title menu (and let Bob click it). Fix: a zone can name a clip
  (`zone 1: _root.players.p2`): it covers only buttons inside that clip, only while it is on show; the most exact
  clip wins (`zone 0: _root.players.p2.combo` keeps the drop-down the host's). Paths found by asking Ruffle what
  each click lands on (now logged while friends may click). CF2 replay: Bob's Settings click on the title refused,
  Ann's Start! works, Bob's card only; replays identical (7fa064fb), relay room 2G9E9 3600 frames desync_at=none.
  153/153, BIC/GM2/TW unchanged.
- Checkpoint (user: "remember the ruffle build we have rn in case it becomes too broken"): commit d0ac840 built clean
  into `ruffle-checkpoints/2026-10-30-d0ac840/` (exe, its Chaos Faction 2 map, README); git branch
  `checkpoint/friends-clicks-2026-10-30`. Taken before the private-screens work.
- Private screens (user: "i want only the player that clicked on it to see it"): key map `private: <clip>` / API
  `coop.only(path)`, `coop.only(slot, path)`, `coop.shared(path)`. The clip belongs to whoever's click opened it:
  only their PC draws it (new picture-only flag, like the HUD), only they click it (it is their zone), everyone else's
  mouse goes through it (it steps out of picking for their mouse events) and their cursor isn't drawn over it.
  User found: with one editor in the game, a second player opening theirs took it over; and the host's hover showed
  for everyone. Fixes: `opened-by <pattern>` (and `coop.only(path, pattern)`): while it is someone's, nobody else's
  click on its openers counts, and they see "Bob has that open: wait until they close it"; `hover: off` (the
  default while friends may click) rests the game's mouse off the stage between clicks, `hover: host` / `coop.hover`.
  Chaos Faction 2: `private: _root.pcb opened-by _root.players.*.instance*` (the editor is _root.pcb, Edit Player
  buttons are _root.players.pN.instanceK, found from the click log). Windows: Bob's editor only on Bob's screen, Ann's
  Edit Player refused with the notice, after Bob's Cancel Ann's opens only on hers, Bob's refused. Replays identical
  (5b2d0408), relay room GHRMG 2800 frames desync_at=none. Tests 154/154, replays BIC/GM2/TW unchanged.
- Hover (user: "hovering doesnt work now at all", then "this is not even the original game's highlight ... you
  just made smth custom"): parking the game's mouse off the stage killed all hover and also mouse-move effects
  (Chaos Faction 2's help bubble stayed up, covering Bob's arrows); the white outline I drew was custom. Chaos
  Faction 2's highlights (the yellow arrows, the glow) are made by its code, so they can only follow one mouse. User
  chose the game's real hover: `hover: host` is the default again and in Chaos Faction 2's map; the outline is gone.
  Kept as options: `hover: own` (per PC, only Button symbols' own Up/Over art, built from their records without
  running code; nothing custom) and `hover: off`. Tests 154/154, replays BIC/GM2/TW unchanged, Chaos Faction 2
  replays identical (18b022a3), relay room F72NH 2800 frames desync_at=none.
- Click queue and 4 players (user: "queueing it is"): Flash has one mouse button; friends' clicks that land together
  used to be dropped. Now they queue (one a frame, oldest first, then by slot; a queued player who already let go
  gets a press and a release); the host's click still goes at once; a friend holding the button over 30 frames while
  others wait is let go. Scripts gained `press X Y SLOT` / `release SLOT` (no frame passes). Test
  four_players_take_turns_on_the_one_mouse: 3 friends in one frame all arrive in slot order, player 3's box keeps out
  player 4, a long hold gives way, player 3's private pop-up with 4 players. API demo demo-4p.txt: replay twice
  identical (4c06b3f1), 4 headless players on the relay room HNNSJ desync_at=none for all 4, 4 real windows (room
  846PM): every window logged Red f270, Blue f271, Green f272, player 4 refused on Bob's switch, Bob's switch ON.
  156/156, replays BIC/GM2/TW unchanged, Chaos Faction 2 replay unchanged (18b022a3). Seen: a name tag of a dot out
  of view is drawn outside the game area in a small window (picture only).
- coop.hud inside a moved parent (Intrusion 1 session's report): a HUD clip undid the camera inside its parent's
  transform, so in a game that scrolls by moving `_root` (Intrusion: _root._x ~6000, _root.ui at minus that) the
  HUD landed (zoom-1) * root offset away, off screen. Now `coop_view::hud_matrix` = P^-1 * camera^-1 * P * own
  (P: the parent in stage coordinates), so the HUD lands where it would with no camera. Culling needs no change:
  the HUD's stage bounds are inside view_bounds' stage half. Intrusion_coop.swf rebuilt with N.CAMS = true (a
  scratch copy), four.txt 1150 frames: 1.0.3.8 shows no HUD, the fix shows it where the cameras-off build does;
  fingerprints identical (picture only). Tests 149/149 + core coop_view; replays BIC/GM2/TW unchanged. Not released.
- Release 1.0.3.9 (1a56efb, user: "release it"): friends' clicks, zones, private screens, hover options, click
  queue, Discord presence, chaos-faction-2 key map, coop.hud fix. Clean build, tests 156/156; replays BIC 1500
  fa623659, GM2 4p 3300 8e30f66c, TW 3000 ad6fe22d; API demo replay twice identical (3fa99a8); live relay
  165.232.82.11:25569 with the packaged exe: BIC 1500 frames and API demo 1500 frames desync_at=none. Zip sha256
  d7d0a0560891193f6504b1d2fe19f2ee5cc6bbb20089be06b087eede746c4eb0 (re-downloaded, same); mods-info 2d5ecf1
  (latest = min = 1.0.3.9).
- One mouse per player (user: "proper multiple mouses ... proper hovering with the original hovering effects";
  plan in multi-mouse-plan.md). Ruffle's core keeps every player's mouse (`core/src/coop_mice.rs`): position,
  hovered, pressed, button; each player's events run with their mouse swapped in, so Ruffle's own hover, press,
  release and drag code runs for each. An object rolls over for the first mouse on it and out for the last;
  `_xmouse` / `stage.mouseX` in a handler is that player's; `startDrag` follows the mouse that started it. The
  borrowed mouse, the click queue, the hold limit, parking and `hover: off` are gone: several players press and
  hold at once. Rules (`desktop/src/coop/mice.rs`, moved out of frame.rs): a friend's mouse is in the stage only
  where their mode allows (`mouse:`, new `mouse N:` / `coop.mouse(slot, mode)`), zones and private screens as
  before. `hover: own` (new default): each PC draws Button symbols by its own player's mouse in the game's real
  Up/Over/Down art (AS3 SimpleButton states; AS2 art copied without scripts, clips shared by the states drawn
  live); `shared` (old `host`) as the game has them. New: `coop.mouse.clicker`, `coop.mouse.hovered(slot)`,
  `coop.mouse.cursors` / `cursors:`, idle friends' cursors fade after 3 s, `--coop-mouse-debug` overlay.
  Grouped game-side library `tools/coop-api/as3/Coop*.as` and `as2/Coop.as` (`Coop.camera.setPosition(...)`);
  groups are made on first use (made at class load, asc's file order broke it: error #1007). API demo moved to it.
  New test SWF desktop/tests/coop/mice (hover counting, clicker, drag, a SimpleButton). Tests 161/161 (+5 new),
  replays BIC/GM2/TW unchanged; picture test: host's PC draws the button Up, player 2's Over, same fingerprint.
  API demo replays identical twice (2p 3fa99a8d, 4p 10073f2d), 4 headless on the relay room LN39R desync_at=none
  (a first try hung before the start, the relay's), 4 real windows room NM65M: identical logs, the three friends'
  presses on the same frame 270 (was 270/271/272), `by P2/P3/P4`, player 4 refused on Bob's switch. Chaos Faction 2
  replay identical twice (67a7e305; was 18b022a3: friends now have mice).
- Release 1.0.4 (a7d989b, user: "push and release"): one mouse per player, hover own/shared, mouse N:, cursors:,
  coop.mouse.clicker / hovered, the Coop library, --coop-mouse-debug; docs page "Modding a game you didn't write"
  pushed to master (6fe08df, Read the Docs). Clean build (exe names a7d989b), tests 161/161; replays BIC fa623659,
  GM2 8e30f66c, TW ad6fe22d; API demo 4p replay twice 10073f2d; CF2 replay twice 67a7e305; live relay
  165.232.82.11:25569 with the packaged exe: BIC 1500 frames (room EFM8K) and API demo 1500 frames (room UDRUE)
  desync_at=none. Zip sha256 ff4b25ad20b8dd677e0def2787ee18723009edc2820d575b151c4dfc0dea23e2 (re-downloaded, same),
  exe 38d2b25287478d311818faa613935f93693dfe152eb3de1f6c4f57f9498ca034. mods-info 12777dd (user: "bump mods-info"; rc-1.0.4, live). Read the Docs did not build 6fe08df (last build 78d91c9): the new page needs a manual build.
- Release 1.0.5 (0703088, user: "commit this and merge this and make a new release if needed"): up to 8 players
  (d1bbd90), a mouse for each of the 8 (bbf95d6: core MAX_POINTERS 8, a slot core has no mouse for stays off), the
  Chaos Faction 2 key map for 6 (c119198; the mod's 6-player SWF is ai-work-flash-games 7507968, not on flash-games
  yet). Tests 163/163; replays BIC fa623659, GM2 8e30f66c, TW ad6fe22d, API demo 4p 10073f2d twice, CF2 2p 67a7e305
  twice (all as 1.0.4); CF2 6p replay twice 09967baa; buttons_check --at 2600: every fighter moves/punches/kicks,
  weapons on the lab SWF, P2's taunt never fires (as on 1.0); lab 6 players relay (40 ms) and direct in sync; 6 real
  windows direct, the same fingerprint at frame 5274. Live relay with the packaged exe: BIC 1500 frames room APA5V
  in sync. Zip sha256 5d7395cfccd246235b5e5ed5eba5bac0d6d8ef2fbb56691050ca51ad3d30d6dc (re-downloaded, same), exe
  4d745f49526e39fabb14f0e9bad17d6e8140b2e293c18b25b16cd0354342005a. Docs site 446fc53 (1.0.4 and 1.0.5).
  mods-info 7ecf0bb (user: "bump mods-info"; rc-1.0.5, max_players 8, live). Public relay with the packaged exe:
  6 headless CF2 players room 843JQ 3000 frames in sync (median 186 ms); a first try a minute after the push hung
  (two players never got in), likely before the relay re-read mods-info.
