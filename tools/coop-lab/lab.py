"""Ruffle Together lab: two players on this PC, no windows, scripted input.

Runs two `ruffle_desktop --coop-headless` players against each other, through a local copy of the
relay with simulated lag (relay mode) or directly (direct mode), and prints each player's result:
whether the games stayed in sync and how long inputs took to reach the game.

  python tools/coop-lab/lab.py --swf <BadIceCream_coop.swf> [--mode relay|direct|both]
      [--frames 4500] [--lag 40] [--jitter 10] [--delay N] [--players 2] [--keys bad-ice-cream]
      [--script <input script>]

Nothing touches the keyboard or mouse; test ports avoid the ones real games use.
"""
import argparse
import json
import os
import re
import socket
import subprocess
import sys
import tempfile
import threading
import time

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
RUFFLE = os.path.join(ROOT, "target", "release", "ruffle_desktop.exe")
SCRIPT = os.path.join(ROOT, "desktop", "tests", "coop", "bic-two-players.txt")
RELAY = os.path.join(ROOT, "..", "global-mods-server", "init.py")
RESERVED = {6420, 7777, 7778, 7779, 27015, 27016, 27020, 27021}


def free_port():
    while True:
        with socket.socket() as s:
            s.bind(("127.0.0.1", 0))
            port = s.getsockname()[1]
        if port not in RESERVED:
            return port


def player(args, role_args, name, out):
    delay = ["--coop-delay", str(args.delay)] if args.delay is not None else []
    cmd = [args.exe, "--coop-headless", "--coop-frames", str(args.frames), "--coop-script", args.script,
           "--coop-name", name, "--coop-keys", args.keys, "--coop-players", str(args.players)] + delay + role_args + [args.swf]
    proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, errors="replace")
    out[name] = {"proc": proc, "lines": []}
    return proc


def read_lines(entry, code_found=None):
    for line in entry["proc"].stdout:
        line = line.rstrip()
        entry["lines"].append(line)
        if code_found is not None:
            m = re.search(r"room code: (\w{5})", line)
            if m:
                code_found.append(m.group(1))


def run(args, mode):
    out, relay = {}, None
    try:
        if mode == "relay":
            games = os.path.join(tempfile.mkdtemp(), "games.json")
            json.dump({"ruffle-coop": {"name": "Ruffle Together", "mode": "star", "max_players": 8}}, open(games, "w"))
            port = free_port()
            relay = subprocess.Popen([sys.executable, RELAY, "--port", str(port), "--no-mods-info", "--no-discord",
                                      "--games", games, "--sim-delay", str(args.lag), "--sim-jitter", str(args.jitter)],
                                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            for _ in range(100):
                try:
                    socket.create_connection(("127.0.0.1", port), 0.2).close()
                    break
                except OSError:
                    time.sleep(0.1)
            addr = "127.0.0.1:%d" % port
            code = []
            host = player(args, ["--coop-host-relay", addr], "Host", out)
            t = threading.Thread(target=read_lines, args=(out["Host"], code), daemon=True)
            t.start()
            for _ in range(200):
                if code:
                    break
                time.sleep(0.05)
            if not code:
                return "no room code from the host: " + " | ".join(out["Host"]["lines"][-5:])
            join = ["--coop-join-relay", addr, "--coop-code", code[0]]
        else:
            port = free_port()
            host = player(args, ["--coop-host", str(port)], "Host", out)
            t = threading.Thread(target=read_lines, args=(out["Host"],), daemon=True)
            t.start()
            time.sleep(1.0)
            join = ["--coop-join", "127.0.0.1:%d" % port]
        names = ["Friend"] if args.players == 2 else ["Friend%d" % i for i in range(1, args.players)]
        readers = []
        for name in names:
            player(args, join, name, out)
            readers.append(threading.Thread(target=read_lines, args=(out[name],), daemon=True))
            readers[-1].start()
        for name in names:
            out[name]["proc"].wait(timeout=args.frames / 25 + 120)
        host.wait(timeout=60)
        for reader in readers + [t]:
            reader.join(timeout=5)
    finally:
        for entry in out.values():
            if entry["proc"].poll() is None:
                entry["proc"].kill()
        if relay:
            relay.kill()
    results = []
    for name, entry in out.items():
        line = next((l for l in entry["lines"] if "COOP RESULT" in l), None)
        results.append("%s: %s" % (name, line or "no result; last output: " + " | ".join(entry["lines"][-3:])))
    return "\n".join(results)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--swf", required=True)
    ap.add_argument("--mode", choices=["relay", "direct", "both"], default="both")
    ap.add_argument("--frames", type=int, default=4500, help="4500 frames = 3 minutes at 25 fps")
    ap.add_argument("--lag", type=float, default=40, help="relay: one-way delay, ms")
    ap.add_argument("--jitter", type=float, default=10, help="relay: extra random delay 0..N ms")
    ap.add_argument("--delay", type=int, help="input delay, frames (default: the host picks it from the ping)")
    ap.add_argument("--players", type=int, default=2, help="2-4 (3-4 need keys for that many, e.g. gun-mayhem-2)")
    ap.add_argument("--keys", default="bad-ice-cream", help="--coop-keys for every player")
    ap.add_argument("--script", default=SCRIPT, help="input script (default: Bad Ice-Cream's)")
    ap.add_argument("--exe", default=RUFFLE, help="ruffle_desktop.exe to test (default: target/release)")
    args = ap.parse_args()
    for mode in (["relay", "direct"] if args.mode == "both" else [args.mode]):
        extra = " (lag %g ms one way + 0-%g ms jitter)" % (args.lag, args.jitter) if mode == "relay" else ""
        print("== %s, %d players, %d frames, input delay %s%s" % (mode, args.players, args.frames,
              args.delay if args.delay is not None else "auto", extra))
        print(run(args, mode))
        sys.stdout.flush()


if __name__ == "__main__":
    main()
