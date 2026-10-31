//! Every player's mouse. Each player has a mouse of their own in the game (Ruffle's core runs
//! hover, press, release and drag for each), but only where the rules let it be: the key map's
//! or the game's mouse modes, zones and private screens. Elsewhere it is outside the stage.
//! Everything that reaches the game comes from inputs and game state, so every PC does the same.

use crate::coop::api::{CoopApi, Private};
use crate::coop::keymap::{Cursors, Hover, MouseMode, Zone, ZoneArea, path_fits};
use crate::coop::lockstep::{InputFrame, MAX_PLAYERS};
use ruffle_core::events::MouseButton;
use ruffle_core::{Player, PlayerEvent};
use std::sync::{Arc, Mutex};

/// Where each player's mouse may be this frame.
struct Rules {
    modes: [MouseMode; MAX_PLAYERS],
    /// The key map's and the game's zones, and each open private screen as its owner's.
    zones: Vec<Zone>,
    privates: Vec<Private>,
}

impl Rules {
    fn of(api: &CoopApi) -> Self {
        let mut zones = api.zones.clone();
        zones.extend(api.privates.iter().filter_map(|p| Some(Zone { slot: p.owner?, area: ZoneArea::Clip(p.path.clone()) })));
        Self { modes: std::array::from_fn(|slot| api.mouse_mode_of(slot as u8)), zones, privates: api.privates.clone() }
    }

    /// May `slot`'s mouse be at the stage point (x, y) (twips)? Never on what belongs to someone
    /// else; the host's anywhere else; a friend's as their mode says. Call it as that player
    /// (`coop_with_pointer`), so other players' private screens are out of the way.
    fn allows(&self, player: &mut Player, slot: u8, x: i32, y: i32) -> bool {
        let target = player.coop_click_target(x, y);
        let target = target.as_deref();
        Zone::allows(&self.zones, slot, x, y, target)
            && match self.modes[slot as usize] {
                _ if slot == 0 => true,
                MouseMode::Host => false,
                MouseMode::Zones => target.is_some() && Zone::is_own(&self.zones, slot, x, y, target),
                MouseMode::Buttons => target.is_some(),
                MouseMode::All => true,
            }
    }

    /// Whose open private screen `target` would open, if it is someone else's: the game has one
    /// of it, so a second player opening it would take it over.
    fn busy(&self, slot: u8, target: &str) -> Option<u8> {
        self.privates.iter().find_map(|p| {
            let opener = p.openers.as_deref().is_some_and(|pattern| path_fits(target, pattern));
            p.owner.filter(|&owner| owner != slot && opener)
        })
    }

    /// Whether clicks are logged for key map authors (what each click lands on).
    fn logged(&self) -> bool {
        self.modes.iter().skip(1).any(|&mode| mode != MouseMode::Host) || !self.privates.is_empty()
    }
}

/// A player's mouse for the mouse debug overlay.
#[derive(Clone, Debug, PartialEq)]
pub struct MouseDebug {
    pub slot: u8,
    pub x: i32,
    pub y: i32,
    pub down: bool,
    /// What it is on in the game (none while it is outside the stage).
    pub hovered: Option<String>,
    /// Whether the rules let it be in the game there.
    pub allowed: bool,
}

#[derive(Default)]
pub struct Mice {
    /// Each player's last mouse input: stage twips and the left button.
    last: [Option<(i32, i32, bool)>; MAX_PLAYERS],
    /// The host's mouse input last frame, none included: the host's mouse moves whenever it
    /// differs (as replays recorded before every player had a mouse expect).
    host_input: Option<(i32, i32, bool)>,
    /// Friends with a mouse in the game (the host's always is).
    on: [bool; MAX_PLAYERS],
    /// Friends whose mouse is inside the stage right now.
    inside: [bool; MAX_PLAYERS],
    /// Players whose press reached the game and who haven't let go.
    down: [bool; MAX_PLAYERS],
    /// Whose press reached the game last: a private screen that opens belongs to them.
    last_clicker: Option<u8>,
    /// Presses refused because another player had that private screen open: (who, whose).
    busy: Vec<(u8, u8)>,
}

