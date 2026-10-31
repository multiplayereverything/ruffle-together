//! What a game can ask Ruffle Together, the same in AS2 and AS3, through ExternalInterface:
//!
//! ```text
//! ExternalInterface.call("coop.players")         number of players (2-4)
//! ExternalInterface.call("coop.localSlot")       this PC's player slot (0-7): camera, HUD, "you" labels ONLY
//! ExternalInterface.call("coop.mouseX", slot)    that slot's mouse, stage pixels (same space as _root._xmouse)
//! ExternalInterface.call("coop.mouseY", slot)
//! ExternalInterface.call("coop.mouseDown", slot) that slot's left button
//! ExternalInterface.call("coop.clicked", slot)   true on the frame that slot pressed its left button
//! ExternalInterface.call("coop.mouse", mode)     where friends' mice are in the game: "host" (nowhere),
//!                                                "zones" (their own zone), "buttons" (clickable things) or "all"
//! ExternalInterface.call("coop.mouse", slot, mode)  ... for one player (coop.mouse(mode) resets them all)
//! ExternalInterface.call("coop.mouse.clicker")   during a player's mouse events (onPress, onRelease, CLICK...),
//!                                                whose mouse it is; undefined otherwise
//! ExternalInterface.call("coop.mouse.hovered", slot)  the path of what that player's mouse is on, or undefined
//! ExternalInterface.call("coop.mouse.cursors", mode)  friends' cursors: "auto" (where they may click), "on", "off"
//! ExternalInterface.call("coop.zone", slot, x, y, w, h)  only that slot may click in the box; (slot) clears its boxes
//! ExternalInterface.call("coop.only", path)      a private screen: only the player who opened it sees and clicks it
//! ExternalInterface.call("coop.only", slot, path)  ... only that slot; coop.shared(path) makes it everyone's again
//! ExternalInterface.call("coop.only", path, pattern)  ... and while it is someone's, nobody else may click the
//!                                                buttons that open it (pattern: "_root.players.*.instance*")
//! ExternalInterface.call("coop.hover", mode)     "own": each screen draws Button symbols as its own player's
//!                                                mouse sees them (the default), "shared": as the game has them
//! ExternalInterface.call("coop.typed", slot)     what that slot typed this frame ("\b" Backspace, "\n" Enter)
//! ExternalInterface.call("coop.option", name)    the host's setting (--coop-option NAME=VALUE), or undefined
//! ExternalInterface.call("coop.options")         every setting, as an object
//! ExternalInterface.call("coop.ping", slot)      this PC's round trip to that player, ms (display only: differs per PC)
//! ExternalInterface.call("coop.events")          what happened since the last call: ["join 0", "leave 2", "chat 1 gg", ...]
//! ExternalInterface.call("coop.buttons", slot)   that slot's held buttons (UP 1, DOWN 2, LEFT 4, RIGHT 8, FIRE 16, ...)
//! ExternalInterface.call("coop.button", slot, name)  true while that slot holds the button ("FIRE", "KEY1", ...)
//! ExternalInterface.call("coop.version")         the API version (2)
//! ExternalInterface.call("coop.has", feature)    true if this build has the feature ("camera", "keys", ...)
//! ExternalInterface.call("coop.name", slot)      that player's name
//! ExternalInterface.call("coop.isHost")          true on the host's PC (like localSlot: never for gameplay)
//! ExternalInterface.call("coop.camera", slot, x, y, zoom)  that slot's camera; (slot) alone turns it off
//! ExternalInterface.call("coop.hud", path)       that clip is drawn without the camera
//! ExternalInterface.call("coop.label", slot, path, text)   a name tag above that clip ("" removes it)
//! ExternalInterface.call("coop.keys", text)      the game's own key map (.keys format), from the next frame
//! ```
//!
//! Every answer except `localSlot` and `isHost` is the same on every PC for the same frame (the mice
//! travel with the other inputs). The camera, HUD and labels only change the picture on each PC.
//! Outside a session there is no provider: `ExternalInterface.available` is false (or the call
//! returns null), so a game falls back to its normal mouse, slot 0 and no camera.

