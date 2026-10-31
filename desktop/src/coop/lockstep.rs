//! Which frame may run next: a frame runs only once every player's input for it is in.

use ruffle_core::events::TextControlCode;
use std::collections::BTreeMap;

pub const UP: u16 = 1;
pub const DOWN: u16 = 2;
pub const LEFT: u16 = 4;
pub const RIGHT: u16 = 8;
pub const FIRE: u16 = 16;
pub const ALT: u16 = 32;
/// More buttons for games with more keys (Territory War: the turn menu's 1-4, view, quit, yes, no).
pub const KEY1: u16 = 64;
pub const KEY2: u16 = 128;
pub const KEY3: u16 = 256;
pub const KEY4: u16 = 512;
pub const VIEW: u16 = 1024;
pub const QUIT: u16 = 2048;
pub const YES: u16 = 4096;
pub const NO: u16 = 8192;
/// A menu key (Esc: the game's pause, skip a cutscene), for games that need one.
pub const MENU: u16 = 16384;

/// A button by its name in input scripts and key maps (UP, FIRE, KEY1, ...; "0" is none).
pub fn button_named(name: &str) -> Option<u16> {
    Some(match name {
        "0" => 0,
        "UP" => UP,
        "DOWN" => DOWN,
        "LEFT" => LEFT,
        "RIGHT" => RIGHT,
        "FIRE" => FIRE,
        "ALT" => ALT,
        "KEY1" => KEY1,
        "KEY2" => KEY2,
        "KEY3" => KEY3,
        "KEY4" => KEY4,
        "VIEW" => VIEW,
        "QUIT" => QUIT,
        "YES" => YES,
        "NO" => NO,
        "MENU" => MENU,
        _ => return None,
    })
}

/// Up to 8 players; each game's key map says how many it takes (most take 2 or 4).
pub const MAX_PLAYERS: usize = 8;
// every player needs a mouse of their own in core
const _: () = assert!(MAX_PLAYERS == ruffle_core::coop_mice::MAX_POINTERS);

/// One player's input for one frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct InputFrame {
    /// UP, DOWN, LEFT, RIGHT, FIRE, ALT bits.
    pub buttons: u16,
    /// Host only: mouse position in stage twips and whether the button is down.
    pub mouse: Option<(i32, i32, bool)>,
    /// What the player typed this frame (names, chat), for the game's text fields.
    pub text: Typed,
}

/// Characters typed on one PC in one frame, in order: up to 8 (more wait for the next frame).
/// Text keys (Backspace, Enter, arrows in a text field...) are private-use characters, see
/// `text_control_char`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Typed {
    len: u8,
    chars: [char; Typed::MAX],
}

impl Typed {
    pub const MAX: usize = 8;

    /// Adds `c`; false when full.
    pub fn push(&mut self, c: char) -> bool {
        if self.len as usize == Self::MAX {
            return false;
        }
        self.chars[self.len as usize] = c;
        self.len += 1;
        true
    }

    pub fn chars(&self) -> &[char] {
        &self.chars[..self.len as usize]
    }

    pub fn from_chars(chars: impl IntoIterator<Item = char>) -> Self {
        let mut typed = Self::default();
        for c in chars {
            typed.push(c);
        }
        typed
    }
}

/// Every text key a text field knows, in a fixed order: each travels as U+E000 + its index.
const TEXT_CONTROLS: [TextControlCode; 25] = {
    use TextControlCode::*;
    [
        MoveLeft, MoveLeftWord, MoveLeftLine, MoveLeftDocument, MoveRight, MoveRightWord, MoveRightLine,
        MoveRightDocument, SelectLeft, SelectLeftWord, SelectLeftLine, SelectLeftDocument, SelectRight,
        SelectRightWord, SelectRightLine, SelectRightDocument, SelectAll, Copy, Paste, Cut, Backspace,
        BackspaceWord, Enter, Delete, DeleteWord,
    ]
};

pub fn text_control_char(code: TextControlCode) -> char {
    let index = TEXT_CONTROLS.iter().position(|c| *c == code).unwrap_or_default() as u32;
    char::from_u32(0xE000 + index).unwrap_or('\u{E000}')
}

pub fn is_text_control(c: char) -> bool {
    (c as u32).wrapping_sub(0xE000) < TEXT_CONTROLS.len() as u32
}

/// The game event for a typed character: a text key, or a character to type.
pub fn typed_event(c: char) -> ruffle_core::PlayerEvent {
    let index = (c as u32).wrapping_sub(0xE000) as usize;
    match TEXT_CONTROLS.get(index) {
        Some(&code) => ruffle_core::PlayerEvent::TextControl { code },
        None => ruffle_core::PlayerEvent::TextInput { codepoint: c },
    }
}

