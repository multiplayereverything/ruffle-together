//! TV mode: the launcher's TV mode starts Ruffle with `RUFFLE_TOGETHER_TV=1` (and `--fullscreen`),
//! and a controller plays the game, so a player on the couch never needs a keyboard or mouse.
//!
//! The controller presses the key map's buttons (UP, FIRE, KEY1, ...), which the game's key map
//! turns into its own keys for this PC's slot, exactly as this PC's keyboard does. In a session only
//! those buttons travel, so nothing here can make the PCs' games differ.
//!
//! | Controller (Xbox names) | Button |
//! |---|---|
//! | D-pad, left stick | UP DOWN LEFT RIGHT |
//! | A, B, X, Y | FIRE, ALT, KEY1, KEY2 |
//! | RB, LB | KEY3, KEY4 |
//! | RT, LT | YES, NO (RT clicks instead when the game has no YES) |
//! | View | VIEW |
//! | Menu (Start), tapped | MENU: the game's own pause |
//! | Menu (Start), held 1 s | the TV menu: Resume, Quit to launcher |
//! | LS (L3) | QUIT |
//! | right stick, RS (R3) | this PC's mouse, and its click |
//!
//! The controller only counts while Ruffle's window has focus.

use crate::coop::lockstep::{ALT, DOWN, FIRE, KEY1, KEY2, KEY3, KEY4, LEFT, MENU, NO, QUIT, RIGHT, UP, VIEW, YES};
use std::collections::HashSet;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

/// How long Start is held to open the TV menu.
pub const HOLD_FOR_MENU: Duration = Duration::from_millis(1000);
/// How long a tapped Start holds MENU (a few frames, so every game sees it).
const TAP: Duration = Duration::from_millis(150);
/// How far a stick goes before it counts as the D-pad.
const STICK: f32 = 0.5;
const POINTER_DEADZONE: f32 = 0.2;
/// Window widths a second, with the right stick all the way over.
const POINTER_SPEED: f64 = 0.8;
const POINTER_SHOWN: Duration = Duration::from_secs(3);

pub const MENU_ITEMS: [&str; 2] = ["Resume", "Quit to launcher"];

/// A controller button, by where it is (named as on an Xbox controller: A is the bottom one).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pad {
    A,
    B,
    X,
    Y,
    Lb,
    Rb,
    Lt,
    Rt,
    View,
    Start,
    L3,
    R3,
    Up,
    Down,
    Left,
    Right,
}

impl Pad {
    fn named(name: &str) -> Option<Self> {
        use Pad::*;
        Some(match name {
            "a" => A,
            "b" => B,
            "x" => X,
            "y" => Y,
            "lb" => Lb,
            "rb" => Rb,
            "lt" => Lt,
            "rt" => Rt,
            "view" => View,
            "start" => Start,
            "l3" => L3,
            "r3" => R3,
            "up" => Up,
            "down" => Down,
            "left" => Left,
            "right" => Right,
            _ => return None,
        })
    }
}

/// A stick axis; up and right are positive (as gilrs reports them).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stick {
    LeftX,
    LeftY,
    RightX,
    RightY,
}

/// Which controller's button names the hints use (`RUFFLE_TOGETHER_PAD`, or a guess from the
/// controller's name).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Xbox,
    PlayStation,
    Nintendo,
}

impl Kind {
    pub fn named(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "xbox" => Some(Self::Xbox),
            "playstation" | "ps" => Some(Self::PlayStation),
            "nintendo" | "switch" => Some(Self::Nintendo),
            _ => None,
        }
    }

    pub fn guess(controller: &str) -> Self {
        let name = controller.to_lowercase();
        let has = |words: &[&str]| words.iter().any(|w| name.contains(w));
        // Sony's own pads call themselves "Wireless Controller", so does Microsoft's: Xbox first
        if has(&["xbox", "xinput"]) {
            Self::Xbox
        } else if has(&["playstation", "dualsense", "dualshock", "ps3", "ps4", "ps5", "wireless controller"]) {
            Self::PlayStation
        } else if has(&["nintendo", "switch", "pro controller", "joy-con"]) {
            Self::Nintendo
        } else {
            Self::Xbox
        }
    }

    /// What the button in this place is called on this controller.
    pub fn label(self, pad: Pad) -> &'static str {
        use Pad::*;
        match pad {
            Up => "Up",
            Down => "Down",
            Left => "Left",
            Right => "Right",
            _ => {
                let names = match self {
                    Self::Xbox => ["A", "B", "X", "Y", "LB", "RB", "LT", "RT", "View", "Menu", "LS", "RS"],
                    Self::PlayStation => ["Cross", "Circle", "Square", "Triangle", "L1", "R1", "L2", "R2", "Create", "Options", "L3", "R3"],
                    // Nintendo's A is on the right, where Xbox has B
                    Self::Nintendo => ["B", "A", "Y", "X", "L", "R", "ZL", "ZR", "-", "+", "LS", "RS"],
                };
                let place = [A, B, X, Y, Lb, Rb, Lt, Rt, View, Start, L3, R3].iter().position(|p| *p == pad).unwrap_or(0);
                names[place]
            }
        }
    }
}