use crate::coop::lockstep::{InputFrame, MAX_PLAYERS};
use ruffle_core::context::UpdateContext;
use ruffle_core::external::{ExternalInterfaceProvider, Value};
use std::sync::{Arc, Mutex};

pub const VERSION: u8 = 2;
/// What `coop.has` says yes to.
pub const FEATURES: &[&str] = &[
    "players", "mouse", "name", "camera", "hud", "label", "keys", "buttons", "typed", "options", "ping", "events", "chat",
    "clicks", "zones", "private", "hover", "pointers", "clicker", "hovered", "cursors",
];

/// Events kept for a game that doesn't read them (the oldest go first).
const MAX_EVENTS: usize = 100;

/// A game that called `coop.typed` within this many frames gets every typed character.
const WANTS_TEXT_FRAMES: u32 = 60;

/// A slot's camera: the stage point (x, y), in stage pixels, shown at the centre of the screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}

/// A private screen: the clip at `path` belongs to one player while it is on show (`fixed`, or
/// whoever's click opened it): only their PC draws it, only they click it, and everyone else's
/// mouse goes through it. The game still has it on every PC; only the picture and the mouse
/// differ, the same way on every PC.
#[derive(Clone, Debug, PartialEq)]
pub struct Private {
    pub path: String,
    pub fixed: Option<u8>,
    /// The buttons that open it (a path pattern): while it is someone's, nobody else may click
    /// them (the game has one of it).
    pub openers: Option<String>,
    /// Whose it is while it is on show.
    pub owner: Option<u8>,
}

/// A name tag that Ruffle draws above a clip.
#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    pub path: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CoopApi {
    pub players: u8,
    pub local_slot: u8,
    /// Each slot's mouse (stage twips) and left button, as of the frame that runs now.
    pub mouse: [(i32, i32, bool); MAX_PLAYERS],
    /// Each slot pressed its left button this frame (`coop.clicked`).
    pub clicked: [bool; MAX_PLAYERS],
    /// Where friends' mice are in the game (for every friend, and per friend), and the clips and
    /// boxes only one player may click: the key map's at first, then whatever the game says.
    pub mouse_mode: crate::coop::keymap::MouseMode,
    pub mouse_modes: [Option<crate::coop::keymap::MouseMode>; MAX_PLAYERS],
    pub zones: Vec<crate::coop::keymap::Zone>,
    /// Private screens: drawn only on their owner's PC, clicked only by them.
    pub privates: Vec<Private>,
    pub hover: crate::coop::keymap::Hover,
    pub cursors: crate::coop::keymap::Cursors,
    /// Whose mouse events run right now (`coop.mouse.clicker`).
    pub clicker: Option<u8>,
    /// What each player's mouse is on, as of the last frame.
    pub hovered: [Option<String>; MAX_PLAYERS],
    /// Each slot's held buttons this frame (the same bits as `lockstep::UP` etc.).
    pub buttons: [u16; MAX_PLAYERS],
    /// What happened that the game hasn't read yet (`coop.events`), oldest first.
    pub events: std::collections::VecDeque<String>,
    /// This PC's round trip to each player in ms (differs per PC, like `local_slot`).
    pub pings: [Option<u32>; MAX_PLAYERS],
    /// The host's game settings, the same on every PC.
    pub options: Vec<(String, String)>,
    /// Each slot's typed characters this frame.
    pub typed: [crate::coop::lockstep::Typed; MAX_PLAYERS],
    /// Frames run, and the frame the game last read `coop.typed`.
    frame: u32,
    typed_read_at: Option<u32>,
    pub names: Vec<String>,
    pub cameras: [Option<Camera>; MAX_PLAYERS],
    pub labels: [Option<Label>; MAX_PLAYERS],
    /// A key map the game gave this frame, for the Stepper to switch to.
    pub keys: Option<String>,
    /// HUD clips the game named this frame, for the provider to mark.
    pub hud: Vec<String>,
    /// Names the game called (tests check that AS2 and AS3 calls arrive).
    #[cfg(test)]
    pub calls: Vec<String>,
}

