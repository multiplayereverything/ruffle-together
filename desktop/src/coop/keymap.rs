//! Keys in and out. A key map says, for each player slot, which game keys the slot's buttons press
//! (as Flash key codes), and which keys a player presses on their own PC for each button. Maps are
//! small text files: built in for the games Ruffle Together ships with, or any file
//! (`--coop-keys <file>`), so a game can bring its own keys without a new build.
//!
//! ```text
//! # comment
//! shared                                    every slot presses the same keys (a hot-seat game)
//! slot 0: UP=38 DOWN=40 LEFT=37 RIGHT=39 FIRE=90 ALT=88
//! slot 1: UP=87 DOWN=83 LEFT=65 RIGHT=68 FIRE=84 ALT=89
//! local: UP=38,87 DOWN=40,83 FIRE=90,84     keys a player presses on their own PC, per button
//! mouse: buttons                            friends' mice: host (off, the default), zones (only in
//!                                           their own zone), buttons (on what the game made
//!                                           clickable), all
//! mouse 2: all                              one player's mouse (overrides `mouse:` for them)
//! zone 1: _root.players.p2                  only player 2 may click the buttons in this clip (only
//!                                           while it is on show; the most exact clip wins)
//! zone 1: 290,95 200x190                    or in this box (stage pixels, on every screen)
//! private: _root.pcb                        a pop-up only the player who opened it sees and clicks;
//!   opened-by _root.players.*.instance*     ... and nobody else opens it while it is theirs
//! hover: own                                each screen draws Button symbols as its own player's
//!                                           mouse sees them (the default); shared: as the game has them
//! cursors: auto                             friends' cursors: where they may click (the default), on, off
//! ```

use crate::coop::lockstep::{MAX_PLAYERS, button_named};
use ruffle_core::PlayerEvent;
use ruffle_core::events::{KeyDescriptor, KeyLocation, LogicalKey, NamedKey, PhysicalKey};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

/// The key maps built into this build.
const BUILT_IN: &[(&str, &str)] = &[
    ("bad-ice-cream", include_str!("keys/bad-ice-cream.keys")),
    ("gun-mayhem-2", include_str!("keys/gun-mayhem-2.keys")),
    ("territory-war", include_str!("keys/territory-war.keys")),
    ("bow-man", include_str!("keys/bow-man.keys")),
    ("smileys-war", include_str!("keys/smileys-war.keys")),
    ("stealth-assassin", include_str!("keys/stealth-assassin.keys")),
    ("stealth-hunter-2", include_str!("keys/stealth-hunter-2.keys")),
    ("intrusion", include_str!("keys/intrusion.keys")),
    ("intrusion-2", include_str!("keys/intrusion-2.keys")),
    ("air-battle", include_str!("keys/air-battle.keys")),
    ("ultimate-crab-battle", include_str!("keys/ultimate-crab-battle.keys")),
    ("fireboy-watergirl", include_str!("keys/fireboy-watergirl.keys")),
    ("chaos-faction-2", include_str!("keys/chaos-faction-2.keys")),
];

/// A game's key map (`--coop-keys`).
#[derive(Clone, Debug, PartialEq)]
pub struct Game {
    name: String,
    /// What every PC compares in the handshake: the name and a hash of the map's text.
    id: String,
    shared: bool,
    /// Per slot: (button, key code), in the file's order.
    slots: Vec<Vec<(u16, u32)>>,
    /// (key code a player presses, button).
    local: Vec<(u32, u16)>,
    /// The map's `mouse:` line, if it has one.
    mouse: Option<MouseMode>,
    zones: Vec<Zone>,
    /// Clips that are private screens (`private:`), with the buttons that open them.
    privates: Vec<(String, Option<String>)>,
    hover: Option<Hover>,
    /// `mouse N:` lines.
    mouse_modes: Vec<(u8, MouseMode)>,
    cursors: Option<Cursors>,
}

/// How each screen draws Button symbols while friends have mice (`hover:`, or `coop.hover`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Hover {
    /// As this PC's player's mouse sees them, in the game's own Up/Over/Down art (the picture
    /// only).
    #[default]
    Own,
    /// As the game has them: every player's hover on every screen.
    Shared,
}

impl Hover {
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "own" => Some(Self::Own),
            // `host` was its name before every player had a mouse
            "shared" | "host" => Some(Self::Shared),
            _ => None,
        }
    }
}

/// Which friends' cursors each screen shows (`cursors:`, or `coop.mouse.cursors`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Cursors {
    /// Only while one is over something that player may click.
    #[default]
    Auto,
    On,
    Off,
}

impl Cursors {
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "auto" => Some(Self::Auto),
            "on" => Some(Self::On),
            "off" => Some(Self::Off),
            _ => None,
        }
    }
}