pub struct Lockstep {
    player_count: u8,
    local_index: u8,
    /// The next frame to run.
    frame: u32,
    /// The frame the next local input is for (current frame + delay).
    next_local: u32,
    inputs: BTreeMap<u32, [Option<InputFrame>; MAX_PLAYERS]>,
    /// The frame each player left at: from then on they hold nothing and are never waited for.
    gone: [Option<u32>; MAX_PLAYERS],
}

impl Lockstep {
    /// Frames `0..delay` start with empty input for everyone, so the game starts at once.
    pub fn new(player_count: u8, local_index: u8, delay: u32) -> Self {
        assert!((1..=MAX_PLAYERS as u8).contains(&player_count) && local_index < player_count);
        let mut inputs = BTreeMap::new();
        for frame in 0..delay {
            let mut row = [None; MAX_PLAYERS];
            for slot in row.iter_mut().take(player_count as usize) {
                *slot = Some(InputFrame::default());
            }
            inputs.insert(frame, row);
        }
        Self {
            player_count,
            local_index,
            frame: 0,
            next_local: delay,
            inputs,
            gone: [None; MAX_PLAYERS],
        }
    }

    /// Stores this PC's input for the frame `delay` frames ahead; returns that frame, to send.
    pub fn push_local(&mut self, input: InputFrame) -> (u32, InputFrame) {
        let frame = self.next_local;
        self.next_local += 1;
        self.push_remote(self.local_index, frame, input);
        (frame, input)
    }

    /// Stores another player's input. Inputs for frames already run, and repeats, are ignored.
    pub fn push_remote(&mut self, player: u8, frame: u32, input: InputFrame) {
        if player >= self.player_count || frame < self.frame || self.gone_at(player, frame) {
            return;
        }
        let slot = &mut self.inputs.entry(frame).or_insert([None; MAX_PLAYERS])[player as usize];
        if slot.is_none() {
            *slot = Some(input);
        }
    }

    /// Every player's input for the next frame (by player index), once all are in; a player
    /// who left holds nothing.
    pub fn ready(&self) -> Option<Vec<InputFrame>> {
        let row = self.inputs.get(&self.frame);
        (0..self.player_count)
            .map(|p| match row.and_then(|row| row[p as usize]) {
                Some(input) => Some(input),
                None if self.gone_at(p, self.frame) => Some(InputFrame::default()),
                None => None,
            })
            .collect()
    }

    fn gone_at(&self, player: u8, frame: u32) -> bool {
        self.gone[player as usize].is_some_and(|from| from <= frame)
    }

    /// `player` left: from frame `from` on they hold nothing (inputs already here for those
    /// frames are dropped). Every PC must be told the same frame (the host decides it).
    pub fn leave(&mut self, player: u8, from: u32) {
        if player >= self.player_count || self.gone[player as usize].is_some() {
            return;
        }
        self.gone[player as usize] = Some(from);
        for (_, row) in self.inputs.range_mut(from..) {
            row[player as usize] = None;
        }
    }

    /// The host's pick for a leaving player: the frame after the last input it has from them
    /// (never one already run).
    pub fn leave_frame(&self, player: u8) -> u32 {
        let last = self.inputs.iter().rev().find(|(_, row)| row[player as usize].is_some());
        last.map_or(self.frame, |(&frame, _)| (frame + 1).max(self.frame))
    }

    pub fn is_gone(&self, player: u8) -> bool {
        self.gone[player as usize].is_some()
    }

    /// Players who leave at the next frame (for the game's `leave` event).
    pub fn leaving_now(&self) -> Vec<u8> {
        (0..self.player_count).filter(|&p| self.gone[p as usize] == Some(self.frame)).collect()
    }

    /// Call after running the frame `ready()` returned.
    pub fn advance(&mut self) {
        self.inputs.remove(&self.frame);
        self.frame += 1;
    }

    pub fn frame(&self) -> u32 {
        self.frame
    }