impl Mice {
    /// Gives this frame's mouse inputs to the game: each friend's mouse comes in or goes out as
    /// their mode changes, then every player's moves and presses, in slot order.
    pub fn feed(&mut self, player: &mut Player, api: &Arc<Mutex<CoopApi>>, inputs: &[InputFrame]) {
        let Ok(rules) = api.lock().map(|api| Rules::of(&api)) else {
            return;
        };
        let players = inputs.len().min(MAX_PLAYERS);
        let host = inputs.first().and_then(|input| input.mouse);
        let host_moved = host != std::mem::replace(&mut self.host_input, host);
        for slot in 1..players {
            let on = rules.modes[slot] != MouseMode::Host;
            if on != self.on[slot] {
                player.coop_set_pointer(slot as u8, on);
                // a slot core has no mouse for stays off: feeding it would move the host's mouse
                let on = on && player.coop_has_pointer(slot as u8);
                (self.on[slot], self.inside[slot], self.down[slot]) = (on, false, false);
            }
        }
        for (slot, input) in inputs.iter().enumerate().take(players) {
            let Some((x, y, down)) = input.mouse else {
                continue;
            };
            let before = std::mem::replace(&mut self.last[slot], input.mouse);
            if slot == 0 {
                self.feed_host(player, api, &rules, (host_moved, before), x, y, down);
            } else if self.on[slot] {
                self.feed_friend(player, api, &rules, slot as u8, before, x, y, down);
            }
        }
    }

    /// The host's mouse is always in the stage, wherever this PC's own cursor is.
    #[allow(clippy::too_many_arguments)]
    fn feed_host(
        &mut self,
        player: &mut Player,
        api: &Arc<Mutex<CoopApi>>,
        rules: &Rules,
        (moved, before): (bool, Option<(i32, i32, bool)>),
        x: i32,
        y: i32,
        down: bool,
    ) {
        if moved {
            player.coop_with_pointer(0, |player| move_mouse(player, x, y));
        }
        if down && !before.is_some_and(|(_, _, down)| down) {
            if self.may_press(player, rules, 0, x, y) {
                self.press(player, api, 0, x, y);
            }
        } else if !down && self.down[0] {
            self.release(player, api, 0, x, y);
        }
    }

    /// A friend's mouse is in the stage only where the rules allow; a press that reached the game
    /// stays in it until let go, wherever it moves (a drag).
    #[allow(clippy::too_many_arguments)]
    fn feed_friend(
        &mut self,
        player: &mut Player,
        api: &Arc<Mutex<CoopApi>>,
        rules: &Rules,
        slot: u8,
        before: Option<(i32, i32, bool)>,
        x: i32,
        y: i32,
        down: bool,
    ) {
        let s = slot as usize;
        let moved = before.is_none_or(|(bx, by, _)| (bx, by) != (x, y));
        if self.down[s] {
            if moved {
                player.coop_with_pointer(slot, |player| move_mouse(player, x, y));
            }
            if !down {
                self.release(player, api, slot, x, y);
            }
            return;
        }
        let allowed = player.coop_with_pointer(slot, |player| rules.allows(player, slot, x, y));
        if allowed && (moved || !self.inside[s]) {
            player.coop_with_pointer(slot, |player| {
                player.set_mouse_in_stage(true);
                move_mouse(player, x, y);
            });
            self.inside[s] = true;
        } else if !allowed && self.inside[s] {
            player.coop_with_pointer(slot, Player::coop_pointer_leave);
            self.inside[s] = false;
        }
        if down && !before.is_some_and(|(_, _, down)| down) && self.may_press(player, rules, slot, x, y) {
            self.press(player, api, slot, x, y);
        }
    }