/// What the controller does outside the game's buttons: this PC's mouse, and leaving.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    MouseMove { x: f64, y: f64 },
    MouseDown { x: f64, y: f64 },
    MouseUp { x: f64, y: f64 },
    Quit,
}

/// One controller (every connected one counts as this one) and the TV menu.
pub struct Tv {
    kind: Kind,
    /// `RUFFLE_TOGETHER_PAD` chose the kind: a controller's name doesn't change it.
    kind_forced: bool,
    held: HashSet<Pad>,
    sticks: [f32; 4],
    focused: bool,
    start_since: Option<Instant>,
    /// This press of Start opened or closed the TV menu: its release is not a tap.
    start_used: bool,
    menu_until: Option<Instant>,
    /// The TV menu is open, on this item.
    menu: Option<usize>,
    /// Where the left stick points in the TV menu (-1 up, 1 down), so one push moves one item.
    menu_stick: i8,
    /// This PC's mouse, in window pixels, once the right stick has moved it.
    pointer: Option<(f64, f64)>,
    pointer_used: Option<Instant>,
    clicking: bool,
    last_update: Option<Instant>,
    actions: Vec<Action>,
    script: Option<PadScript>,
}

impl Tv {
    pub fn new(kind: Option<Kind>) -> Self {
        Self {
            kind: kind.unwrap_or_default(),
            kind_forced: kind.is_some(),
            held: HashSet::new(),
            sticks: [0.0; 4],
            focused: true,
            start_since: None,
            start_used: false,
            menu_until: None,
            menu: None,
            menu_stick: 0,
            pointer: None,
            pointer_used: None,
            clicking: false,
            last_update: None,
            actions: vec![],
            script: None,
        }
    }