impl CoopApi {
    pub fn new(players: u8, local_slot: u8) -> Self {
        Self {
            players,
            local_slot,
            mouse: [(0, 0, false); MAX_PLAYERS],
            clicked: [false; MAX_PLAYERS],
            mouse_mode: Default::default(),
            mouse_modes: [None; MAX_PLAYERS],
            zones: vec![],
            privates: vec![],
            hover: Default::default(),
            cursors: Default::default(),
            clicker: None,
            hovered: Default::default(),
            buttons: [0; MAX_PLAYERS],
            typed: Default::default(),
            options: vec![],
            pings: [None; MAX_PLAYERS],
            events: Default::default(),
            frame: 0,
            typed_read_at: None,
            names: vec![],
            cameras: [None; MAX_PLAYERS],
            labels: Default::default(),
            keys: None,
            hud: vec![],
            #[cfg(test)]
            calls: vec![],
        }
    }

    /// Takes this frame's inputs; a slot without a mouse this frame keeps its last one.
    pub fn update(&mut self, inputs: &[InputFrame]) {
        self.frame += 1;
        for (slot, input) in inputs.iter().enumerate().take(MAX_PLAYERS) {
            self.buttons[slot] = input.buttons;
            self.typed[slot] = input.text;
            let was_down = self.mouse[slot].2;
            if let Some(mouse) = input.mouse {
                self.mouse[slot] = mouse;
            }
            self.clicked[slot] = self.mouse[slot].2 && !was_down;
        }
    }

    pub fn wants_text(&self) -> bool {
        self.typed_read_at.is_some_and(|at| self.frame.saturating_sub(at) <= WANTS_TEXT_FRAMES)
    }

