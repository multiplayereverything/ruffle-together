# Modding a game you didn't write

Most Flash games worth playing together are someone else's SWF: no source code, sometimes obfuscated. You can still
make them co-op. You read the game's code with a decompiler, find the few places that matter, and add your own code
next to the game's without rewriting it. This page walks through how.

:::{tip}
Try a key map first. If the game already has two players on one keyboard, a {doc}`key map <key-maps>` is all you
need and the SWF doesn't change. Everything below is for games that need code added.
:::

## What you need

- **[JPEXS FFDec](https://github.com/jindrapetrik/jpexs-decompiler)** (free), the Flash decompiler. Its command line
  tool, `ffdec-cli.jar`, exports and recompiles scripts. You need Java 21 to run it.
- **The original SWF**, untouched. Keep it: every build starts from it, so you never edit the game by hand.
- **Python 3.10+** for the build scripts.
- The build scripts and FFDec helpers from the
  [Flash Multiplayer Skill](https://github.com/multiplayereverything/flash-multiplayer-skill) (`templates/build/`
  and `tools/ffdec/`). They do the fiddly parts below and check the result.

## 1. Read the game

Find out what you're dealing with:

- **AS2 or AS3?** FFDec shows it when you open the SWF: AS3 games have classes (`DoABC` tags), AS2 games have frame
  scripts and clip events (`DoAction` tags). The way you add code differs between them.
- **The SWF version.** SWF 6 and 7 hide `ExternalInterface` and ignore letter case in names, which matters later.
- **Obfuscated?** Names like `§_a§` or `_-3c` mean the code was scrambled. It still works, but some ways of adding
  code are ruled out (see AS3 below).

Then export every script so you can read and search them:

```bash
java -jar ffdec-cli.jar -export script decompiled/ game.swf
```

Keep `decompiled/` to yourself: it's the game's code, so it never goes into your mod's source or anywhere public.

## 2. Find the places that matter

You don't need to understand the whole game. For co-op you need the **one** place each of these goes through:

| What | Look for | Why |
|---|---|---|
| **Input** | `Key.isDown`, key listeners, `on(keyPress ...)`, `KeyboardEvent` | Each extra player needs their own input. |
| **The player** | `_root.hero`, a class with one instance, everything that reads "the player" | Enemies, pickups, doors and the camera must know about every player. |
| **The frame loop** | `onEnterFrame`, `ENTER_FRAME`, the main update function | Where you move each player's hero. |
| **The camera** | Whatever scrolls the level after the player | Replace it with one {doc}`camera <../api/picture>` per player. |
| **Menus** | What starts the game: mouse, keys, `keyPress` buttons | So test scripts can get through them. |

Write what you find in a short `CODE_MAP.md` (system, where, notes). Then **prove** each one: put a `trace` there and
run the game. Reading the code isn't enough; games often have functions that are never called.

You don't need to fix randomness or timers: Ruffle Together already makes `Math.random`, the clock and timers the same
on every PC.

## 3. Write your code

Your code reads every player's input through the {doc}`co-op API <../api/index>` (most easily the
{doc}`Coop library <../api/library>`) and moves one hero per player, the same way the game moves its one hero. The
{doc}`first co-op game <first-game>` page shows the pattern. Two rules:

- **Wrap, don't rewrite.** Replace a game function with one that calls the original and then does yours. If the game
  defines the function again later (on a later frame, or every frame), wrap it again each frame.
- **Outside Ruffle Together, change nothing.** Check `Coop.active()` (or `coop.players > 1`) first, so the SWF still
  plays exactly like the original in Flash Player.

## 4. Put your code into the SWF

Your build script starts from the untouched SWF and produces one file, `dist/<Game>_coop.swf`. Every player needs those
exact bytes.

### AS2 games: add new script tags

AS2 mods never recompile the game's own scripts. Your code goes into **new** `DoAction` tags, next to the game's:

```bash
java -cp "<ffdec folder>/lib/*" tools/ffdec/InsertAction.java game.swf out.swf 1 coop.as start.as
```

This compiles each file into its own tag on frame 1, before the frame is shown. Your build should then check that
every original tag is still there, unchanged and in order, and that only script tags were added. Start from
`templates/build/build_as2_insert.py`, which does all of that.

**Example: Chaos Faction 2.** Its mod adds two script tags and changes nothing else. One of them, run on frame 1 after
the game's own script loads the save, gives player 2 a fighter of their own:

```actionscript
// SWF 8 and older: unhide ExternalInterface first
ASSetPropFlags(_global, "flash", 0, 4096);
ASSetPropFlags(flash.external.ExternalInterface, null, 0, 4096);
if (flash.external.ExternalInterface.available and flash.external.ExternalInterface.call("coop.players") > 1) {
   _root.cf2_coop = true;
   _root.SO.data.pArray2[0] = true;   // player 2 is on
   _root.SO.data.pArray2[8] = 4;      // as "Keyboard 2", not a computer player
}
```

Its menus need no code at all: the {doc}`key map <key-maps>` gives player 2 their own mouse on their card and makes
the player editor private.

AS2 traps:

- **Keep each file under 32 KB** and don't wrap your whole mod in one big `if`. FFDec fails with "offset larger than
  maximum allowed" when a jump is too long.
- **SWF 6:** names ignore letter case (`var n` overwrites `N`), and there's no `try/catch`.
- **Obfuscated games** whose scripts don't survive a recompile: compile your code in a scratch copy and copy the
  finished tags into the untouched SWF (`templates/build/build_as2_splice.py`).

### AS3 games: replace or add classes

AS3 code lives in classes. FFDec's `-importScript` compiles a folder of `.as` files into the SWF: new classes are
added, and a file with the same path as a game class replaces that class.

1. **Copy only the game classes you change** from `decompiled/` into your build folder, and patch them there with exact
   text replacements that must match. That way your mod's source holds only your own code.
2. **Add empty stubs for your new classes first** (`tools/ffdec/AddClasses.java`): `-importScript` silently skips
   classes that don't exist yet.
3. **Compile:** `java -jar ffdec-cli.jar -importScript stubs.swf out.swf build/as`
4. **Check namespaces** (`tools/ffdec/CheckNamespaces.java`): recompiling a class whose subclasses you didn't
   recompile breaks them at run time (error #1070). On obfuscated games, don't recompile classes that have
   subclasses; add new ones instead.

Start from `templates/build/build_as3_patches.py`. FFDec often exits successfully after a compile error, so the
scripts also fail on `SEVERE`, `Exception` or `error` in its output.

## 5. Test it

Test it the same way as any co-op game: replays that give the same result twice, then two to eight players through the
relay, then real windows. See {doc}`testing`. If the window title ever says OUT OF SYNC, the report says what differs;
the usual cause is a local answer (`localSlot`, `isHost`) reaching gameplay. See {doc}`tips`.

:::{seealso}
The [Flash Multiplayer Skill](https://github.com/multiplayereverything/flash-multiplayer-skill) has the full
playbook: the traps found in every mod so far, the build templates, and the per-game notes (`games/`). It is written
so an AI agent can follow it step by step.
:::