    /// Plays the pad script's presses that are due.
    pub fn run_script(&mut self, now: Instant) {
        let due = self.script.as_mut().map(|script| script.due(now)).unwrap_or_default();
        for event in due {
            match event {
                PadEvent::Button(pad, down) => self.button(pad, down, now),
                PadEvent::Stick(stick, value) => self.stick(stick, value),
            }
        }
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// A controller with this name is connected: its button names, unless `RUFFLE_TOGETHER_PAD` chose.
    pub fn connected(&mut self, name: &str) {
        if !self.kind_forced {
            self.kind = Kind::guess(name);
        }
    }

    /// The TV menu's selected item, while it is open.
    pub fn menu(&self) -> Option<usize> {
        self.menu
    }

    /// Where to draw this PC's mouse (window pixels), for a few seconds after the right stick
    /// moved it or it clicked: the system's own pointer doesn't move with it.
    pub fn pointer(&self, now: Instant) -> Option<(f64, f64)> {
        self.pointer.filter(|_| self.clicking || self.pointer_used.is_some_and(|at| now.saturating_duration_since(at) < POINTER_SHOWN))
    }

    pub fn button(&mut self, pad: Pad, down: bool, now: Instant) {
        if down && !self.focused {
            return;
        }
        if down {
            self.held.insert(pad);
        } else {
            self.held.remove(&pad);
        }
        if let Some(item) = self.menu {
            if down {
                match pad {
                    Pad::Up => self.menu = Some(item.saturating_sub(1)),
                    Pad::Down => self.menu = Some((item + 1).min(MENU_ITEMS.len() - 1)),
                    Pad::A => self.pick(item),
                    Pad::B => self.menu = None,
                    Pad::Start => {
                        self.menu = None;
                        self.start_used = true;
                    }
                    _ => {}
                }
            }
            if pad == Pad::Start && !down {
                self.start_since = None;
            }
            return;
        }
        if pad == Pad::Start {
            if down {
                self.start_since = Some(now);
                self.start_used = false;
            } else if self.start_since.take().is_some() && !self.start_used {
                self.menu_until = Some(now + TAP);
            }
        }
    }

    pub fn stick(&mut self, stick: Stick, value: f32) {
        self.sticks[stick as usize] = if self.focused { value } else { 0.0 };
    }

    pub fn focus(&mut self, focused: bool) {
        self.focused = focused;
        if !focused {
            self.held.clear();
            self.sticks = [0.0; 4];
            self.start_since = None;
            self.release_click();
        }
    }

    fn pick(&mut self, item: usize) {
        self.menu = None;
        if item == 1 {
            self.actions.push(Action::Quit);
        }
    }

    fn release_click(&mut self) {
        if std::mem::take(&mut self.clicking)
            && let Some((x, y)) = self.pointer
        {
            self.actions.push(Action::MouseUp { x, y });
        }
    }

    /// Runs the parts that take time: holding Start, the TV menu's stick, the mouse. `window` is
    /// the window's size in pixels; `uses` the buttons the game's key map has.
    pub fn update(&mut self, now: Instant, window: (f64, f64), uses: u16) {
        let dt = self.last_update.map_or(0.0, |at| now.saturating_duration_since(at).as_secs_f64().min(0.1));
        self.last_update = Some(now);
        if self.menu.is_none()
            && !self.start_used
            && self.start_since.is_some_and(|since| now.saturating_duration_since(since) >= HOLD_FOR_MENU)
        {
            self.menu = Some(0);
            self.start_used = true;
            self.release_click();
        }
        if let Some(item) = self.menu {
            let ly = self.sticks[Stick::LeftY as usize];
            let point = if ly > STICK { -1 } else if ly < -STICK { 1 } else { 0 };
            if point != self.menu_stick {
                self.menu_stick = point;
                if point < 0 {
                    self.menu = Some(item.saturating_sub(1));
                } else if point > 0 {
                    self.menu = Some((item + 1).min(MENU_ITEMS.len() - 1));
                }
            }
            return;
        }
        let (width, height) = (window.0.max(1.0), window.1.max(1.0));
        let (rx, ry) = (self.sticks[Stick::RightX as usize], self.sticks[Stick::RightY as usize]);
        if rx.hypot(ry) > POINTER_DEADZONE {
            let (x, y) = self.pointer.unwrap_or((width / 2.0, height / 2.0));
            let speed = POINTER_SPEED * width * dt;
            let to = ((x + rx as f64 * speed).clamp(0.0, width - 1.0), (y - ry as f64 * speed).clamp(0.0, height - 1.0));
            if self.pointer != Some(to) {
                self.pointer = Some(to);
                self.actions.push(Action::MouseMove { x: to.0, y: to.1 });
            }
            self.pointer_used = Some(now);
        }
        let click = self.held.contains(&Pad::R3) || (uses & YES == 0 && self.held.contains(&Pad::Rt));
        if click != self.clicking {
            let (x, y) = match self.pointer {
                Some(at) => at,
                None => {
                    let at = (width / 2.0, height / 2.0);
                    self.pointer = Some(at);
                    self.actions.push(Action::MouseMove { x: at.0, y: at.1 });
                    at
                }
            };
            self.clicking = click;
            self.pointer_used = Some(now);
            self.actions.push(if click { Action::MouseDown { x, y } } else { Action::MouseUp { x, y } });
        }
    }

    /// The game's buttons this controller holds now (none while the TV menu is open).
    pub fn buttons(&self, now: Instant, uses: u16) -> u16 {
        if !self.focused || self.menu.is_some() {
            return 0;
        }
        let held = |pad| self.held.contains(&pad);
        let (lx, ly) = (self.sticks[Stick::LeftX as usize], self.sticks[Stick::LeftY as usize]);
        let mut buttons = 0;
        for (on, button) in [
            (held(Pad::Up) || ly > STICK, UP),
            (held(Pad::Down) || ly < -STICK, DOWN),
            (held(Pad::Left) || lx < -STICK, LEFT),
            (held(Pad::Right) || lx > STICK, RIGHT),
            (held(Pad::A), FIRE),
            (held(Pad::B), ALT),
            (held(Pad::X), KEY1),
            (held(Pad::Y), KEY2),
            (held(Pad::Rb), KEY3),
            (held(Pad::Lb), KEY4),
            (held(Pad::Rt) && uses & YES != 0, YES),
            (held(Pad::Lt), NO),
            (held(Pad::View), VIEW),
            (held(Pad::L3), QUIT),
            (self.menu_until.is_some_and(|until| now < until), MENU),
        ] {
            if on {
                buttons |= button;
            }
        }
        buttons
    }

    pub fn take_actions(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.actions)
    }
}