    pub fn add_event(&mut self, event: String) {
        if self.events.len() == MAX_EVENTS {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }

    /// Where player `slot`'s mouse is in the game.
    pub fn mouse_mode_of(&self, slot: u8) -> crate::coop::keymap::MouseMode {
        self.mouse_modes.get(slot as usize).copied().flatten().unwrap_or(self.mouse_mode)
    }

    /// The camera this PC draws with.
    pub fn local_camera(&self) -> Option<Camera> {
        self.cameras[self.local_slot as usize]
    }

    pub fn answer(&mut self, name: &str, args: &[Value]) -> Value {
        let slot = match args.first() {
            Some(Value::Number(n)) if *n >= 0.0 && (*n as usize) < self.players as usize => Some(*n as usize),
            _ => None,
        };
        let number = |i: usize| match args.get(i) {
            Some(Value::Number(n)) if n.is_finite() => Some(*n),
            _ => None,
        };
        let text = |i: usize| match args.get(i) {
            Some(Value::String(s)) => Some(s.clone()),
            _ => None,
        };
        match (name, slot) {
            ("coop.players", _) => Value::Number(self.players.into()),
            ("coop.localSlot", _) => Value::Number(self.local_slot.into()),
            ("coop.mouseX", Some(slot)) => Value::Number(self.mouse[slot].0 as f64 / 20.0),
            ("coop.mouseY", Some(slot)) => Value::Number(self.mouse[slot].1 as f64 / 20.0),
            ("coop.mouseDown", Some(slot)) => Value::Bool(self.mouse[slot].2),
            ("coop.clicked", Some(slot)) => Value::Bool(self.clicked[slot]),
            ("coop.mouse", Some(slot)) if slot > 0 => match text(1).and_then(|mode| crate::coop::keymap::MouseMode::named(&mode)) {
                Some(mode) => {
                    self.mouse_modes[slot] = Some(mode);
                    Value::Bool(true)
                }
                None => Value::Bool(false),
            },
            ("coop.mouse", _) => match text(0).and_then(|mode| crate::coop::keymap::MouseMode::named(&mode)) {
                Some(mode) => {
                    self.mouse_mode = mode;
                    self.mouse_modes = [None; MAX_PLAYERS];
                    Value::Bool(true)
                }
                None => Value::Bool(false),
            },
            ("coop.mouse.clicker", _) => self.clicker.map_or(Value::Undefined, |slot| Value::Number(slot.into())),
            ("coop.mouse.hovered", Some(slot)) => self.hovered[slot].clone().map_or(Value::Undefined, Value::String),
            ("coop.mouse.cursors", _) => match text(0).and_then(|mode| crate::coop::keymap::Cursors::named(&mode)) {
                Some(cursors) => {
                    self.cursors = cursors;
                    Value::Bool(true)
                }
                None => Value::Bool(false),
            },
            ("coop.zone", Some(slot)) => match (number(1), number(2), number(3), number(4)) {
                (Some(x), Some(y), Some(width), Some(height)) if width > 0.0 && height > 0.0 => {
                    let area = crate::coop::keymap::ZoneArea::Box { x, y, width, height };
                    self.zones.push(crate::coop::keymap::Zone { slot: slot as u8, area });
                    Value::Bool(true)
                }
                _ if args.len() == 2 && text(1).is_some() => {
                    match text(1).and_then(|path| crate::coop::keymap::Zone::parse(slot as u8, &path)) {
                        Some(zone) => {
                            self.zones.push(zone);
                            Value::Bool(true)
                        }
                        None => Value::Bool(false),
                    }
                }
                (None, None, None, None) if args.len() == 1 => {
                    self.zones.retain(|zone| zone.slot as usize != slot);
                    Value::Bool(true)
                }
                _ => Value::Bool(false),
            },
            ("coop.typed", Some(slot)) => {
                self.typed_read_at = Some(self.frame);
                let text: String = self.typed[slot]
                    .chars()
                    .iter()
                    .filter_map(|&c| match crate::coop::lockstep::typed_event(c) {
                        ruffle_core::PlayerEvent::TextInput { codepoint } => Some(codepoint),
                        ruffle_core::PlayerEvent::TextControl { code } => match code {
                            ruffle_core::events::TextControlCode::Backspace => Some('\u{8}'),
                            ruffle_core::events::TextControlCode::Enter => Some('\n'),
                            _ => None,
                        },
                        _ => None,
                    })
                    .collect();
                Value::String(text)
            }
            ("coop.events", _) => Value::List(self.events.drain(..).map(Value::String).collect()),
            ("coop.ping", Some(slot)) => self.pings[slot].map_or(Value::Undefined, |ms| Value::Number(ms.into())),
            ("coop.option", _) => text(0)
                .and_then(|name| self.options.iter().find(|(n, _)| *n == name))
                .map_or(Value::Undefined, |(_, value)| Value::String(value.clone())),
            ("coop.options", _) => Value::Object(
                self.options.iter().map(|(n, v)| (n.clone(), Value::String(v.clone()))).collect(),
            ),
            ("coop.only", _) => {
                let (fixed, path, openers) = match (slot, text(0), text(1)) {
                    (Some(slot), _, Some(path)) => (Some(slot as u8), path, None),
                    (_, Some(path), openers) => (None, path, openers),
                    _ => return Value::Bool(false),
                };
                self.privates.retain(|private| private.path != path);
                self.privates.push(Private { path, fixed, openers, owner: None });
                Value::Bool(true)
            }
            ("coop.hover", _) => match text(0).and_then(|mode| crate::coop::keymap::Hover::named(&mode)) {
                Some(hover) => {
                    self.hover = hover;
                    Value::Bool(true)
                }
                None => Value::Bool(false),
            },
            ("coop.shared", _) => match text(0) {
                Some(path) => {
                    self.privates.retain(|private| private.path != path);
                    Value::Bool(true)
                }
                None => Value::Bool(false),
            },
            ("coop.buttons", Some(slot)) => Value::Number(self.buttons[slot].into()),
            ("coop.button", Some(slot)) => match text(1).and_then(|name| crate::coop::lockstep::button_named(&name)) {
                Some(button) => Value::Bool(self.buttons[slot] & button != 0),
                None => Value::Undefined,
            },
            ("coop.version", _) => Value::Number(VERSION.into()),
            ("coop.has", _) => Value::Bool(text(0).is_some_and(|f| FEATURES.contains(&f.as_str()))),
            ("coop.name", Some(slot)) => self.names.get(slot).map_or(Value::Undefined, |n| Value::String(n.clone())),
            ("coop.isHost", _) => Value::Bool(self.local_slot == 0),
            ("coop.camera", Some(slot)) => match (number(1), number(2), number(3).unwrap_or(1.0)) {
                (Some(x), Some(y), zoom) if zoom > 0.0 => {
                    self.cameras[slot] = Some(Camera { x, y, zoom });
                    Value::Bool(true)
                }
                (None, None, _) if args.len() == 1 => {
                    self.cameras[slot] = None;
                    Value::Bool(true)
                }
                _ => Value::Bool(false),
            },
            ("coop.hud", _) => match text(0) {
                Some(path) => {
                    self.hud.push(path);
                    Value::Bool(true)
                }
                None => Value::Bool(false),
            },
            ("coop.label", Some(slot)) => match (text(1), text(2).unwrap_or_default()) {
                (Some(_), text) if text.is_empty() => {
                    self.labels[slot] = None;
                    Value::Bool(true)
                }
                (Some(path), text) => {
                    self.labels[slot] = Some(Label { path, text });
                    Value::Bool(true)
                }
                _ => Value::Bool(false),
            },
            ("coop.keys", _) => match text(0) {
                Some(keys) => match crate::coop::keymap::Game::parse("game", &keys) {
                    Ok(_) => {
                        self.keys = Some(keys);
                        Value::Bool(true)
                    }
                    Err(e) => {
                        tracing::warn!("Ruffle Together: the game's key map is wrong: {e}");
                        Value::Bool(false)
                    }
                },
                None => Value::Bool(false),
            },
            _ => Value::Undefined,
        }
    }
}

/// Answers the game's ExternalInterface calls from the shared `CoopApi`.
pub struct CoopApiProvider(pub Arc<Mutex<CoopApi>>);

impl ExternalInterfaceProvider for CoopApiProvider {
    fn call_method(&self, context: &mut UpdateContext<'_>, name: &str, args: &[Value]) -> Value {
        let Ok(mut api) = self.0.lock() else {
            return Value::Undefined;
        };
        #[cfg(test)]
        api.calls.push(name.to_string());
        let answer = api.answer(name, args);
        for path in std::mem::take(&mut api.hud) {
            if !ruffle_core::coop_view::set_hud(context, &path) {
                tracing::warn!("Ruffle Together: coop.hud: no clip at {path}");
            }
        }
        answer
    }

