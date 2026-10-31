//! Ruffle Together: one mouse per player. Ruffle's own mouse belongs to whichever player is
//! active (player 1, slot 0, unless another player's events are running); every other player's
//! mouse waits here. Switching swaps them, so Ruffle's own mouse code (hover, press, release,
//! drag) runs for each player unchanged.

use crate::backend::ui::MouseCursor;
use crate::display_object::{InteractiveObject, TInteractiveObject};
use crate::prelude::*;
use gc_arena::Collect;

/// The most players a session has (the desktop's `MAX_PLAYERS`, which checks they match).
pub const MAX_POINTERS: usize = 8;

/// One player's mouse while it isn't Ruffle's own.
#[derive(Clone, Copy, Collect)]
#[collect(no_drop)]
pub struct Pointer<'gc> {
    #[collect(require_static)]
    pub position: Point<Twips>,
    #[collect(require_static)]
    pub stage_override: Option<Point<Twips>>,
    pub in_stage: bool,
    pub hovered: Option<InteractiveObject<'gc>>,
    pub pressed: Option<InteractiveObject<'gc>>,
    pub down: bool,
    #[collect(require_static)]
    pub cursor: MouseCursor,
    pub cursor_needs_check: bool,
}

impl Default for Pointer<'_> {
    /// A new mouse, outside the stage until it moves in.
    fn default() -> Self {
        Self {
            position: Point::ZERO,
            stage_override: None,
            in_stage: false,
            hovered: None,
            pressed: None,
            down: false,
            cursor: MouseCursor::Arrow,
            cursor_needs_check: false,
        }
    }
}

#[derive(Collect, Default)]
#[collect(no_drop)]
pub struct Pointers<'gc> {
    /// By slot. Empty for a player without a mouse in the game, and for the active player,
    /// whose mouse is Ruffle's own right now.
    pub stored: [Option<Pointer<'gc>>; MAX_POINTERS],
    /// Whose mouse Ruffle's own is.
    pub active: u8,
    /// Whose mouse started the drag (`startDrag`); `None` is player 1's.
    pub drag_owner: Option<u8>,
    /// Some player other than player 1 has a mouse in the game.
    pub on: bool,
}

impl<'gc> Pointers<'gc> {
    /// What the other players' mice are over (not the active one's).
    pub fn others_hovering(&self) -> impl Iterator<Item = InteractiveObject<'gc>> + '_ {
        self.stored.iter().flatten().filter_map(|pointer| pointer.hovered)
    }

    /// Whether another player's mouse is over `object` too: then it rolls over only for the first
    /// mouse and out only for the last.
    pub fn shared_hover(&self, object: InteractiveObject<'gc>) -> bool {
        self.others_hovering().any(|other| DisplayObject::ptr_eq(other.as_displayobject(), object.as_displayobject()))
    }
}