static TV: OnceLock<Mutex<Tv>> = OnceLock::new();

/// The launcher asked for TV mode (`RUFFLE_TOGETHER_TV=1`).
pub fn requested() -> bool {
    std::env::var("RUFFLE_TOGETHER_TV").is_ok_and(|value| value.trim() == "1")
}

/// Turns TV mode on, for the rest of this process, with a controller played from a script if
/// there is one.
pub fn start(script: Option<PadScript>) {
    let kind = std::env::var("RUFFLE_TOGETHER_PAD").ok().and_then(|name| Kind::named(name.trim()));
    let mut tv = Tv::new(kind);
    tv.script = script;
    let _ = TV.set(Mutex::new(tv));
}

/// TV mode's controller, if TV mode is on.
pub fn tv() -> Option<MutexGuard<'static, Tv>> {
    TV.get().and_then(|tv| tv.lock().ok())
}

pub fn on() -> bool {
    TV.get().is_some()
}

/// How much bigger Ruffle Together's own text is, to read from a sofa.
pub fn text_scale() -> f32 {
    if on() { 2.0 } else { 1.0 }
}

/// What the controller does in an input script (`--coop-pad-script`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PadEvent {
    Button(Pad, bool),
    Stick(Stick, f32),
}

/// Testing (`--coop-pad-script FILE`): a controller played from a file, for labs without one. Times
/// count from when the window opens.
///
/// ```text
/// wait 3000              nothing for 3000 ms
/// press start 1200       hold Start for 1200 ms, then 100 ms with nothing
/// press a                a tap (100 ms)
/// hold right / release right
/// stick right 1 0 500    the right stick at x 1, y 0 for 500 ms (left / right; up is +y)
/// ```
pub struct PadScript {
    events: Vec<(Duration, PadEvent)>,
    next: usize,
    start: Option<Instant>,
}