    fn on_callback_available(&self, _name: &str) {}

    fn get_id(&self) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `coop.clicked` is true on the press frame only; `coop.mouse` and `coop.zone` change the
    /// click rules, `coop.zone(slot)` clears that slot's boxes.
    #[test]
    fn clicks_and_click_rules() {
        use crate::coop::keymap::MouseMode;
        let mut api = CoopApi::new(2, 0);
        let mouse = |down| InputFrame { mouse: Some((100, 100, down)), ..Default::default() };
        api.update(&[InputFrame::default(), mouse(true)]);
        assert_eq!(api.answer("coop.clicked", &[Value::Number(1.0)]), Value::Bool(true));
        api.update(&[InputFrame::default(), mouse(true)]);
        assert_eq!(api.answer("coop.clicked", &[Value::Number(1.0)]), Value::Bool(false), "held, not a new click");
        assert_eq!(api.answer("coop.mouse", &[Value::String("buttons".into())]), Value::Bool(true));
        assert_eq!(api.mouse_mode, MouseMode::Buttons);
        assert_eq!(api.answer("coop.mouse", &[Value::String("nobody".into())]), Value::Bool(false));
        let zone = [1.0, 10.0, 20.0, 30.0, 40.0].map(Value::Number);
        assert_eq!(api.answer("coop.zone", &zone), Value::Bool(true));
        assert_eq!(api.zones.len(), 1);
        assert_eq!(api.answer("coop.zone", &[Value::Number(1.0)]), Value::Bool(true));
        assert!(api.zones.is_empty());
        // private screens: whoever opens it, or a fixed player; coop.shared ends it
        assert_eq!(api.answer("coop.only", &[Value::String("_root.notes".into())]), Value::Bool(true));
        assert_eq!(api.answer("coop.only", &[Value::Number(1.0), Value::String("_root.shop".into())]), Value::Bool(true));
        let privates: Vec<_> = api.privates.iter().map(|p| (p.path.as_str(), p.fixed)).collect();
        assert_eq!(privates, [("_root.notes", None), ("_root.shop", Some(1))]);
        assert_eq!(api.answer("coop.shared", &[Value::String("_root.notes".into())]), Value::Bool(true));
        assert_eq!(api.privates.len(), 1);
        assert_eq!(api.answer("coop.only", &[]), Value::Bool(false));
        let with_openers = [Value::String("_root.pcb".into()), Value::String("_root.players.*.instance*".into())];
        assert_eq!(api.answer("coop.only", &with_openers), Value::Bool(true));
        assert_eq!(api.privates.last().and_then(|p| p.openers.as_deref()), Some("_root.players.*.instance*"));
        assert_eq!(api.answer("coop.hover", &[Value::String("shared".into())]), Value::Bool(true));
        assert_eq!(api.hover, crate::coop::keymap::Hover::Shared);
    }