    /// Whether `slot`'s press at (x, y) (twips) reaches the game; logs what it lands on.
    fn may_press(&mut self, player: &mut Player, rules: &Rules, slot: u8, x: i32, y: i32) -> bool {
        let (allowed, target) = player.coop_with_pointer(slot, |player| {
            (rules.allows(player, slot, x, y), player.coop_click_target(x, y))
        });
        let busy = target.as_deref().and_then(|target| rules.busy(slot, target));
        if let Some(owner) = busy {
            self.busy.push((slot, owner));
        }
        let allowed = allowed && busy.is_none();
        if rules.logged() {
            let target = target.unwrap_or_else(|| "nothing clickable".into());
            let verdict = if allowed { "clicks" } else { "may not click" };
            tracing::info!("Ruffle Together: player {} {verdict} {target} ({}, {})", slot + 1, x / 20, y / 20);
        }
        allowed
    }

    fn press(&mut self, player: &mut Player, api: &Arc<Mutex<CoopApi>>, slot: u8, x: i32, y: i32) {
        as_clicker(api, slot, || {
            player.coop_with_pointer(slot, |player| {
                let (x, y) = player.stage_to_viewport(x, y);
                player.handle_event(PlayerEvent::MouseDown { x, y, button: MouseButton::Left, index: Some(0) });
            })
        });
        self.down[slot as usize] = true;
        self.last_clicker = Some(slot);
    }

    fn release(&mut self, player: &mut Player, api: &Arc<Mutex<CoopApi>>, slot: u8, x: i32, y: i32) {
        as_clicker(api, slot, || {
            player.coop_with_pointer(slot, |player| {
                let (x, y) = player.stage_to_viewport(x, y);
                player.handle_event(PlayerEvent::MouseUp { x, y, button: MouseButton::Left });
            })
        });
        self.down[slot as usize] = false;
    }

    /// Who each private screen belongs to (game state and whose press came last, so the same on
    /// every PC), whether this PC draws it (only its owner's does), and whose mice go through it.
    pub fn update_privates(&self, player: &mut Player, api: &Arc<Mutex<CoopApi>>) {
        let Ok((mut privates, local, players)) = api.lock().map(|api| (api.privates.clone(), api.local_slot, api.players))
        else {
            return;
        };
        for private in &mut privates {
            private.owner = if player.coop_clip_shown(&private.path) {
                private.fixed.or(private.owner).or(self.last_clicker).or(Some(0))
            } else {
                None
            };
            player.coop_set_drawn(&private.path, private.owner.is_none_or(|owner| owner == local));
        }
        for slot in 0..players {
            let others: Vec<String> =
                privates.iter().filter(|p| p.owner.is_some_and(|o| o != slot)).map(|p| p.path.clone()).collect();
            player.set_coop_hidden(slot, others);
        }
        if let Ok(mut api) = api.lock() {
            for private in privates {
                if let Some(p) = api.privates.iter_mut().find(|p| p.path == private.path) {
                    p.owner = private.owner;
                }
            }
        }
    }

    /// After the frame: what each player's mouse is on (`coop.mouse.hovered`, the same on every
    /// PC), and what this PC draws (`hover`).
    pub fn after_frame(&self, player: &mut Player, api: &Arc<Mutex<CoopApi>>) {
        let Ok((players, hover)) = api.lock().map(|api| (api.players as usize, api.hover)) else {
            return;
        };
        let hovered: Vec<Option<String>> = (0..players)
            .map(|slot| (slot == 0 || self.on[slot]).then(|| player.coop_hovered_path(slot as u8)).flatten())
            .collect();
        if let Ok(mut api) = api.lock() {
            for (slot, path) in hovered.into_iter().enumerate() {
                api.hovered[slot] = path;
            }
        }
        player.coop_set_view(hover == Hover::Own);
    }