/// Whether the clip path `target` fits `pattern`: the same number of names, each the same, or
/// `*` (any name), or ending in `*` (any name starting so): `_root.players.*.instance*`.
pub fn path_fits(target: &str, pattern: &str) -> bool {
    let (mut target, mut pattern) = (target.split('.'), pattern.split('.'));
    loop {
        match (target.next(), pattern.next()) {
            (None, None) => return true,
            (Some(name), Some(want)) => {
                let fits = match want.strip_suffix('*') {
                    Some(start) => name.starts_with(start),
                    None => name == want,
                };
                if !fits {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

/// Where a friend's mouse is in the game (`mouse:` in a key map, or the game's `coop.mouse`).
/// Elsewhere it is outside the stage: it neither hovers nor clicks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MouseMode {
    /// Nowhere: only the host's mouse is in the game (every PC still knows every player's mouse).
    #[default]
    Host,
    /// On something clickable inside their own zone: their own card on a character screen. The
    /// host's mouse is everywhere but other players' zones.
    Zones,
    /// On something the game made clickable (what shows the hand cursor).
    Buttons,
    /// Anywhere.
    All,
}

impl MouseMode {
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "host" => Some(Self::Host),
            "zones" => Some(Self::Zones),
            "buttons" => Some(Self::Buttons),
            "all" => Some(Self::All),
            _ => None,
        }
    }
}

/// Something only one player may click: the buttons inside a clip (a player's card; only while
/// it is on show), or a box on the stage (on every screen).
#[derive(Clone, Debug, PartialEq)]
pub struct Zone {
    pub slot: u8,
    pub area: ZoneArea,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ZoneArea {
    /// `_root.players.p2`: what is clicked is this clip or inside it.
    Clip(String),
    /// Stage pixels.
    Box { x: f64, y: f64, width: f64, height: f64 },
}

impl Zone {
    /// How exactly the zone covers a click at (x, y) (twips) on `target` (the path of what is
    /// clicked, if anything): None if it doesn't; a deeper clip covers more exactly than a
    /// shallower one, and any clip more exactly than a box.
    fn covers(&self, x: i32, y: i32, target: Option<&str>) -> Option<usize> {
        match &self.area {
            ZoneArea::Clip(path) => target
                .filter(|target| {
                    *target == path || target.strip_prefix(path.as_str()).is_some_and(|rest| rest.starts_with('.'))
                })
                .map(|_| 1 + path.split('.').count()),
            ZoneArea::Box { x: bx, y: by, width, height } => {
                let (x, y) = (x as f64 / 20.0, y as f64 / 20.0);
                (x >= *bx && y >= *by && x < bx + width && y < by + height).then_some(0)
            }
        }
    }

    /// Who a click belongs to: the players of the most exact zones that cover it (none: it
    /// belongs to nobody in particular).
    fn owners(zones: &[Zone], x: i32, y: i32, target: Option<&str>) -> Vec<u8> {
        let covering: Vec<(usize, u8)> =
            zones.iter().filter_map(|zone| Some((zone.covers(x, y, target)?, zone.slot))).collect();
        let best = covering.iter().map(|(exact, _)| *exact).max();
        covering.into_iter().filter(|(exact, _)| Some(*exact) == best).map(|(_, slot)| slot).collect()
    }

    /// May `slot` click there? Not where it belongs to someone else.
    pub fn allows(zones: &[Zone], slot: u8, x: i32, y: i32, target: Option<&str>) -> bool {
        let owners = Self::owners(zones, x, y, target);
        owners.is_empty() || owners.contains(&slot)
    }

    /// Is it `slot`'s own?
    pub fn is_own(zones: &[Zone], slot: u8, x: i32, y: i32, target: Option<&str>) -> bool {
        Self::owners(zones, x, y, target).contains(&slot)
    }

    /// `X,Y WxH` (stage pixels) or a clip path (`_root.players.p2`).
    pub fn parse(slot: u8, text: &str) -> Option<Self> {
        let text = text.trim();
        let area = if text.starts_with(|c: char| c.is_ascii_digit() || c == '-') {
            let (at, size) = text.split_once(' ')?;
            let (x, y) = at.split_once(',')?;
            let (width, height) = size.trim().split_once('x')?;
            ZoneArea::Box { x: x.parse().ok()?, y: y.parse().ok()?, width: width.parse().ok()?, height: height.parse().ok()? }
        } else if !text.is_empty() && !text.contains(char::is_whitespace) {
            ZoneArea::Clip(text.to_string())
        } else {
            return None;
        };
        Some(Self { slot, area })
    }
}

impl Game {
    pub fn built_in_names() -> Vec<&'static str> {
        BUILT_IN.iter().map(|(name, _)| *name).collect()
    }

    pub fn built_in(name: &str) -> Option<Self> {
        let (name, text) = BUILT_IN.iter().find(|(n, _)| *n == name)?;
        Some(Self::parse(name, text).expect("built-in key maps parse"))
    }

    /// A built-in name, or the path of a key map file.
    pub fn load(spec: &str) -> Result<Self, String> {
        if let Some(game) = Self::built_in(spec) {
            return Ok(game);
        }
        let path = std::path::Path::new(spec);
        let text = std::fs::read_to_string(path).map_err(|_| {
            format!("no keys for \"{spec}\": not a file, and not one of {}", Self::built_in_names().join(", "))
        })?;
        let name = path.file_stem().map_or(spec.into(), |s| s.to_string_lossy().into_owned());
        Self::parse(&name, &text).map_err(|e| format!("{spec}: {e}"))
    }

    pub fn parse(name: &str, text: &str) -> Result<Self, String> {
        let mut game = Game {
            name: name.to_string(),
            id: format!("{name}#{:.12x}", Sha256::digest(text.replace('\r', "").as_bytes())),
            shared: false,
            slots: vec![],
            local: vec![],
            mouse: None,
            zones: vec![],
            privates: vec![],
            hover: None,
            mouse_modes: vec![],
            cursors: None,
        };
        for (number, line) in text.lines().enumerate() {
            let line = line.split('#').next().unwrap_or_default().trim();
            let fail = |why: String| format!("line {}: {why}", number + 1);
            let pairs = |list: &str| -> Result<Vec<(u16, Vec<u32>)>, String> {
                list.split_whitespace()
                    .map(|pair| {
                        let (button, codes) = pair.split_once('=').ok_or_else(|| fail(format!("expected BUTTON=code: {pair}")))?;
                        let button = button_named(button).ok_or_else(|| fail(format!("unknown button {button}")))?;
                        let codes = codes
                            .split(',')
                            .map(|c| {
                                c.parse::<u32>()
                                    .ok()
                                    .filter(|&c| key_descriptor(c).is_some())
                                    .ok_or_else(|| fail(format!("no key with code {c}")))
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        Ok((button, codes))
                    })
                    .collect()
            };
            if line.is_empty() {
                continue;
            } else if line == "shared" {
                game.shared = true;
            } else if let Some(mode) = line.strip_prefix("mouse:") {
                game.mouse = MouseMode::named(mode.trim()).map(Some).ok_or_else(|| fail(format!("mouse is host, zones, buttons or all: {mode}")))?;
            } else if let Some(rest) = line.strip_prefix("mouse ") {
                let one = (|| {
                    let (slot, mode) = rest.split_once(':')?;
                    let slot = slot.trim().parse::<u8>().ok().filter(|&s| s > 0 && (s as usize) < MAX_PLAYERS)?;
                    Some((slot, MouseMode::named(mode.trim())?))
                })()
                .ok_or_else(|| fail(format!("expected mouse N: host, zones, buttons or all (N is 1 to {})", MAX_PLAYERS - 1)))?;
                game.mouse_modes.push(one);
            } else if let Some(cursors) = line.strip_prefix("cursors:") {
                game.cursors = Some(Cursors::named(cursors.trim()).ok_or_else(|| fail(format!("cursors is auto, on or off: {cursors}")))?);
            } else if let Some(rest) = line.strip_prefix("zone ") {
                let zone = (|| {
                    let (slot, rest) = rest.split_once(':')?;
                    let slot = slot.trim().parse::<u8>().ok().filter(|&s| (s as usize) < MAX_PLAYERS)?;
                    Zone::parse(slot, rest)
                })()
                .ok_or_else(|| fail("expected zone N: X,Y WxH or zone N: _root.clip".into()))?;
                game.zones.push(zone);
            } else if let Some(rest) = line.strip_prefix("private:") {
                let (path, openers) = match rest.split_once(" opened-by ") {
                    Some((path, openers)) => (path.trim(), Some(openers.trim())),
                    None => (rest.trim(), None),
                };
                let one_name = |text: &str| !text.is_empty() && !text.contains(char::is_whitespace);
                if !one_name(path) || openers.is_some_and(|o| !one_name(o)) {
                    return Err(fail("expected private: _root.clip (opened-by _root.pattern)".into()));
                }
                game.privates.push((path.to_string(), openers.map(str::to_string)));
            } else if let Some(hover) = line.strip_prefix("hover:") {
                game.hover = Some(Hover::named(hover.trim()).ok_or_else(|| fail(format!("hover is own or shared: {hover}")))?);
            } else if let Some(list) = line.strip_prefix("local:") {
                for (button, codes) in pairs(list)? {
                    game.local.extend(codes.into_iter().map(|code| (code, button)));
                }
            } else if let Some(rest) = line.strip_prefix("slot ") {
                let (slot, list) = rest.split_once(':').ok_or_else(|| fail("expected slot N: ...".into()))?;
                if slot.trim().parse::<usize>().ok() != Some(game.slots.len()) || game.slots.len() >= MAX_PLAYERS {
                    return Err(fail(format!("slots go 0, 1, 2, 3 in order (got {slot})")));
                }
                let keys = pairs(list)?
                    .into_iter()
                    .map(|(button, codes)| match codes[..] {
                        [code] => Ok((button, code)),
                        _ => Err(fail("a slot button presses one key".into())),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                game.slots.push(keys);
            } else {
                return Err(fail(format!("unknown line: {line}")));
            }
        }
        if game.slots.is_empty() {
            return Err("no slots".into());
        }
        Ok(game)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// The name and a hash of the map: every player must have the same.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// How many players the game has keys for.
    pub fn max_players(&self) -> u8 {
        self.slots.len() as u8
    }

    /// The keys each player slot presses in the game.
    pub fn keys(&self) -> GameKeys {
        let slots: Vec<Vec<GameKey>> = self
            .slots
            .iter()
            .map(|slot| {
                slot.iter()
                    .map(|&(button, code)| GameKey { button, key: key_descriptor(code).expect("checked when parsed") })
                    .collect()
            })
            .collect();
        GameKeys {
            held: vec![0; slots.len()],
            slots,
            shared: self.shared,
            mouse: self.mouse,
            zones: self.zones.clone(),
            privates: self.privates.clone(),
            hover: self.hover,
            mouse_modes: self.mouse_modes.clone(),
            cursors: self.cursors,
        }
    }

    /// This PC's buttons from the keys held right now.
    /// Whether this PC's keys include the key with this Flash key code.
    pub fn has_local_code(&self, code: u32) -> bool {
        self.local.iter().any(|(local, _)| *local == code)
    }

    pub fn local_buttons(&self, held: &HashSet<PhysicalKey>) -> u16 {
        let mut buttons = 0;
        for &key in held {
            let pressed = KeyDescriptor { physical_key: key, logical_key: LogicalKey::Unknown, key_location: KeyLocation::Standard };
            if let (Some(code), _) = ruffle_core::coop_key_codes(pressed) {
                buttons |= self.local.iter().filter(|(c, _)| *c == code).fold(0, |all, (_, b)| all | b);
            }
        }
        buttons
    }
}

/// The key a game sees for a Flash key code: the physical key and the logical key both map to it.
fn key_descriptor(code: u32) -> Option<KeyDescriptor> {
    use PhysicalKey as P;
    const LETTERS: [PhysicalKey; 26] = [
        P::KeyA, P::KeyB, P::KeyC, P::KeyD, P::KeyE, P::KeyF, P::KeyG, P::KeyH, P::KeyI, P::KeyJ, P::KeyK, P::KeyL, P::KeyM,
        P::KeyN, P::KeyO, P::KeyP, P::KeyQ, P::KeyR, P::KeyS, P::KeyT, P::KeyU, P::KeyV, P::KeyW, P::KeyX, P::KeyY, P::KeyZ,
    ];
    const DIGITS: [PhysicalKey; 10] =
        [P::Digit0, P::Digit1, P::Digit2, P::Digit3, P::Digit4, P::Digit5, P::Digit6, P::Digit7, P::Digit8, P::Digit9];
    const NUMPAD: [PhysicalKey; 10] =
        [P::Numpad0, P::Numpad1, P::Numpad2, P::Numpad3, P::Numpad4, P::Numpad5, P::Numpad6, P::Numpad7, P::Numpad8, P::Numpad9];
    let character = |physical_key, ch| KeyDescriptor { physical_key, logical_key: LogicalKey::Character(ch), key_location: KeyLocation::Standard };
    let numpad = |physical_key, ch| KeyDescriptor { physical_key, logical_key: LogicalKey::Character(ch), key_location: KeyLocation::Numpad };
    let named = |physical_key, named| KeyDescriptor { physical_key, logical_key: LogicalKey::Named(named), key_location: KeyLocation::Standard };
    let digit = |n: u32| char::from_digit(n, 10).unwrap_or('0');
    Some(match code {
        65..=90 => character(LETTERS[(code - 65) as usize], (b'a' + (code - 65) as u8) as char),
        48..=57 => character(DIGITS[(code - 48) as usize], digit(code - 48)),
        96..=105 => numpad(NUMPAD[(code - 96) as usize], digit(code - 96)),
        106 => numpad(P::NumpadMultiply, '*'),
        107 => numpad(P::NumpadAdd, '+'),
        109 => numpad(P::NumpadSubtract, '-'),
        110 => numpad(P::NumpadDecimal, '.'),
        111 => numpad(P::NumpadDivide, '/'),
        32 => character(P::Space, ' '),
        37 => named(P::ArrowLeft, NamedKey::ArrowLeft),
        38 => named(P::ArrowUp, NamedKey::ArrowUp),
        39 => named(P::ArrowRight, NamedKey::ArrowRight),
        40 => named(P::ArrowDown, NamedKey::ArrowDown),
        8 => named(P::Backspace, NamedKey::Backspace),
        9 => named(P::Tab, NamedKey::Tab),
        13 => named(P::Enter, NamedKey::Enter),
        16 => named(P::ShiftLeft, NamedKey::Shift),
        17 => named(P::ControlLeft, NamedKey::Control),
        18 => named(P::AltLeft, NamedKey::Alt),
        27 => named(P::Escape, NamedKey::Escape),
        33 => named(P::PageUp, NamedKey::PageUp),
        34 => named(P::PageDown, NamedKey::PageDown),
        35 => named(P::End, NamedKey::End),
        36 => named(P::Home, NamedKey::Home),
        45 => named(P::Insert, NamedKey::Insert),
        46 => named(P::Delete, NamedKey::Delete),
        186 => character(P::Semicolon, ';'),
        187 => character(P::Equal, '='),
        188 => character(P::Comma, ','),
        189 => character(P::Minus, '-'),
        190 => character(P::Period, '.'),
        191 => character(P::Slash, '/'),
        192 => character(P::Backquote, '`'),
        219 => character(P::BracketLeft, '['),
        220 => character(P::Backslash, '\\'),
        221 => character(P::BracketRight, ']'),
        222 => character(P::Quote, '\''),
        _ => return None,
    })
}

#[derive(Clone, Copy)]
struct GameKey {
    button: u16,
    key: KeyDescriptor,
}

/// The game's own keys for each player slot, and what each slot holds now.
pub struct GameKeys {
    slots: Vec<Vec<GameKey>>,
    held: Vec<u16>,
    /// Every slot presses the same keys (a hot-seat game): a key is down while any player holds it.
    shared: bool,
    /// Where friends' mice are in the game, and the clips and boxes only one player may click
    /// (the map's `mouse:`, `mouse N:` and `zone`; nothing when it says nothing, so a game's own
    /// `coop.keys` map keeps the rules).
    pub mouse: Option<MouseMode>,
    pub mouse_modes: Vec<(u8, MouseMode)>,
    pub zones: Vec<Zone>,
    /// The map's private screens (`private:`), with the buttons that open them.
    pub privates: Vec<(String, Option<String>)>,
    pub hover: Option<Hover>,
    pub cursors: Option<Cursors>,
}

impl GameKeys {
    /// Key presses and releases that turn `slot`'s held buttons into `buttons`.
    pub fn events(&mut self, slot: u8, buttons: u16) -> Vec<PlayerEvent> {
        let Some(keys) = self.slots.get(slot as usize) else {
            return vec![];
        };
        let union = |held: &[u16]| held.iter().fold(0, |all, b| all | b);
        let before_all = union(&self.held);
        let before = std::mem::replace(&mut self.held[slot as usize], buttons);
        // shared keys: what changes is the OR of every player's buttons
        let (before, buttons) = if self.shared {
            (before_all, union(&self.held))
        } else {
            (before, buttons)
        };
        let mut events = vec![];
        for game_key in keys {
            let (was, is) = (before & game_key.button != 0, buttons & game_key.button != 0);
            if is && !was {
                events.push(PlayerEvent::KeyDown { key: game_key.key });
                // a real keyboard also types the character: on(keyPress "a") hears only that
                if let LogicalKey::Character(codepoint) = game_key.key.logical_key {
                    events.push(PlayerEvent::TextInput { codepoint });
                }
            } else if was && !is {
                events.push(PlayerEvent::KeyUp { key: game_key.key });
            }
        }
        events
    }

    /// Releases everything held, for every slot.
    pub fn release_all(&mut self) -> Vec<PlayerEvent> {
        (0..self.slots.len() as u8)
            .flat_map(|slot| self.events(slot, 0))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `mouse:` and `zone` lines: who may click, and boxes only one player may click.
    #[test]
    fn mouse_rules_and_zones() {
        let game = Game::parse("t", "slot 0: FIRE=32
mouse: buttons
zone 1: 290,95 200x190").expect("map");
        let keys = game.keys();
        assert_eq!(keys.mouse, Some(MouseMode::Buttons));
        let zone = Zone { slot: 1, area: ZoneArea::Box { x: 290.0, y: 95.0, width: 200.0, height: 190.0 } };
        assert_eq!(keys.zones, [zone]);
        let px = |p: i32| p * 20;
        assert!(Zone::allows(&keys.zones, 1, px(300), px(100), None), "its owner");
        assert!(!Zone::allows(&keys.zones, 0, px(300), px(100), None), "anyone else");
        assert!(Zone::allows(&keys.zones, 0, px(10), px(10), None), "outside every box");
        // clips: only what is clicked inside them, the most exact clip wins
        let clips = Game::parse("t", "slot 0: FIRE=32\nzone 1: _root.players.p2\nzone 0: _root.players.p2.combo")
            .expect("map")
            .keys()
            .zones;
        assert!(Zone::is_own(&clips, 1, 0, 0, Some("_root.players.p2.b10")));
        assert!(!Zone::allows(&clips, 0, 0, 0, Some("_root.players.p2.b10")), "the host stays off it");
        assert!(Zone::is_own(&clips, 0, 0, 0, Some("_root.players.p2.combo.instance614")), "the drop-down is the host's");
        assert!(!Zone::allows(&clips, 1, 0, 0, Some("_root.players.p2.combo")));
        assert!(Zone::allows(&clips, 0, 0, 0, Some("_root.players.p20")), "not a clip inside it");
        assert!(Zone::allows(&clips, 0, 0, 0, Some("_root.startbutton")) && !Zone::is_own(&clips, 1, 0, 0, Some("_root.startbutton")));
        assert!(Zone::allows(&clips, 0, 0, 0, None), "nothing clicked there");
        let private = Game::parse("t", "slot 0: FIRE=32\nprivate: _root.pcb\nhover: shared").expect("map").keys();
        assert_eq!(private.privates, [("_root.pcb".to_string(), None)]);
        assert_eq!(private.hover, Some(Hover::Shared));
        let one = Game::parse("t", "slot 0: FIRE=32\nmouse 2: all\ncursors: off\nhover: own").expect("map").keys();
        assert_eq!(one.mouse_modes, [(2, MouseMode::All)]);
        assert_eq!((one.cursors, one.hover), (Some(Cursors::Off), Some(Hover::Own)));
        assert!(Game::parse("t", "slot 0: FIRE=32\nmouse 0: all").is_err(), "the host's mouse is always on");
        assert!(Game::parse("t", "slot 0: FIRE=32\ncursors: some").is_err());
        let opened = Game::parse("t", "slot 0: FIRE=32\nprivate: _root.pcb opened-by _root.players.*.instance*").expect("map");
        assert_eq!(opened.keys().privates, [("_root.pcb".to_string(), Some("_root.players.*.instance*".to_string()))]);
        assert!(Game::parse("t", "slot 0: FIRE=32\nprivate:").is_err());
        assert!(Game::parse("t", "slot 0: FIRE=32\nhover: maybe").is_err());
        assert!(path_fits("_root.players.p1.instance189", "_root.players.*.instance*"));
        assert!(!path_fits("_root.players.p1.b9", "_root.players.*.instance*"), "an arrow");
        assert!(!path_fits("_root.players.p2.combo.instance614", "_root.players.*.instance*"), "deeper");
        assert!(!path_fits("_root.pcb.instance244", "_root.players.*.instance*"));
        assert_eq!(Game::parse("t", "slot 0: FIRE=32").expect("map").keys().mouse, None);
        assert!(Game::parse("t", "slot 0: FIRE=32
mouse: some").is_err());
        assert!(Game::parse("t", "slot 0: FIRE=32
zone 9: 1,2 3x4").is_err());
    }
    use crate::coop::lockstep::{ALT, DOWN, FIRE, KEY1, KEY2, KEY3, KEY4, LEFT, MENU, NO, QUIT, RIGHT, UP, VIEW, YES, button_named};

    fn game(name: &str) -> Game {
        Game::built_in(name).expect("built in")
    }

    fn held(keys: &[PhysicalKey]) -> HashSet<PhysicalKey> {
        keys.iter().copied().collect()
    }

    fn down_keys(events: &[PlayerEvent]) -> Vec<PhysicalKey> {
        events
            .iter()
            .filter_map(|e| match e {
                PlayerEvent::KeyDown { key } => Some(key.physical_key),
                _ => None,
            })
            .collect()
    }

    fn up_keys(events: &[PlayerEvent]) -> Vec<PhysicalKey> {
        events
            .iter()
            .filter_map(|e| match e {
                PlayerEvent::KeyUp { key } => Some(key.physical_key),
                _ => None,
            })
            .collect()
    }

    /// Every key code a map can name reaches the game as that code, by Ruffle's physical mapping
    /// and by its logical one (the default), and every built-in map parses.
    #[test]
    fn key_codes_round_trip_and_built_in_maps_parse() {
        let mut known = 0;
        for code in 0..=255 {
            if let Some(key) = key_descriptor(code) {
                assert_eq!(ruffle_core::coop_key_codes(key), (Some(code), Some(code)), "code {code}");
                known += 1;
            }
        }
        assert_eq!(known, 80, "letters, digits, numpad, arrows, named keys and punctuation");
        for name in Game::built_in_names() {
            assert!(Game::built_in(name).is_some(), "{name}");
        }
    }

    #[test]
    fn bad_key_map_files_say_what_is_wrong() {
        let err = |text: &str| Game::parse("t", text).err().expect("an error");
        assert!(err("slot 0: UP=38 JUMP=32").contains("unknown button JUMP"));
        assert!(err("slot 0: UP=999").contains("no key with code 999"));
        assert!(err("slot 1: UP=38").contains("slots go 0, 1, 2, 3"));
        assert!(err("slot 0: UP=38,87").contains("one key"));
        assert!(err("players 2\nslot 0: UP=38").contains("line 1: unknown line"));
        assert_eq!(err("# nothing"), "no slots");
        let a = Game::parse("t", "slot 0: UP=38").expect("parses");
        let b = Game::parse("t", "slot 0: UP=40").expect("parses");
        assert_ne!(a.id(), b.id(), "a different map is refused in the handshake");
    }

    #[test]
    fn a_key_map_file_can_be_loaded() {
        let dir = std::env::temp_dir().join(format!("coop-keys-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("my-game.keys");
        std::fs::write(&path, "slot 0: FIRE=32\r\nslot 1: FIRE=13\r\nlocal: FIRE=32\r\n").expect("write");
        let game = Game::load(&path.to_string_lossy()).expect("loads");
        assert_eq!((game.name(), game.max_players()), ("my-game", 2));
        assert_eq!(game.local_buttons(&held(&[PhysicalKey::Space])), FIRE);
        assert!(Game::load("pong").err().expect("an error").contains("gun-mayhem-2"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn wasd_and_arrows_both_move() {
        let bic = game("bad-ice-cream");
        assert_eq!(bic.local_buttons(&held(&[PhysicalKey::KeyW])), UP);
        assert_eq!(bic.local_buttons(&held(&[PhysicalKey::ArrowUp])), UP);
        assert_eq!(bic.local_buttons(&held(&[PhysicalKey::KeyA, PhysicalKey::ArrowDown])), LEFT | DOWN);
        assert_eq!(bic.local_buttons(&held(&[PhysicalKey::KeyD])), RIGHT);
        for fire in [PhysicalKey::Space, PhysicalKey::Enter, PhysicalKey::KeyF] {
            assert_eq!(bic.local_buttons(&held(&[fire])), FIRE);
        }
        assert_eq!(bic.local_buttons(&held(&[PhysicalKey::KeyQ])), 0);
    }

    /// Gun Mayhem 2's own defaults (savedata2.controlarray): player 1 arrows + Z shoot + X
    /// grenade, player 2 W A S D + T shoot + Y grenade; a player may use either set on their PC.
    #[test]
    fn gun_mayhem_2_keys() {
        let gm2 = game("gun-mayhem-2");
        assert_eq!(gm2.local_buttons(&held(&[PhysicalKey::ArrowLeft])), LEFT);
        assert_eq!(gm2.local_buttons(&held(&[PhysicalKey::KeyW])), UP);
        for (key, button) in [
            (PhysicalKey::KeyZ, FIRE),
            (PhysicalKey::KeyT, FIRE),
            (PhysicalKey::KeyX, ALT),
            (PhysicalKey::KeyY, ALT),
            (PhysicalKey::Space, 0),
        ] {
            assert_eq!(gm2.local_buttons(&held(&[key])), button, "{key:?}");
        }
        let mut keys = gm2.keys();
        let mut down = down_keys(&keys.events(0, UP | FIRE | ALT));
        down.sort_by_key(|k| *k as u32);
        let mut expected = vec![PhysicalKey::ArrowUp, PhysicalKey::KeyZ, PhysicalKey::KeyX];
        expected.sort_by_key(|k| *k as u32);
        assert_eq!(down, expected);
        let events = keys.events(1, LEFT | FIRE | ALT);
        let mut down = down_keys(&events);
        down.sort_by_key(|k| *k as u32);
        let mut expected = vec![PhysicalKey::KeyA, PhysicalKey::KeyT, PhysicalKey::KeyY];
        expected.sort_by_key(|k| *k as u32);
        assert_eq!(down, expected);
        assert!(events.iter().any(|e| matches!(e,
            PlayerEvent::KeyDown { key } if key.logical_key == LogicalKey::Character('t'))));
    }

    /// Players 3 and 4 use the numpad (GM2's defaults: 111/103/104/105 + 106/109 and
    /// 101/97/98/99 + 96/110); the game sees numpad key codes only with the numpad location.
    #[test]
    fn gun_mayhem_2_players_3_and_4_use_the_numpad() {
        let mut keys = game("gun-mayhem-2").keys();
        let pressed = |events: Vec<PlayerEvent>| -> Vec<(LogicalKey, KeyLocation)> {
            events
                .into_iter()
                .filter_map(|e| match e {
                    PlayerEvent::KeyDown { key } => Some((key.logical_key, key.key_location)),
                    _ => None,
                })
                .collect()
        };
        let numpad = |c| (LogicalKey::Character(c), KeyLocation::Numpad);
        let p3 = pressed(keys.events(2, UP | LEFT | FIRE | ALT));
        for key in [numpad('/'), numpad('7'), numpad('*'), numpad('-')] {
            assert!(p3.contains(&key), "player 3 {key:?}");
        }
        let p4 = pressed(keys.events(3, DOWN | RIGHT | FIRE | ALT));
        for key in [numpad('2'), numpad('3'), numpad('0'), numpad('.')] {
            assert!(p4.contains(&key), "player 4 {key:?}");
        }
        assert_eq!(game("gun-mayhem-2").max_players(), 4);
        assert_eq!(game("bad-ice-cream").max_players(), 2);
    }

    /// Territory War is hot-seat: every player presses the same keys, so a key is down while any
    /// player holds it and released only when nobody does.
    #[test]
    fn territory_war_keys_are_shared_by_every_player() {
        let mut keys = game("territory-war").keys();
        assert_eq!(down_keys(&keys.events(0, LEFT)), vec![PhysicalKey::ArrowLeft]);
        assert!(keys.events(1, LEFT).is_empty(), "already down");
        assert!(keys.events(0, 0).is_empty(), "player 2 still holds it");
        assert_eq!(up_keys(&keys.events(1, 0)), vec![PhysicalKey::ArrowLeft]);
        let events = keys.events(2, KEY1 | VIEW);
        let mut down = down_keys(&events);
        down.sort_by_key(|k| *k as u32);
        let mut expected = vec![PhysicalKey::Digit1, PhysicalKey::KeyV];
        expected.sort_by_key(|k| *k as u32);
        assert_eq!(down, expected);
        // the top-row digit, not the numpad one: the game reads key code 49
        assert!(events.iter().any(|e| matches!(e, PlayerEvent::KeyDown { key }
            if key.logical_key == LogicalKey::Character('1') && key.key_location == KeyLocation::Standard)));
        assert_eq!(game("territory-war").max_players(), 4);
    }

    #[test]
    fn territory_war_local_keys() {
        let tw = game("territory-war");
        for (key, button) in [
            (PhysicalKey::ArrowUp, UP),
            (PhysicalKey::Space, FIRE),
            (PhysicalKey::KeyX, ALT),
            (PhysicalKey::Digit1, KEY1),
            (PhysicalKey::Digit4, KEY4),
            (PhysicalKey::KeyV, VIEW),
            (PhysicalKey::KeyU, QUIT),
            (PhysicalKey::KeyY, YES),
            (PhysicalKey::KeyN, NO),
            (PhysicalKey::KeyW, 0),
        ] {
            assert_eq!(tw.local_buttons(&held(&[key])), button, "{key:?}");
        }
    }

    /// Bow Man: player 1 W/S + Space, player 2 I/K + Enter. The game's own arrow keys zoom, quit
    /// and clear the screen, so the map never presses an arrow.
    #[test]
    fn bow_man_keys_never_press_an_arrow() {
        let bm = game("bow-man");
        let mut keys = bm.keys();
        let mut down = down_keys(&keys.events(0, UP | DOWN | FIRE | LEFT | RIGHT | ALT));
        down.sort_by_key(|k| *k as u32);
        let mut expected = vec![PhysicalKey::KeyW, PhysicalKey::KeyS, PhysicalKey::Space];
        expected.sort_by_key(|k| *k as u32);
        assert_eq!(down, expected);
        let events = keys.events(1, UP | FIRE | LEFT);
        let mut down = down_keys(&events);
        down.sort_by_key(|k| *k as u32);
        let mut expected = vec![PhysicalKey::KeyI, PhysicalKey::Enter];
        expected.sort_by_key(|k| *k as u32);
        assert_eq!(down, expected);
        assert!(events.iter().any(|e| matches!(e,
            PlayerEvent::KeyDown { key } if key.logical_key == LogicalKey::Named(NamedKey::Enter))));
        assert_eq!(bm.max_players(), 2);
        assert_eq!(bm.local_buttons(&held(&[PhysicalKey::ArrowUp])), UP);
        assert_eq!(bm.local_buttons(&held(&[PhysicalKey::KeyS])), DOWN);
        for fire in [PhysicalKey::Space, PhysicalKey::Enter, PhysicalKey::KeyF] {
            assert_eq!(bm.local_buttons(&held(&[fire])), FIRE);
        }
    }

    /// Ultimate Crab Battle: each player has their own move + fire keys (P4 on the numpad), and
    /// every player also presses 1-3, Ctrl, Y and N.
    #[test]
    fn ultimate_crab_battle_keys() {
        let ucb = game("ultimate-crab-battle");
        assert_eq!(ucb.max_players(), 4);
        let mut keys = ucb.keys();
        for (slot, buttons, expected) in [
            (0, UP | FIRE, vec![PhysicalKey::ArrowUp, PhysicalKey::KeyA]),
            (1, LEFT | FIRE, vec![PhysicalKey::KeyJ, PhysicalKey::KeyU]),
            (2, DOWN | FIRE, vec![PhysicalKey::KeyG, PhysicalKey::KeyR]),
            (3, UP | FIRE, vec![PhysicalKey::Numpad8, PhysicalKey::Numpad0]),
        ] {
            let mut down = down_keys(&keys.events(slot, buttons));
            down.sort_by_key(|k| *k as u32);
            let mut expected = expected;
            expected.sort_by_key(|k| *k as u32);
            assert_eq!(down, expected, "slot {slot}");
        }
        for slot in 0..4 {
            let events = keys.events(slot, KEY1 | KEY2 | KEY3 | ALT | YES | NO);
            let mut down = down_keys(&events);
            down.sort_by_key(|k| *k as u32);
            let mut expected = vec![
                PhysicalKey::Digit1,
                PhysicalKey::Digit2,
                PhysicalKey::Digit3,
                PhysicalKey::ControlLeft,
                PhysicalKey::KeyY,
                PhysicalKey::KeyN,
            ];
            expected.sort_by_key(|k| *k as u32);
            assert_eq!(down, expected, "slot {slot}");
            // the top-row digit the game reads as key code 49, not the numpad one
            assert!(events.iter().any(|e| matches!(e, PlayerEvent::KeyDown { key }
                if key.logical_key == LogicalKey::Character('1') && key.key_location == KeyLocation::Standard)));
        }
        for (key, button) in [
            (PhysicalKey::ArrowLeft, LEFT),
            (PhysicalKey::KeyW, UP),
            (PhysicalKey::KeyD, RIGHT),
            (PhysicalKey::KeyA, 0),
            (PhysicalKey::Space, FIRE),
            (PhysicalKey::Enter, FIRE),
            (PhysicalKey::KeyZ, FIRE),
            (PhysicalKey::KeyF, FIRE),
            (PhysicalKey::ControlLeft, ALT),
            (PhysicalKey::Digit3, KEY3),
            (PhysicalKey::KeyY, YES),
            (PhysicalKey::KeyN, NO),
        ] {
            assert_eq!(ucb.local_buttons(&held(&[key])), button, "{key:?}");
        }
    }

    /// AS2 buttons' on(keyPress "a") hear the typed character, not the key: a slot's key press
    /// also types its game key's character, as a real keyboard does, for any map (built in or
    /// the game's own from `coop.keys`). Named keys (arrows, Enter, Ctrl) type nothing.
    #[test]
    fn a_key_press_types_its_character() {
        let typed = |events: &[PlayerEvent]| -> Vec<char> {
            events
                .iter()
                .filter_map(|e| match e {
                    PlayerEvent::TextInput { codepoint } => Some(*codepoint),
                    _ => None,
                })
                .collect()
        };
        let mut keys = game("ultimate-crab-battle").keys();
        let events = keys.events(0, KEY1);
        assert_eq!(typed(&events), vec!['1']);
        let down = events.iter().position(|e| matches!(e, PlayerEvent::KeyDown { .. }));
        let text = events.iter().position(|e| matches!(e, PlayerEvent::TextInput { .. }));
        assert!(down < text, "the key goes down before its character, as from winit");
        assert!(typed(&keys.events(0, KEY1)).is_empty(), "held: typed once");
        assert!(typed(&keys.events(0, 0)).is_empty(), "a release types nothing");
        assert!(typed(&keys.events(0, UP | ALT)).is_empty(), "arrows and Ctrl type nothing");
        // a game's own map (Dad 'n Me: every player's FIRE is A)
        let mut keys = Game::parse("game", "slot 0: LEFT=37 FIRE=65 ALT=83
slot 1: LEFT=37 FIRE=65 ALT=83").expect("map").keys();
        assert_eq!(typed(&keys.events(1, FIRE | LEFT)), vec!['a']);
        assert_eq!(typed(&keys.events(0, ALT)), vec!['s']);
        // shared keys type once, when the first player presses them
        let mut keys = game("territory-war").keys();
        assert_eq!(typed(&keys.events(0, KEY1)), vec!['1']);
        assert!(typed(&keys.events(1, KEY1)).is_empty());
    }

    /// Esc is an ordinary key: nothing unless the map gives it a button.
    #[test]
    fn esc_is_only_what_the_map_says() {
        assert_eq!(game("gun-mayhem-2").local_buttons(&held(&[PhysicalKey::Escape])), 0);
        let esc_is_fire = Game::parse("t", "slot 0: FIRE=27
local: FIRE=27").expect("map");
        assert_eq!(esc_is_fire.local_buttons(&held(&[PhysicalKey::Escape])), FIRE);
    }

    /// Stealth Hunter 2's cutscenes skip with Esc: MENU presses it, for every player.
    #[test]
    fn stealth_hunter_2_esc_skips_a_cutscene() {
        let sh2 = game("stealth-hunter-2");
        assert_eq!(sh2.local_buttons(&held(&[PhysicalKey::Escape])), MENU);
        let mut keys = sh2.keys();
        for slot in 0..4 {
            assert_eq!(down_keys(&keys.events(slot, MENU)), vec![PhysicalKey::Escape], "slot {slot}");
            keys.events(slot, 0);
        }
        assert_eq!(button_named("MENU"), Some(MENU));
    }

    #[test]
    fn fireboy_watergirl_keys() {
        let fw = game("fireboy-watergirl");
        assert_eq!(fw.max_players(), 2);
        let mut keys = fw.keys();
        let mut fireboy = down_keys(&keys.events(0, UP | LEFT | RIGHT));
        fireboy.sort_by_key(|k| *k as u32);
        let mut want = vec![PhysicalKey::ArrowUp, PhysicalKey::ArrowLeft, PhysicalKey::ArrowRight];
        want.sort_by_key(|k| *k as u32);
        assert_eq!(fireboy, want, "Fireboy is the arrows");
        let mut watergirl = down_keys(&keys.events(1, UP | LEFT | RIGHT));
        watergirl.sort_by_key(|k| *k as u32);
        let mut want = vec![PhysicalKey::KeyW, PhysicalKey::KeyA, PhysicalKey::KeyD];
        want.sort_by_key(|k| *k as u32);
        assert_eq!(watergirl, want, "Watergirl is W A D");
        assert_eq!(down_keys(&keys.events(0, MENU)), vec![PhysicalKey::KeyP], "P pauses");
        // on their own PC each player may use either key set, and Esc is the game's pause
        assert_eq!(fw.local_buttons(&held(&[PhysicalKey::KeyW])), UP);
        assert_eq!(fw.local_buttons(&held(&[PhysicalKey::ArrowLeft])), LEFT);
        assert_eq!(fw.local_buttons(&held(&[PhysicalKey::Escape])), MENU);
    }

    /// Chaos Faction 2: every player presses the game's solo keys on their own PC; P1 sends them as they are, P2
    /// as numpad codes the mod maps to player 2. Only P1 pauses.
    #[test]
    fn chaos_faction_2_keys() {
        let cf = game("chaos-faction-2");
        assert_eq!(cf.max_players(), 2);
        let mut keys = cf.keys();
        assert_eq!(down_keys(&keys.events(0, FIRE | KEY3)), vec![PhysicalKey::KeyZ, PhysicalKey::ShiftLeft]);
        assert_eq!(down_keys(&keys.events(1, FIRE | KEY3)), vec![PhysicalKey::Numpad1, PhysicalKey::Numpad0]);
        assert_eq!(down_keys(&keys.events(0, MENU)), vec![PhysicalKey::Space], "Space pauses");
        assert!(down_keys(&keys.events(1, MENU)).is_empty(), "only the host pauses");
        assert_eq!(cf.local_buttons(&held(&[PhysicalKey::KeyT])), KEY1);
        assert_eq!(cf.local_buttons(&held(&[PhysicalKey::KeyD])), KEY2);
        // the character screen: friends click their own card, the P2 drop-down is the host's
        assert_eq!(keys.mouse, Some(MouseMode::Zones));
        // what the game names its buttons (seen on the players page)
        let own = |slot: u8, target: &str| Zone::is_own(&keys.zones, slot, 0, 0, Some(target));
        assert!(own(1, "_root.players.p2.b10") && own(1, "_root.players.p2.instance613"), "P2's arrows, Edit Player");
        assert!(!own(1, "_root.players.p2.combo.instance614"), "not the Keyboard 2 drop-down");
        assert!(!own(1, "_root.players.p1.b9") && !own(1, "_root.nextbutton") && !own(1, "_root.instance108"));
        assert!(!Zone::allows(&keys.zones, 0, 0, 0, Some("_root.players.p2.b10")), "the host stays off the P2 card");
        assert!(Zone::allows(&keys.zones, 0, 0, 0, Some("_root.instance108")), "the title screen is the host's");
    }

    #[test]
    fn games_by_name() {
        for name in ["territory-war", "bad-ice-cream", "gun-mayhem-2", "bow-man", "stealth-hunter-2"] {
            assert_eq!(game(name).name(), name);
        }
        assert!(Game::built_in("pong").is_none());
    }

    #[test]
    fn slot1_maps_to_wasd() {
        let mut keys = game("bad-ice-cream").keys();
        let events = keys.events(1, UP | FIRE);
        let mut down = down_keys(&events);
        down.sort_by_key(|k| *k as u32);
        let mut expected = vec![PhysicalKey::KeyW, PhysicalKey::KeyF];
        expected.sort_by_key(|k| *k as u32);
        assert_eq!(down, expected);
        // the game reads logical keys: W must arrive as the character 'w'
        assert!(events.iter().any(|e| matches!(e,
            PlayerEvent::KeyDown { key } if key.logical_key == LogicalKey::Character('w'))));
    }

    #[test]
    fn slot0_maps_to_arrows_and_space() {
        let mut keys = game("bad-ice-cream").keys();
        let events = keys.events(0, LEFT | FIRE);
        assert!(events.iter().any(|e| matches!(e,
            PlayerEvent::KeyDown { key } if key.logical_key == LogicalKey::Named(NamedKey::ArrowLeft))));
        assert!(down_keys(&events).contains(&PhysicalKey::Space));
    }

    #[test]
    fn only_changes_emitted() {
        let mut keys = game("bad-ice-cream").keys();
        assert_eq!(keys.events(0, UP).len(), 1);
        assert!(keys.events(0, UP).is_empty());
        let events = keys.events(0, UP | RIGHT);
        assert_eq!(down_keys(&events), vec![PhysicalKey::ArrowRight]);
        assert!(up_keys(&events).is_empty());
    }

    #[test]
    fn release_on_zero() {
        let mut keys = game("bad-ice-cream").keys();
        keys.events(1, DOWN | FIRE);
        let events = keys.events(1, 0);
        assert_eq!(up_keys(&events).len(), 2);
        assert!(down_keys(&events).is_empty());
    }

    #[test]
    fn release_all_lets_go_of_every_slot() {
        let mut keys = game("bad-ice-cream").keys();
        keys.events(0, UP);
        keys.events(1, LEFT | FIRE);
        let events = keys.release_all();
        assert_eq!(up_keys(&events).len(), 3);
        assert!(keys.events(0, 0).is_empty());
    }
}