    /// One friend's mouse on its own; whose events run; what each mouse is on; friends' cursors.
    #[test]
    fn mice_per_player() {
        use crate::coop::keymap::{Cursors, MouseMode};
        let mut api = CoopApi::new(3, 0);
        assert_eq!(api.answer("coop.mouse", &[s("zones")]), Value::Bool(true));
        assert_eq!(api.answer("coop.mouse", &[n(2.0), s("all")]), Value::Bool(true));
        assert_eq!((api.mouse_mode_of(1), api.mouse_mode_of(2)), (MouseMode::Zones, MouseMode::All));
        assert_eq!(api.answer("coop.mouse", &[n(0.0), s("all")]), Value::Bool(false), "the host's mouse is always in");
        assert_eq!(api.answer("coop.mouse", &[s("host")]), Value::Bool(true));
        assert_eq!(api.mouse_mode_of(2), MouseMode::Host, "coop.mouse(mode) is everyone's");
        assert_eq!(api.answer("coop.mouse.clicker", &[]), Value::Undefined);
        api.clicker = Some(2);
        assert_eq!(api.answer("coop.mouse.clicker", &[]), n(2.0));
        api.hovered[1] = Some("_root.menu.sword".into());
        assert_eq!(api.answer("coop.mouse.hovered", &[n(1.0)]), s("_root.menu.sword"));
        assert_eq!(api.answer("coop.mouse.hovered", &[n(2.0)]), Value::Undefined);
        assert_eq!(api.answer("coop.mouse.cursors", &[s("off")]), Value::Bool(true));
        assert_eq!(api.cursors, Cursors::Off);
        assert_eq!(api.answer("coop.mouse.cursors", &[s("some")]), Value::Bool(false));
        assert_eq!(api.answer("coop.has", &[s("clicker")]), Value::Bool(true));
    }

    fn n(v: f64) -> Value {
        Value::Number(v)
    }

    fn s(v: &str) -> Value {
        Value::String(v.into())
    }

    #[test]
    fn answers_in_stage_pixels_per_slot() {
        let mut api = CoopApi::new(3, 2);
        api.update(&[
            InputFrame { buttons: 0, mouse: Some((2000, 4000, false)), ..Default::default() },
            InputFrame { buttons: 0, mouse: Some((200, 300, true)), ..Default::default() },
            InputFrame::default(),
        ]);
        assert_eq!(api.answer("coop.players", &[]), n(3.0));
        assert_eq!(api.answer("coop.localSlot", &[]), n(2.0));
        assert_eq!(api.answer("coop.mouseX", &[n(0.0)]), n(100.0));
        assert_eq!(api.answer("coop.mouseY", &[n(1.0)]), n(15.0));
        assert_eq!(api.answer("coop.mouseDown", &[n(1.0)]), Value::Bool(true));
        assert_eq!(api.answer("coop.mouseX", &[n(2.0)]), n(0.0), "slot 2 hasn't moved yet");
        assert_eq!(api.answer("coop.mouseX", &[n(3.0)]), Value::Undefined, "only 3 players");
        assert_eq!(api.answer("coop.mouseX", &[]), Value::Undefined);
        assert_eq!(api.answer("location.href", &[]), Value::Undefined);
        // no mouse this frame: the last one stays
        api.update(&[InputFrame::default(), InputFrame::default(), InputFrame::default()]);
        assert_eq!(api.answer("coop.mouseX", &[n(0.0)]), n(100.0));
    }