    /// The other players' cursors (slot, stage twips, button) for this PC to draw (the picture
    /// only): with `cursors: auto`, only while a friend may have mice and one is over something
    /// that player may click.
    pub fn cursors(&self, player: &mut Player, api: &Arc<Mutex<CoopApi>>) -> Vec<(u8, i32, i32, bool)> {
        let Ok((rules, cursors, players, local)) =
            api.lock().map(|api| (Rules::of(&api), api.cursors, api.players, api.local_slot))
        else {
            return vec![];
        };
        let others = (0..players).filter(|&slot| slot != local).filter_map(|slot| {
            let (x, y, down) = self.last[slot as usize]?;
            Some((slot, x, y, down))
        });
        match cursors {
            Cursors::Off => vec![],
            Cursors::On => others.collect(),
            Cursors::Auto if !self.on.iter().any(|&on| on) => vec![],
            Cursors::Auto => others
                .filter(|&(slot, x, y, _)| {
                    (slot == 0 || self.on[slot as usize])
                        && player.coop_with_pointer(slot, |player| {
                            let target = player.coop_click_target(x, y);
                            // not over another player's private screen, which this PC doesn't draw
                            let unseen = target.as_deref().is_some_and(|target| {
                                rules.privates.iter().any(|p| p.owner.is_some_and(|o| o != local) && inside(target, &p.path))
                            });
                            target.is_some() && !unseen && rules.allows(player, slot, x, y)
                        })
                        && !rules.privates.iter().any(|p| {
                            // nor over this PC's own private screen, which that player doesn't see
                            p.owner == Some(local) && player.coop_clip_covers(&p.path, x, y)
                        })
                })
                .collect(),
        }
    }

    /// Every player's mouse, for the mouse debug overlay.
    pub fn debug(&self, player: &mut Player, api: &Arc<Mutex<CoopApi>>) -> Vec<MouseDebug> {
        let Ok((rules, players, hovered)) = api.lock().map(|api| (Rules::of(&api), api.players, api.hovered.clone())) else {
            return vec![];
        };
        (0..players)
            .filter_map(|slot| {
                let (x, y, down) = self.last[slot as usize]?;
                let allowed = player.coop_with_pointer(slot, |player| rules.allows(player, slot, x, y));
                Some(MouseDebug { slot, x, y, down, hovered: hovered[slot as usize].clone(), allowed })
            })
            .collect()
    }

    /// Presses refused since the last call because another player had that private screen
    /// open: (who pressed, whose it is).
    pub fn take_busy(&mut self) -> Vec<(u8, u8)> {
        std::mem::take(&mut self.busy)
    }
}

/// Runs `f` with `coop.mouse.clicker` answering `slot`.
fn as_clicker<R>(api: &Arc<Mutex<CoopApi>>, slot: u8, f: impl FnOnce() -> R) -> R {
    if let Ok(mut api) = api.lock() {
        api.clicker = Some(slot);
    }
    let result = f();
    if let Ok(mut api) = api.lock() {
        api.clicker = None;
    }
    result
}

/// The key map's private screens, nobody's yet.
pub fn privates(specs: &[(String, Option<String>)]) -> Vec<Private> {
    specs
        .iter()
        .map(|(path, openers)| Private { path: path.clone(), fixed: None, openers: openers.clone(), owner: None })
        .collect()
}

/// Whether `target` is the clip at `path` or inside it.
fn inside(target: &str, path: &str) -> bool {
    target == path || target.strip_prefix(path).is_some_and(|rest| rest.starts_with('.'))
}

/// Moves the active player's mouse to the stage point (x, y) (twips). Inputs carry stage
/// positions, used exactly as they are on every PC (the window position is only for Ruffle's
/// event plumbing).
fn move_mouse(player: &mut Player, x: i32, y: i32) {
    player.set_stage_mouse_override(Some((x, y)));
    let (x, y) = player.stage_to_viewport(x, y);
    player.handle_event(PlayerEvent::MouseMove { x, y });
}