    /// Players whose input for the next frame hasn't arrived.
    pub fn waiting_on(&self) -> Vec<u8> {
        let row = self.inputs.get(&self.frame);
        (0..self.player_count)
            .filter(|&p| !self.gone_at(p, self.frame) && row.is_none_or(|row| row[p as usize].is_none()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(buttons: u16) -> InputFrame {
        InputFrame {
            buttons,
            ..Default::default()
        }
    }

    /// A player who left holds nothing from their leave frame on and is never waited for; their
    /// late inputs (and ones already here for those frames) are dropped.
    #[test]
    fn a_player_who_left_holds_nothing() {
        let mut lockstep = Lockstep::new(3, 0, 0);
        for frame in 0..3 {
            lockstep.push_remote(0, frame, key(UP));
            lockstep.push_remote(2, frame, key(FIRE));
        }
        lockstep.push_remote(1, 0, key(LEFT));
        lockstep.push_remote(1, 1, key(LEFT));
        lockstep.push_remote(1, 2, key(LEFT)); // dropped: the test has them leave at 2
        assert_eq!(lockstep.leave_frame(1), 3, "after their last input");
        lockstep.leave(1, 2);
        assert_eq!(lockstep.ready().map(|i| i[1].buttons), Some(LEFT), "frame 0 still has their input");
        lockstep.advance();
        lockstep.advance();
        assert_eq!(lockstep.leaving_now(), vec![1]);
        assert_eq!(lockstep.ready().map(|i| i[1]), Some(InputFrame::default()), "frame 2: gone, input dropped");
        lockstep.advance();
        lockstep.push_remote(1, 3, key(LEFT));
        lockstep.push_remote(0, 3, key(UP));
        assert_eq!(lockstep.waiting_on(), vec![2], "never waits for them");
        lockstep.push_remote(2, 3, key(FIRE));
        assert_eq!(lockstep.ready().map(|i| i[1]), Some(InputFrame::default()), "late input ignored");
        assert!(lockstep.is_gone(1));
        assert_eq!(lockstep.leaving_now(), Vec::<u8>::new());
    }

    #[test]
    fn typed_holds_eight_characters_and_text_keys_round_trip() {
        let mut typed = Typed::default();
        for c in "abcdefgh".chars() {
            assert!(typed.push(c));
        }
        assert!(!typed.push('i'), "full");
        assert_eq!(typed.chars().iter().collect::<String>(), "abcdefgh");
        for code in TEXT_CONTROLS {
            let c = text_control_char(code);
            assert!(is_text_control(c));
            assert!(matches!(typed_event(c), ruffle_core::PlayerEvent::TextControl { code: back } if back == code));
        }
        assert!(!is_text_control('a'));
        assert!(matches!(typed_event('é'), ruffle_core::PlayerEvent::TextInput { codepoint: 'é' }));
    }

    #[test]
    fn waits_for_all_players() {
        let mut ls = Lockstep::new(2, 0, 1);
        // frame 0 is prefilled for everyone
        assert!(ls.ready().is_some());
        ls.advance();
        ls.push_local(key(UP));
        assert!(ls.ready().is_none());
        ls.push_remote(1, 1, key(DOWN));
        assert_eq!(ls.ready(), Some(vec![key(UP), key(DOWN)]));
    }

    #[test]
    fn delay_tags_future_frame() {
        let mut ls = Lockstep::new(2, 0, 3);
        let (frame, input) = ls.push_local(key(FIRE));
        assert_eq!(frame, 3);
        assert_eq!(input, key(FIRE));
    }

    #[test]
    fn four_players() {
        let mut ls = Lockstep::new(4, 2, 0);
        ls.push_local(key(LEFT));
        ls.push_remote(0, 0, key(0));
        ls.push_remote(1, 0, key(0));
        assert!(ls.ready().is_none());
        ls.push_remote(3, 0, key(RIGHT));
        assert_eq!(ls.ready(), Some(vec![key(0), key(0), key(LEFT), key(RIGHT)]));
    }

    #[test]
    fn burst_catch_up() {
        let mut ls = Lockstep::new(2, 0, 3);
        for _ in 0..8 {
            ls.push_local(key(0));
        }
        // nothing from the friend yet: only the 3 prefilled frames can run
        let mut ran = 0;
        while ls.ready().is_some() {
            ls.advance();
            ran += 1;
        }
        assert_eq!(ran, 3);
        // the friend's inputs for frames 3..=10 arrive at once
        for frame in 3..=10 {
            ls.push_remote(1, frame, key(UP));
        }
        while ls.ready().is_some() {
            ls.advance();
            ran += 1;
        }
        assert_eq!(ran, 11);
        assert_eq!(ls.frame(), 11);
    }

    #[test]
    fn waiting_on_names_missing() {
        let mut ls = Lockstep::new(3, 0, 0);
        ls.push_local(key(0));
        ls.push_remote(2, 0, key(0));
        assert_eq!(ls.waiting_on(), vec![1]);
    }

    #[test]
    fn old_and_duplicate_inputs_are_ignored() {
        let mut ls = Lockstep::new(2, 0, 0);
        ls.push_local(key(0));
        ls.push_remote(1, 0, key(UP));
        ls.push_remote(1, 0, key(DOWN)); // a duplicate must not replace the first
        assert_eq!(ls.ready(), Some(vec![key(0), key(UP)]));
        ls.advance();
        ls.push_remote(1, 0, key(LEFT)); // already played: dropped
        assert_eq!(ls.waiting_on(), vec![0, 1]);
    }
}