    /// A mod can read every player's buttons directly, without faking keyboard keys.
    #[test]
    fn every_slots_buttons() {
        use crate::coop::lockstep::{FIRE, LEFT, UP};
        let mut api = CoopApi::new(2, 0);
        api.update(&[
            InputFrame { buttons: UP | FIRE, ..Default::default() },
            InputFrame { buttons: LEFT, ..Default::default() },
        ]);
        assert_eq!(api.answer("coop.buttons", &[n(0.0)]), n((UP | FIRE).into()));
        assert_eq!(api.answer("coop.button", &[n(0.0), s("FIRE")]), Value::Bool(true));
        assert_eq!(api.answer("coop.button", &[n(1.0), s("FIRE")]), Value::Bool(false));
        assert_eq!(api.answer("coop.button", &[n(1.0), s("LEFT")]), Value::Bool(true));
        assert_eq!(api.answer("coop.button", &[n(1.0), s("JUMP")]), Value::Undefined, "no such button");
        assert_eq!(api.answer("coop.buttons", &[n(2.0)]), Value::Undefined, "only 2 players");
        assert_eq!(api.answer("coop.has", &[s("buttons")]), Value::Bool(true));
        // a let-go button reads false on the next frame
        api.update(&[InputFrame::default(), InputFrame::default()]);
        assert_eq!(api.answer("coop.buttons", &[n(0.0)]), n(0.0));
    }

    /// Each player's own typed text this frame, with Backspace and Enter as "\b" and "\n"; reading
    /// it tells Ruffle the game wants every typed character for a while.
    #[test]
    fn every_slots_typed_text() {
        use crate::coop::lockstep::{Typed, text_control_char};
        use ruffle_core::events::TextControlCode::{Backspace, Enter, MoveLeft};
        let mut api = CoopApi::new(2, 0);
        let typed = |text: &str| Typed::from_chars(text.chars());
        api.update(&[
            InputFrame { text: typed("Hi"), ..Default::default() },
            InputFrame {
                text: Typed::from_chars([
                    'x',
                    text_control_char(Backspace),
                    text_control_char(MoveLeft),
                    text_control_char(Enter),
                ]),
                ..Default::default()
            },
        ]);
        assert!(!api.wants_text());
        assert_eq!(api.answer("coop.typed", &[n(0.0)]), s("Hi"));
        assert_eq!(api.answer("coop.typed", &[n(1.0)]), s("x\u{8}\n"), "other text keys left out");
        assert!(api.wants_text());
        api.update(&[InputFrame::default(), InputFrame::default()]);
        assert_eq!(api.answer("coop.typed", &[n(0.0)]), s(""), "typed once");
        for _ in 0..=WANTS_TEXT_FRAMES {
            api.update(&[InputFrame::default(), InputFrame::default()]);
        }
        assert!(!api.wants_text(), "not read for a while");
    }

    #[test]
    fn events_are_read_once() {
        let mut api = CoopApi::new(2, 0);
        api.add_event("join 0".into());
        api.add_event("leave 1".into());
        assert_eq!(api.answer("coop.events", &[]), Value::List(vec![s("join 0"), s("leave 1")]));
        assert_eq!(api.answer("coop.events", &[]), Value::List(vec![]));
        for i in 0..MAX_EVENTS + 5 {
            api.add_event(format!("e{i}"));
        }
        let Value::List(kept) = api.answer("coop.events", &[]) else { panic!("a list") };
        assert_eq!(kept.len(), MAX_EVENTS);
        assert_eq!(kept[0], s("e5"), "the oldest went first");
    }

    #[test]
    fn pings_per_slot() {
        let mut api = CoopApi::new(3, 1);
        api.pings = [None; MAX_PLAYERS];
        api.pings[..2].copy_from_slice(&[Some(80), Some(0)]);
        assert_eq!(api.answer("coop.ping", &[n(0.0)]), n(80.0));
        assert_eq!(api.answer("coop.ping", &[n(1.0)]), n(0.0));
        assert_eq!(api.answer("coop.ping", &[n(2.0)]), Value::Undefined, "not measured yet");
        assert_eq!(api.answer("coop.ping", &[n(3.0)]), Value::Undefined, "no slot 3");
    }