impl PadScript {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut events = vec![];
        let mut at = Duration::ZERO;
        // a file saved by Notepad starts with a byte order mark
        for (number, line) in text.trim_start_matches('\u{feff}').lines().enumerate() {
            let words: Vec<&str> = line.split('#').next().unwrap_or_default().split_whitespace().collect();
            let fail = |why: &str| format!("pad script line {}: {why}", number + 1);
            let ms = |word: Option<&&str>, default: u64| match word {
                Some(w) => w.parse::<u64>().map(Duration::from_millis).map_err(|_| fail("expected milliseconds")),
                None => Ok(Duration::from_millis(default)),
            };
            let pad = |word: Option<&&str>| word.and_then(|w| Pad::named(&w.to_ascii_lowercase())).ok_or_else(|| fail("unknown button"));
            match words.first().copied() {
                None => {}
                Some("wait") => at += ms(words.get(1), 0)?,
                Some("press") => {
                    let (pad, long) = (pad(words.get(1))?, ms(words.get(2), 100)?);
                    events.push((at, PadEvent::Button(pad, true)));
                    events.push((at + long, PadEvent::Button(pad, false)));
                    at += long + Duration::from_millis(100);
                }
                Some("hold") => events.push((at, PadEvent::Button(pad(words.get(1))?, true))),
                Some("release") => events.push((at, PadEvent::Button(pad(words.get(1))?, false))),
                Some("stick") => {
                    let (sx, sy) = match words.get(1).copied() {
                        Some("left") => (Stick::LeftX, Stick::LeftY),
                        Some("right") => (Stick::RightX, Stick::RightY),
                        _ => return Err(fail("stick left or stick right")),
                    };
                    let value = |word: Option<&&str>| {
                        word.and_then(|w| w.parse::<f32>().ok()).filter(|v| (-1.0..=1.0).contains(v)).ok_or_else(|| fail("expected -1 to 1"))
                    };
                    let (x, y) = (value(words.get(2))?, value(words.get(3))?);
                    events.push((at, PadEvent::Stick(sx, x)));
                    events.push((at, PadEvent::Stick(sy, y)));
                    if let Some(word) = words.get(4) {
                        at += ms(Some(word), 0)?;
                        events.push((at, PadEvent::Stick(sx, 0.0)));
                        events.push((at, PadEvent::Stick(sy, 0.0)));
                    }
                }
                Some(other) => return Err(fail(&format!("unknown command {other}"))),
            }
        }
        events.sort_by_key(|(at, _)| *at);
        Ok(Self { events, next: 0, start: None })
    }

    /// The events due by `now` (the first call starts the clock).
    pub fn due(&mut self, now: Instant) -> Vec<PadEvent> {
        let start = *self.start.get_or_insert(now);
        let elapsed = now.saturating_duration_since(start);
        let mut due = vec![];
        while let Some(&(at, event)) = self.events.get(self.next)
            && at <= elapsed
        {
            due.push(event);
            self.next += 1;
        }
        due
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    const WINDOW: (f64, f64) = (1000.0, 500.0);

    #[test]
    fn the_controller_presses_the_key_maps_buttons() {
        let now = Instant::now();
        let mut tv = Tv::new(None);
        tv.button(Pad::A, true, now);
        tv.button(Pad::Rb, true, now);
        tv.stick(Stick::LeftX, -0.9);
        assert_eq!(tv.buttons(now, 0), FIRE | KEY3 | LEFT);
        tv.stick(Stick::LeftX, -0.3);
        assert_eq!(tv.buttons(now, 0), FIRE | KEY3, "a small push is not the D-pad");
        tv.button(Pad::A, false, now);
        tv.button(Pad::Rb, false, now);
        tv.button(Pad::Up, true, now);
        tv.button(Pad::View, true, now);
        assert_eq!(tv.buttons(now, 0), UP | VIEW);
        tv.button(Pad::Rt, true, now);
        assert_eq!(tv.buttons(now, YES) & YES, YES, "RT is YES in a game that has one");
        assert_eq!(tv.buttons(now, 0) & YES, 0);
    }

    #[test]
    fn a_tapped_start_pauses_and_a_held_one_opens_the_menu() {
        let t0 = Instant::now();
        let mut tv = Tv::new(None);
        tv.button(Pad::Start, true, t0);
        tv.update(t0 + ms(200), WINDOW, 0);
        assert_eq!(tv.buttons(t0 + ms(200), 0), 0, "MENU waits for the release");
        tv.button(Pad::Start, false, t0 + ms(300));
        assert_eq!(tv.buttons(t0 + ms(350), 0), MENU, "a tap is the game's pause");
        assert_eq!(tv.buttons(t0 + ms(500), 0), 0, "for a few frames");

        let t1 = t0 + ms(1000);
        tv.button(Pad::Start, true, t1);
        tv.button(Pad::A, true, t1);
        tv.update(t1 + ms(900), WINDOW, 0);
        assert_eq!(tv.menu(), None);
        assert_eq!(tv.buttons(t1 + ms(900), 0), FIRE);
        tv.update(t1 + HOLD_FOR_MENU, WINDOW, 0);
        assert_eq!(tv.menu(), Some(0), "held for a second: the TV menu, on Resume");
        assert_eq!(tv.buttons(t1 + HOLD_FOR_MENU, 0), 0, "the game's buttons let go while it is open");
        tv.button(Pad::Start, false, t1 + ms(1200));
        assert_eq!(tv.buttons(t1 + ms(1250), 0), 0, "releasing that Start doesn't pause the game");
        tv.button(Pad::A, false, t1 + ms(1300));
        tv.button(Pad::B, true, t1 + ms(1400));
        assert_eq!(tv.menu(), None, "B closes it");
        assert!(tv.take_actions().is_empty());
    }

    #[test]
    fn quit_to_launcher() {
        let t0 = Instant::now();
        let mut tv = Tv::new(None);
        tv.button(Pad::Start, true, t0);
        tv.update(t0 + HOLD_FOR_MENU, WINDOW, 0);
        tv.button(Pad::Start, false, t0 + HOLD_FOR_MENU);
        tv.button(Pad::Down, true, t0 + ms(1100));
        tv.button(Pad::Down, false, t0 + ms(1150));
        tv.button(Pad::Down, true, t0 + ms(1200));
        assert_eq!(tv.menu(), Some(1), "the last item stays the last");
        tv.button(Pad::Up, true, t0 + ms(1250));
        assert_eq!(tv.menu(), Some(0));
        tv.stick(Stick::LeftY, -1.0);
        tv.update(t0 + ms(1300), WINDOW, 0);
        tv.update(t0 + ms(1350), WINDOW, 0);
        assert_eq!(tv.menu(), Some(1), "one push of the stick, one item");
        tv.button(Pad::A, true, t0 + ms(1400));
        assert_eq!(tv.take_actions(), [Action::Quit]);
        assert_eq!(tv.menu(), None);
    }

    #[test]
    fn the_right_stick_is_the_mouse() {
        let t0 = Instant::now();
        let mut tv = Tv::new(None);
        tv.update(t0, WINDOW, 0);
        tv.stick(Stick::RightX, 1.0);
        tv.update(t0 + ms(100), WINDOW, 0);
        let Some(Action::MouseMove { x, y }) = tv.take_actions().first().copied() else {
            panic!("the stick moves the mouse");
        };
        assert_eq!((x, y), (500.0 + 0.8 * 1000.0 * 0.1, 250.0), "from the middle, 0.8 window widths a second");
        tv.stick(Stick::RightX, 0.1);
        tv.update(t0 + ms(200), WINDOW, 0);
        assert!(tv.take_actions().is_empty(), "a resting stick stays put");
        tv.button(Pad::Rt, true, t0 + ms(300));
        tv.update(t0 + ms(300), WINDOW, 0);
        assert_eq!(tv.take_actions(), [Action::MouseDown { x, y }], "RT clicks in a game without YES");
        tv.button(Pad::Rt, false, t0 + ms(400));
        tv.update(t0 + ms(400), WINDOW, 0);
        assert_eq!(tv.take_actions(), [Action::MouseUp { x, y }]);
        tv.button(Pad::Rt, true, t0 + ms(500));
        tv.update(t0 + ms(500), WINDOW, YES);
        assert!(tv.take_actions().is_empty(), "in a game with YES, RT is YES");
        tv.button(Pad::R3, true, t0 + ms(600));
        tv.update(t0 + ms(600), WINDOW, YES);
        assert_eq!(tv.take_actions(), [Action::MouseDown { x, y }], "R3 always clicks");
        tv.focus(false);
        assert_eq!(tv.take_actions(), [Action::MouseUp { x, y }], "losing focus lets go");
    }

    #[test]
    fn only_while_the_window_has_focus() {
        let now = Instant::now();
        let mut tv = Tv::new(None);
        tv.button(Pad::A, true, now);
        tv.focus(false);
        assert_eq!(tv.buttons(now, 0), 0);
        tv.button(Pad::B, true, now);
        tv.stick(Stick::LeftX, 1.0);
        tv.focus(true);
        assert_eq!(tv.buttons(now, 0), 0, "presses made while another window was in front don't count");
    }

    #[test]
    fn button_names() {
        assert_eq!(Kind::guess("Xbox Wireless Controller"), Kind::Xbox);
        assert_eq!(Kind::guess("DualSense Wireless Controller"), Kind::PlayStation);
        assert_eq!(Kind::guess("Nintendo Switch Pro Controller"), Kind::Nintendo);
        assert_eq!(Kind::guess("Some Gamepad"), Kind::Xbox);
        assert_eq!(Kind::PlayStation.label(Pad::A), "Cross");
        assert_eq!(Kind::Nintendo.label(Pad::B), "A");
        assert_eq!(Kind::Xbox.label(Pad::Start), "Menu");
        let mut tv = Tv::new(Kind::named("playstation"));
        tv.connected("Xbox Controller");
        assert_eq!(tv.kind(), Kind::PlayStation, "RUFFLE_TOGETHER_PAD wins");
    }

    #[test]
    fn pad_scripts() {
        let mut script = PadScript::parse("wait 1000\npress start 1200 # the menu\nstick right 1 -0.5 300\nhold a").expect("script");
        let t0 = Instant::now();
        assert!(script.due(t0).is_empty());
        assert_eq!(script.due(t0 + ms(1000)), [PadEvent::Button(Pad::Start, true)]);
        assert_eq!(script.due(t0 + ms(2200)), [PadEvent::Button(Pad::Start, false)]);
        assert_eq!(
            script.due(t0 + ms(2600)),
            [
                PadEvent::Stick(Stick::RightX, 1.0),
                PadEvent::Stick(Stick::RightY, -0.5),
                PadEvent::Stick(Stick::RightX, 0.0),
                PadEvent::Stick(Stick::RightY, 0.0),
                PadEvent::Button(Pad::A, true),
            ]
        );
        assert!(PadScript::parse("\u{feff}wait 10").is_ok(), "a byte order mark");
        assert!(PadScript::parse("press z").is_err());
        assert!(PadScript::parse("stick up 1 0").is_err());
        assert!(PadScript::parse("jump").is_err());
    }
}