    #[test]
    fn the_hosts_options() {
        let mut api = CoopApi::new(2, 1);
        api.options = vec![("map".into(), "3".into()), ("mode".into(), "team play".into())];
        assert_eq!(api.answer("coop.option", &[s("map")]), s("3"));
        assert_eq!(api.answer("coop.option", &[s("mode")]), s("team play"));
        assert_eq!(api.answer("coop.option", &[s("lives")]), Value::Undefined);
        let Value::Object(all) = api.answer("coop.options", &[]) else { panic!("an object") };
        assert_eq!(all.get("map"), Some(&s("3")));
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn version_features_names_and_host() {
        let mut api = CoopApi::new(2, 1);
        api.names = vec!["Liam".into(), "Sam".into()];
        assert_eq!(api.answer("coop.version", &[]), n(2.0));
        assert_eq!(api.answer("coop.has", &[s("camera")]), Value::Bool(true));
        assert_eq!(api.answer("coop.has", &[s("keys")]), Value::Bool(true));
        assert_eq!(api.answer("coop.has", &[s("teleport")]), Value::Bool(false));
        assert_eq!(api.answer("coop.has", &[]), Value::Bool(false));
        assert_eq!(api.answer("coop.name", &[n(1.0)]), s("Sam"));
        assert_eq!(api.answer("coop.name", &[n(2.0)]), Value::Undefined);
        assert_eq!(api.answer("coop.isHost", &[]), Value::Bool(false));
        assert_eq!(CoopApi::new(2, 0).answer("coop.isHost", &[]), Value::Bool(true));
    }

    #[test]
    fn every_slot_has_a_camera_and_this_pc_draws_its_own() {
        let mut api = CoopApi::new(2, 1);
        assert_eq!(api.local_camera(), None);
        assert_eq!(api.answer("coop.camera", &[n(0.0), n(100.0), n(50.0), n(2.0)]), Value::Bool(true));
        assert_eq!(api.answer("coop.camera", &[n(1.0), n(300.0), n(40.0)]), Value::Bool(true));
        assert_eq!(api.cameras[0], Some(Camera { x: 100.0, y: 50.0, zoom: 2.0 }));
        assert_eq!(api.local_camera(), Some(Camera { x: 300.0, y: 40.0, zoom: 1.0 }), "zoom 1 by default");
        assert_eq!(api.answer("coop.camera", &[n(1.0)]), Value::Bool(true));
        assert_eq!(api.local_camera(), None, "(slot) alone turns it off");
        assert_eq!(api.answer("coop.camera", &[n(0.0), n(1.0), n(1.0), n(0.0)]), Value::Bool(false), "zoom 0");
        assert_eq!(api.answer("coop.camera", &[n(0.0), n(1.0)]), Value::Bool(false), "x without y");
        assert_eq!(api.answer("coop.camera", &[n(2.0), n(1.0), n(1.0)]), Value::Undefined, "no slot 2");
    }

    #[test]
    fn labels_and_hud_paths() {
        let mut api = CoopApi::new(2, 0);
        assert_eq!(api.answer("coop.label", &[n(1.0), s("_root.p2"), s("Sam")]), Value::Bool(true));
        assert_eq!(api.labels[1], Some(Label { path: "_root.p2".into(), text: "Sam".into() }));
        assert_eq!(api.answer("coop.label", &[n(1.0), s("_root.p2"), s("")]), Value::Bool(true));
        assert_eq!(api.labels[1], None);
        assert_eq!(api.answer("coop.label", &[n(1.0)]), Value::Bool(false), "no path");
        assert_eq!(api.answer("coop.hud", &[s("_root.hud")]), Value::Bool(true));
        assert_eq!(api.hud, vec!["_root.hud".to_string()]);
        assert_eq!(api.answer("coop.hud", &[]), Value::Bool(false));
    }

    #[test]
    fn the_game_can_give_its_own_key_map() {
        let mut api = CoopApi::new(2, 0);
        assert_eq!(api.answer("coop.keys", &[s("slot 0: FIRE=32\nslot 1: FIRE=13")]), Value::Bool(true));
        assert_eq!(api.keys.as_deref(), Some("slot 0: FIRE=32\nslot 1: FIRE=13"));
        api.keys = None;
        assert_eq!(api.answer("coop.keys", &[s("slot 0: JUMP=32")]), Value::Bool(false), "unknown button");
        assert_eq!(api.keys, None);
    }
}
