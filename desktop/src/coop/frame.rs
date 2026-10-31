//! Runs exactly one frame from every player's input, the same way on every PC. This replaces
//! Ruffle's clock-driven `tick` during a session: frames advance only when the inputs exist,
//! and timers and the frame clock move by exactly one frame each time.

use crate::coop::api::{CoopApi, CoopApiProvider};
use crate::coop::keymap::{Game, GameKeys};
use crate::coop::lockstep::{InputFrame, typed_event};
use crate::coop::mice::{Mice, MouseDebug, privates};
use ruffle_core::limits::ExecutionLimit;
use ruffle_core::{FloatDuration, Player, PlayerEvent};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct Stepper {
    keys: GameKeys,
    loaded: bool,
    /// The key map the game gave with `coop.keys`, if it gave one.
    game_keys: Option<String>,
    /// What the game can ask through ExternalInterface (every slot's mouse, the players).
    api: Arc<Mutex<CoopApi>>,
    mice: Mice,
}

impl Stepper {
    pub fn new(keys: GameKeys, players: u8, local_slot: u8) -> Self {
        let mut api = CoopApi::new(players, local_slot);
        apply_mouse_rules(&mut api, &keys);
        Self { keys, loaded: false, game_keys: None, api: Arc::new(Mutex::new(api)), mice: Mice::default() }
    }

    #[cfg(test)]
    pub fn api(&self) -> &Arc<Mutex<CoopApi>> {
        &self.api
    }

    /// `inputs` is every player's input for this frame, by player index.
    pub fn step(&mut self, player: &mut Player, inputs: &[InputFrame]) {
        if !self.loaded {
            // frames load part of a streaming movie within a time limit, so a slower PC would
            // load less per frame; load all of it before the first frame instead
            while !player.preload(&mut ExecutionLimit::none()) {}
            self.loaded = true;
            player.set_external_interface_provider(Some(Box::new(CoopApiProvider(self.api.clone()))));
            player.set_coop_local(self.api.lock().map_or(0, |api| api.local_slot));
        }
        if let Ok(mut api) = self.api.lock() {
            api.update(inputs);
        }
        // the host's mouse is always in the stage, wherever this PC's own cursor is
        player.set_mouse_in_stage(true);
        // Characters reach the game from one place only, or a key would type twice: while a text
        // box is selected, what the players typed (names, chat); otherwise each slot's key press
        // types its game key's character (on(keyPress "a") buttons), whatever key map it comes
        // from and whoever pressed it (keyboard, script, replay). Game state, so the same on
        // every PC.
        let text_box = player.coop_text_box_focused();
        for (slot, input) in inputs.iter().enumerate() {
            for event in self.keys.events(slot as u8, input.buttons) {
                if !(text_box && matches!(event, PlayerEvent::TextInput { .. })) {
                    player.handle_event(event);
                }
            }
            if text_box {
                for &c in input.text.chars() {
                    player.handle_event(typed_event(c));
                }
            }
        }
        self.mice.feed(player, &self.api, inputs);

        let frame = Duration::from_secs_f64(1.0 / player.frame_rate());
        player.run_frame();
        player.update_timers(FloatDuration::from_std(frame));
        self.apply_game_calls(player);
        self.mice.update_privates(player, &self.api);
        self.mice.after_frame(player, &self.api);
        player.audio_mut().tick();
        player.advance_frame_clock(frame);
        // rendering has side effects on the game (e.g. scrollRect updates), so it happens
        // exactly once per frame on every PC
        player.render();
    }

    /// The other players' cursors (slot, stage twips, button) for the window to draw.
    pub fn friend_cursors(&self, player: &mut Player) -> Vec<(u8, i32, i32, bool)> {
        self.mice.cursors(player, &self.api)
    }

    /// Every player's mouse, for the mouse debug overlay.
    pub fn mouse_debug(&self, player: &mut Player) -> Vec<MouseDebug> {
        self.mice.debug(player, &self.api)
    }

    /// Presses refused since the last call because another player had that private screen
    /// open: (who pressed, whose it is).
    pub fn take_busy(&mut self) -> Vec<(u8, u8)> {
        self.mice.take_busy()
    }

    /// Something the game hears through `coop.events` ("join 1", "leave 2", "chat 0 hi"...);
    /// every PC adds the same on the same frame.
    pub fn add_event(&mut self, event: String) {
        if let Ok(mut api) = self.api.lock() {
            api.add_event(event);
        }
    }

    /// Whether the game reads `coop.typed` (every typed character is wanted then).
    pub fn wants_text(&self) -> bool {
        self.api.lock().is_ok_and(|api| api.wants_text())
    }

    /// This PC's round trip to each player, for `coop.ping`.
    pub fn set_pings(&mut self, pings: [Option<u32>; crate::coop::lockstep::MAX_PLAYERS]) {
        if let Ok(mut api) = self.api.lock() {
            api.pings = pings;
        }
    }

    /// The host's game settings, for `coop.option`.
    pub fn set_options(&mut self, options: &[(String, String)]) {
        if let Ok(mut api) = self.api.lock() {
            api.options = options.to_vec();
        }
    }

    /// Every player's name, for `coop.name`.
    pub fn set_names(&mut self, names: &[String]) {
        if let Ok(mut api) = self.api.lock() {
            api.names = names.to_vec();
        }
    }

    /// The labels the game asked for, for the window to draw.
    pub fn labels(&self) -> Vec<crate::coop::api::Label> {
        self.api.lock().map(|api| api.labels.iter().flatten().cloned().collect()).unwrap_or_default()
    }

    /// What the game asked for during the frame (its scripts and timers): its own key map (every
    /// PC switches on the same frame, since every PC runs the same calls; the same map again
    /// changes nothing) and this PC's camera.
    fn apply_game_calls(&mut self, player: &mut Player) {
        let Ok(mut api) = self.api.lock() else {
            return;
        };
        if let Some(text) = api.keys.take()
            && self.game_keys.as_ref() != Some(&text)
            && let Ok(game) = Game::parse("game", &text)
        {
            self.game_keys = Some(text);
            for event in self.keys.release_all() {
                player.handle_event(event);
            }
            self.keys = game.keys();
            apply_mouse_rules(&mut api, &self.keys);
        }
        player.set_coop_camera(api.local_camera().map(|c| (c.x, c.y, c.zoom)));
    }

    /// Lets go of every key, e.g. when the session ends.
    pub fn release_all(&mut self, player: &mut Player) {
        for event in self.keys.release_all() {
            player.handle_event(event);
        }
    }
}

/// The mouse rules a key map gives; what it says nothing about stays as it is (a game's own
/// `coop.keys` map keeps the rules the game set).
fn apply_mouse_rules(api: &mut CoopApi, keys: &GameKeys) {
    if let Some(mode) = keys.mouse {
        api.mouse_mode = mode;
    }
    for &(slot, mode) in &keys.mouse_modes {
        api.mouse_modes[slot as usize] = Some(mode);
    }
    if !keys.zones.is_empty() {
        api.zones = keys.zones.clone();
    }
    if !keys.privates.is_empty() {
        api.privates = privates(&keys.privates);
    }
    if let Some(hover) = keys.hover {
        api.hover = hover;
    }
    if let Some(cursors) = keys.cursors {
        api.cursors = cursors;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coop::keymap::Game;
    use crate::coop::api::Camera;
    use crate::coop::lockstep::{FIRE, UP};
    use ruffle_core::PlayerBuilder;
    use ruffle_core::external::Value;
    use ruffle_core::frame_clock::FrameClock;
    use std::sync::{Arc, Mutex};

    #[test]
    fn step_advances_clock_by_exactly_one_frame() {
        let player = PlayerBuilder::new().build();
        let mut player = player.lock().expect("player");
        let clock = Arc::new(Mutex::new(FrameClock::new(0)));
        player.set_frame_clock(Some(clock.clone()));
        let frame = Duration::from_secs_f64(1.0 / player.frame_rate());

        let mut stepper = Stepper::new(Game::built_in("bad-ice-cream").expect("built in").keys(), 2, 0);
        let inputs = [InputFrame { buttons: UP, mouse: Some((200, 400, true)), ..Default::default() }, InputFrame::default()];
        stepper.step(&mut player, &inputs);
        stepper.step(&mut player, &inputs);

        assert_eq!(clock.lock().expect("clock").elapsed(), frame * 2);
    }

    /// Two PCs with different (or minimized, 1x1) windows must see the host's mouse at exactly
    /// the same stage spot, and inside the stage, whatever their own cursor does.
    #[test]
    fn host_mouse_is_the_same_on_every_pc() {
        let mut spots = vec![];
        for (width, height) in [(550, 550), (1, 1), (1280, 720)] {
            let player = PlayerBuilder::new().with_viewport_dimensions(width, height, 1.0).build();
            let mut player = player.lock().expect("player");
            player.set_mouse_in_stage(false); // this PC's own cursor is outside its window
            let mut stepper = Stepper::new(Game::built_in("bad-ice-cream").expect("built in").keys(), 2, 0);
            let inputs = [InputFrame { buttons: 0, mouse: Some((4001, 3999, false)), ..Default::default() }];
            stepper.step(&mut player, &inputs);
            assert!(player.mouse_in_stage(), "the host's mouse is always in the stage");
            spots.push(player.mouse_position_twips());
        }
        assert_eq!(spots, vec![(4001, 3999); 3]);
    }

    /// The co-op API answers through ExternalInterface; Ruffle's own AS2 and AS3 ExternalInterface
    /// tests show that both kinds of game reach it in a session.
    #[test]
    fn as2_and_as3_reach_the_coop_api() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/tests/swfs");
        for swf in ["avm1/external_interface/test.swf", "avm2/external_interface/test.swf"] {
            let player = crate::coop::replay::headless_player(
                &root.join(swf),
                None,
                crate::coop::storage::SessionStorage::default(),
            )
            .expect("player");
            let mut player = player.lock().expect("player");
            let mut stepper = Stepper::new(Game::built_in("bad-ice-cream").expect("built in").keys(), 2, 0);
            for _ in 0..5 {
                stepper.step(&mut player, &[InputFrame::default(), InputFrame::default()]);
            }
            let calls = stepper.api().lock().expect("api").calls.clone();
            assert!(calls.contains(&"ping".to_string()), "{swf}: {calls:?}");
        }
    }

    struct Traces(Arc<Mutex<Vec<String>>>);

    impl ruffle_core::backend::log::LogBackend for Traces {
        fn avm_trace(&self, message: &str) {
            self.0.lock().expect("traces").push(message.to_string());
        }

        fn avm_warning(&self, _message: &str) {}
    }

    /// The frames at which Ruffle's own AVM2 sound-complete test SWF (two embedded sounds) sees
    /// each channel's soundComplete, without an audio device (as on a PC without one).
    fn sound_complete_frames(session: bool) -> Vec<(usize, String)> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/tests/swfs");
        let movie = ruffle_core::tag_utils::movie_from_path(
            root.join("avm2/soundchannel_soundcomplete/test.swf"),
            None,
        )
        .expect("test swf");
        let traces = Arc::new(Mutex::new(vec![]));
        let player = PlayerBuilder::new()
            .with_movie(movie)
            .with_log(Traces(traces.clone()))
            .with_autoplay(true)
            .build();
        let mut player = player.lock().expect("player");
        if session {
            player.set_frame_clock(Some(Arc::new(Mutex::new(FrameClock::new(0)))));
        }
        let mut stepper = Stepper::new(Game::built_in("bad-ice-cream").expect("built in").keys(), 2, 0);
        let mut seen = vec![];
        for frame in 0..300 {
            stepper.step(&mut player, &[InputFrame::default(), InputFrame::default()]);
            for line in traces.lock().expect("traces").drain(..) {
                if line.contains("soundComplete") {
                    seen.push((frame, line));
                }
            }
        }
        seen
    }

    /// A sound's end follows the frame clock in a session, so soundComplete / onSoundComplete
    /// fire at the same frame on every PC, whatever (or whether) its audio device plays.
    #[test]
    fn sounds_complete_on_the_frame_clock() {
        let seen = sound_complete_frames(true);
        let channels: Vec<_> = seen.iter().map(|(_, line)| line.split(':').next().unwrap_or("")).collect();
        assert_eq!(channels, ["noise_channel", "silence_channel"], "{seen:?}");
        assert_eq!(seen, sound_complete_frames(true), "the same frames every time");

        // control: outside a session a PC without an audio device never ends a sound
        assert_eq!(sound_complete_frames(false), vec![]);
    }

    /// Streaming loads only part of a movie per frame, as fast as the PC allows; the first
    /// step loads all of it, so every PC starts from the same fully loaded movie.
    #[test]
    fn first_step_loads_the_whole_movie() {
        let Ok(swf) = std::env::var("COOP_TEST_SWF") else {
            eprintln!("skipping: COOP_TEST_SWF not set");
            return;
        };
        let player = crate::coop::replay::headless_player(
            std::path::Path::new(&swf),
            None,
            crate::coop::storage::SessionStorage::default(),
        )
        .expect("player");
        let mut player = player.lock().expect("player");
        let mut stepper = Stepper::new(Game::built_in("bad-ice-cream").expect("built in").keys(), 2, 0);
        stepper.step(&mut player, &[InputFrame::default(), InputFrame::default()]);
        assert!(player.preload(&mut ruffle_core::limits::ExecutionLimit::exhausted()));
    }

    /// Ruffle's own AS2 text-input test: a selected text box, traced on Escape. Two players type
    /// into it (in slot order, on the same frame), Backspace included, then Escape shows it.
    #[test]
    fn every_players_typing_reaches_the_selected_text_box() {
        use crate::coop::lockstep::{Typed, text_control_char};
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/tests/swfs");
        let movie =
            ruffle_core::tag_utils::movie_from_path(root.join("avm1/edittext_input/test.swf"), None).expect("test swf");
        let traces = Arc::new(Mutex::new(vec![]));
        let player = PlayerBuilder::new()
            .with_movie(movie)
            .with_log(Traces(traces.clone()))
            .with_autoplay(true)
            .build();
        let mut player = player.lock().expect("player");
        let escape = Game::parse("t", "slot 0: FIRE=27\nslot 1: FIRE=27").expect("map").keys();
        let mut stepper = Stepper::new(escape, 2, 0);
        stepper.step(&mut player, &[InputFrame::default(), InputFrame::default()]);
        assert!(player.coop_text_box_focused(), "the SWF selects its text box");
        let typed = |text: &str| InputFrame { text: Typed::from_chars(text.chars()), ..Default::default() };
        let backspace = Typed::from_chars([text_control_char(ruffle_core::events::TextControlCode::Backspace)]);
        stepper.step(&mut player, &[typed("Lia"), typed("xm")]);
        stepper.step(&mut player, &[InputFrame::default(), InputFrame { text: backspace, ..Default::default() }]);
        stepper.step(&mut player, &[typed("!"), InputFrame::default()]);
        stepper.step(&mut player, &[InputFrame { buttons: FIRE, ..Default::default() }, InputFrame::default()]);
        assert_eq!(traces.lock().expect("traces").last().map(String::as_str), Some("Liax!"));
    }

    /// Ruffle's own AS2 keyPress test (on(keyPress "a") traces "keyPress a"): a button press from
    /// input alone (a script, a replay) reaches it, with a map like a game's own `coop.keys`, for
    /// a friend's slot too, and only once while held.
    #[test]
    fn a_pressed_button_reaches_keypress_buttons() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/tests/swfs");
        let movie =
            ruffle_core::tag_utils::movie_from_path(root.join("avm1/button_key_events/test.swf"), None).expect("test swf");
        let traces = Arc::new(Mutex::new(vec![]));
        let player = PlayerBuilder::new()
            .with_movie(movie)
            .with_log(Traces(traces.clone()))
            .with_autoplay(true)
            .build();
        let mut player = player.lock().expect("player");
        let keys = Game::parse("game", "slot 0: FIRE=65\nslot 1: FIRE=65").expect("map").keys();
        let mut stepper = Stepper::new(keys, 2, 0);
        let none = InputFrame::default();
        let fire = InputFrame { buttons: FIRE, ..Default::default() };
        stepper.step(&mut player, &[none, none]);
        stepper.step(&mut player, &[none, fire]);
        stepper.step(&mut player, &[none, fire]);
        let presses = traces.lock().expect("traces").iter().filter(|t| *t == "keyPress a").count();
        assert_eq!(presses, 1);
    }

    /// Mice per frame, by slot: stage pixels and the left button.
    type Mice = Vec<Option<(i32, i32, bool)>>;

    fn at(x: i32, y: i32, down: bool) -> Option<(i32, i32, bool)> {
        Some((x * 20, y * 20, down))
    }

    /// Runs a test SWF (`desktop/tests/coop/<swf>`) for `players` with `rules` added to the key map:
    /// one idle frame, then `frames`, then three more of the last. Returns the traces.
    fn run_mice(swf: &str, rules: &str, players: u8, local: u8, frames: &[Mice]) -> (Vec<String>, Stepper, Arc<Mutex<Player>>) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/coop").join(swf);
        let movie = ruffle_core::tag_utils::movie_from_path(&path, None).expect("test swf");
        let traces = Arc::new(Mutex::new(vec![]));
        let player = PlayerBuilder::new().with_movie(movie).with_log(Traces(traces.clone())).with_autoplay(true).build();
        let slots: String = (0..players).map(|slot| format!("slot {slot}: FIRE=32\n")).collect();
        let mut stepper = Stepper::new(Game::parse("t", &format!("{slots}{rules}")).expect("map").keys(), players, local);
        {
            let mut player = player.lock().expect("player");
            let inputs = |mice: &Mice| -> Vec<InputFrame> {
                (0..players as usize).map(|slot| InputFrame { mouse: mice.get(slot).copied().flatten(), ..Default::default() }).collect()
            };
            let idle: Mice = vec![at(10, 10, false)];
            stepper.step(&mut player, &inputs(&idle));
            for mice in frames {
                stepper.step(&mut player, &inputs(mice));
            }
            for _ in 0..3 {
                stepper.step(&mut player, &inputs(frames.last().unwrap_or(&idle)));
            }
        }
        let traces = traces.lock().expect("traces").clone();
        (traces, stepper, player)
    }

    fn clicks(swf: &str, rules: &str, clicks: &[(usize, i32, i32)]) -> Vec<String> {
        let mut frames = vec![];
        for &(slot, x, y) in clicks {
            for down in [true, false] {
                let mut mice: Mice = vec![at(10, 10, false), None];
                mice[slot] = at(x, y, down);
                frames.push(mice);
            }
        }
        frames.push(vec![at(10, 10, false), None]);
        let (traces, _, player) = run_mice(swf, rules, 2, 0, &frames);
        assert_eq!(player.lock().expect("player").mouse_position_twips(), (200, 200), "Ruffle's own mouse is the host's ({rules})");
        traces
    }

    /// Friends' clicks on the click test SWF (a button-mode button at 100,100 100x60 traces "button"
    /// when clicked, any press traces "down X,Y"). By default only the host clicks; `mouse: buttons`
    /// lets a friend click what the game made clickable and nothing else; `all` anywhere; a zone
    /// keeps its box to its owner.
    #[test]
    fn friends_click_what_the_rules_allow() {
        let swf = "clicks/Clicks.swf";
        let on_button = [(1, 150, 130)];
        let off_button = [(1, 300, 250)];
        assert_eq!(clicks(swf, "", &on_button), Vec::<String>::new(), "only the host clicks by default");
        assert_eq!(clicks(swf, "", &[(0, 150, 130)]), ["down 150,130", "button"]);
        assert_eq!(clicks(swf, "mouse: buttons", &on_button), ["down 150,130", "button"]);
        assert_eq!(clicks(swf, "mouse: buttons\nhover: shared", &on_button), ["down 150,130", "button"]);
        assert_eq!(clicks(swf, "mouse: buttons", &off_button), Vec::<String>::new(), "not on something clickable");
        assert_eq!(clicks(swf, "mouse: all", &off_button), ["down 300,250"]);
        let zone = "mouse: buttons\nzone 0: 90,90 130x80";
        assert_eq!(clicks(swf, zone, &on_button), Vec::<String>::new(), "the host's box");
        assert_eq!(clicks(swf, zone, &[(0, 150, 130)]), ["down 150,130", "button"]);
        assert_eq!(clicks(swf, "mouse: buttons", &[(1, 150, 130), (0, 300, 250)]), ["down 150,130", "button", "down 300,250"]);
        // `zones`: a friend only in their own box, the host everywhere else
        assert_eq!(clicks(swf, "mouse: zones", &on_button), Vec::<String>::new(), "no box of their own");
        let own = "mouse: zones\nzone 1: 90,90 130x80";
        assert_eq!(clicks(swf, own, &on_button), ["down 150,130", "button"]);
        assert_eq!(clicks(swf, own, &[(0, 150, 130)]), Vec::<String>::new(), "the friend's box");
        assert_eq!(clicks(swf, own, &[(0, 300, 250)]), ["down 300,250"]);
        // a friend's press that never lets go doesn't stop the host: each player has their own
        // mouse button, so the host clicks the same button while the friend holds it
        let mut stuck: Vec<Mice> = vec![vec![at(10, 10, false), at(150, 130, true)]; 4];
        stuck.push(vec![at(150, 130, true), at(150, 130, true)]);
        stuck.push(vec![at(150, 130, false), at(150, 130, true)]);
        let (traces, ..) = run_mice(swf, "mouse: buttons", 2, 0, &stuck);
        assert_eq!(traces, ["down 150,130", "down 150,130", "button"], "the host still clicks");
    }

    /// Every player's own mouse on the mice test SWF: a sprite button "a" rolls over for the first
    /// mouse on it and out for the last; a click names its player (`coop.mouse.clicker`) and sees
    /// that player's mouse (`stage.mouseX`); `coop.mouse.hovered` says what each mouse is on.
    #[test]
    fn every_player_hovers_and_clicks_with_their_own_mouse() {
        let host = at(10, 10, false);
        let frames: Vec<Mice> = vec![
            vec![host, at(150, 130, false), None],
            vec![host, at(150, 130, false), at(160, 130, false)],
            vec![host, at(300, 250, false), at(160, 130, false)],
            vec![host, at(300, 250, false), at(300, 260, false)],
            vec![host, at(300, 250, false), at(170, 140, true)],
            vec![host, at(300, 250, false), at(170, 140, false)],
        ];
        let (traces, stepper, _) = run_mice("mice/Mice.swf", "mouse: all", 3, 0, &frames);
        assert_eq!(traces, ["over a", "out a", "over a", "click a 2 170,140"]);
        let api = stepper.api().lock().expect("api");
        assert_eq!(api.hovered[2].as_deref(), Some("_root.a"));
        assert_eq!(api.hovered[1], None, "on nothing clickable");
        assert_eq!(api.clicker, None, "only during a player's mouse events");
    }

    /// Player 2 drags the square while the host's mouse moves: it follows player 2's mouse only.
    #[test]
    fn a_drag_follows_the_mouse_that_started_it() {
        let frames: Vec<Mice> = vec![
            vec![at(10, 10, false), at(110, 210, true)],
            vec![at(50, 50, false), at(150, 230, true)],
            vec![at(60, 60, false), at(200, 250, true)],
            vec![at(70, 70, false), at(200, 250, false)],
        ];
        let (traces, ..) = run_mice("mice/Mice.swf", "mouse: all", 2, 0, &frames);
        assert_eq!(traces, ["dropped 190,240"]);
    }

    /// `mouse 2: host` keeps player 3's mouse out while player 2's is in.
    #[test]
    fn one_players_mouse_can_be_off() {
        let host = at(10, 10, false);
        let frames: Vec<Mice> = vec![
            vec![host, None, at(150, 130, true)],
            vec![host, None, at(150, 130, false)],
            vec![host, at(150, 130, true), at(10, 10, false)],
            vec![host, at(150, 130, false), at(10, 10, false)],
        ];
        let (traces, ..) = run_mice("mice/Mice.swf", "mouse: all\nmouse 2: host", 3, 0, &frames);
        assert_eq!(traces, ["over a", "click a 1 150,130"]);
    }

    /// A private screen (the click test SWF's pop-up, opened by its button): it belongs to the
    /// friend whose click opened it, the host's click goes through it to the button under it, only
    /// the owner's PC draws it, and its owner can use it; once closed it is nobody's.
    #[test]
    fn a_private_screen_belongs_to_whoever_opened_it() {
        let host = at(10, 10, false);
        let click = |slot: usize, x: i32, y: i32| -> Vec<Mice> {
            [true, false]
                .map(|down| {
                    let mut mice = vec![host, None];
                    mice[slot] = at(x, y, down);
                    mice
                })
                .to_vec()
        };
        for local in [0u8, 1] {
            let mut frames = click(1, 150, 130); // the friend opens the pop-up
            frames.extend(click(0, 150, 130)); // the host clicks through it, onto the button
            frames.extend(click(0, 250, 185)); // the host can't use the pop-up's button
            frames.extend(click(1, 250, 185)); // its owner can
            frames.push(vec![host, at(10, 10, false)]);
            let (traces, stepper, _) = run_mice("clicks/Clicks.swf", "mouse: buttons\nprivate: _root.popup", 2, local, &frames);
            assert_eq!(stepper.api().lock().expect("api").privates[0].owner, None, "closed: nobody's");
            assert_eq!(
                traces,
                ["down 150,130", "button", "down 150,130", "button", "down 250,185", "down 250,185", "popup"],
                "PC of player {}",
                local + 1
            );
        }
    }

    /// Four players on the click test SWF. Presses in the same frame all reach the game, in slot
    /// order; a friend's zone keeps out another friend; a friend holding the button doesn't hold up
    /// anyone; a private pop-up is its opener's with four players.
    #[test]
    fn four_players_click_at_once() {
        let swf = "clicks/Clicks.swf";
        let host = at(10, 10, false);
        let together: Vec<Mice> = vec![
            vec![host, at(300, 250, true), at(310, 250, true), at(320, 250, true)],
            vec![host, at(300, 250, false), at(310, 250, false), at(320, 250, false)],
        ];
        let (traces, ..) = run_mice(swf, "mouse: all", 4, 0, &together);
        assert_eq!(traces, ["down 300,250", "down 310,250", "down 320,250"]);

        let boxed: Vec<Mice> = vec![
            vec![host, None, at(300, 250, true), at(305, 250, true)],
            vec![host, None, at(300, 250, false), at(305, 250, false)],
        ];
        let (traces, ..) = run_mice(swf, "mouse: all\nzone 2: 280,230 60x40", 4, 0, &boxed);
        assert_eq!(traces, ["down 300,250"], "player 4 may not click in player 3's box");

        let mut held: Vec<Mice> = vec![vec![host, at(300, 250, true), None, None]];
        held.push(vec![host, at(300, 250, true), at(310, 250, true), None]);
        held.push(vec![host, at(300, 250, true), at(310, 250, false), None]);
        held.push(vec![host, at(300, 250, false), at(310, 250, false), None]);
        let (traces, ..) = run_mice(swf, "mouse: all", 4, 0, &held);
        assert_eq!(traces, ["down 300,250", "down 310,250"], "player 3 clicks while player 2 holds");

        let private: Vec<Mice> = vec![
            vec![host, None, at(150, 130, true), None],
            vec![host, None, at(150, 130, false), None],
            vec![host, at(150, 130, true), None, at(250, 185, true)],
            vec![host, at(150, 130, false), None, at(250, 185, false)],
            vec![host, None, at(250, 185, true), None],
            vec![host, None, at(250, 185, false), None],
        ];
        let (traces, stepper, _) = run_mice(swf, "mouse: buttons\nprivate: _root.popup", 4, 0, &private);
        assert_eq!(traces, ["down 150,130", "button", "down 150,130", "button", "down 250,185", "popup"]);
        assert_eq!(stepper.api().lock().expect("api").privates[0].owner, None, "closed by its owner");
    }

    /// 8 players: players 2-8 each click their own spot at once, each with their own mouse; the host's
    /// mouse stays where it was.
    #[test]
    fn eight_players_click_at_once() {
        let host = at(10, 10, false);
        let all = |down: bool| -> Mice { std::iter::once(host).chain((1..8).map(|slot| at(290 + 5 * slot, 250, down))).collect() };
        let (traces, _, player) = run_mice("clicks/Clicks.swf", "mouse: all", 8, 0, &[all(true), all(false)]);
        let expected: Vec<String> = (1..8).map(|slot| format!("down {},250", 290 + 5 * slot)).collect();
        assert_eq!(traces, expected);
        assert_eq!(player.lock().expect("player").mouse_position_twips(), (200, 200), "the host's mouse didn't move");
    }

    /// `hover: own`: player 2's mouse on the SimpleButton makes the game's button Over (red) on
    /// every PC, but each PC draws it as its own player's mouse sees it: the host's PC in Up art
    /// (blue), player 2's in Over art. `hover: shared` draws the game's state everywhere. The game
    /// itself is the same on both PCs.
    #[test]
    fn each_screen_draws_buttons_by_its_own_mouse() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/coop/mice/Mice.swf");
        let colour = |hover: &str, local: u8| -> ([u8; 3], u64) {
            let clock = Some(Arc::new(Mutex::new(FrameClock::new(1_700_000_000_000))));
            let player = crate::coop::replay::headless_player(&path, clock, crate::coop::storage::SessionStorage::default())
                .expect("player");
            let mut player = player.lock().expect("player");
            let keys = Game::parse("t", &format!("slot 0: FIRE=32\nslot 1: FIRE=32\nmouse: all\nhover: {hover}")).expect("map");
            let mut stepper = Stepper::new(keys.keys(), 2, local);
            for _ in 0..3 {
                let inputs = [
                    InputFrame { mouse: at(10, 10, false), ..Default::default() },
                    InputFrame { mouse: at(300, 130, false), ..Default::default() },
                ];
                stepper.step(&mut player, &inputs);
            }
            let picture = crate::coop::replay::capture(&mut player).expect("capture");
            let (sx, sy) = (picture.width() as f64 / 400.0, picture.height() as f64 / 300.0);
            let pixel = picture.get_pixel((300.0 * sx) as u32, (130.0 * sy) as u32);
            ([pixel[0], pixel[1], pixel[2]], player.coop_fingerprint())
        };
        let (blue, red) = ([0x30, 0x70, 0xF0], [0xE0, 0x40, 0x40]);
        let (host, host_print) = colour("own", 0);
        let (friend, friend_print) = colour("own", 1);
        assert_eq!((host, friend), (blue, red), "each PC by its own mouse");
        assert_eq!(colour("shared", 0).0, red, "the game's own state");
        assert_eq!(host_print, friend_print, "the game is the same on both PCs");
    }

    /// A game's own `coop.keys` map without `mouse:` or `zone` lines keeps the click rules the
    /// game set (the API demo sets `coop.mouse("buttons")`, then its key map).
    #[test]
    fn a_game_key_map_keeps_the_click_rules() {
        let swf = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/coop/clicks/Clicks.swf");
        let movie = ruffle_core::tag_utils::movie_from_path(&swf, None).expect("test swf");
        let player = PlayerBuilder::new().with_movie(movie).with_autoplay(true).build();
        let mut player = player.lock().expect("player");
        let keys = Game::parse("t", "slot 0: FIRE=32
slot 1: FIRE=32").expect("map").keys();
        let mut stepper = Stepper::new(keys, 2, 0);
        stepper.step(&mut player, &[InputFrame::default(), InputFrame::default()]);
        {
            let mut api = stepper.api().lock().expect("api");
            api.answer("coop.mouse", &[Value::String("buttons".into())]);
            api.answer("coop.zone", &[1.0, 1.0, 2.0, 3.0, 4.0].map(Value::Number));
            api.answer("coop.keys", &[Value::String("slot 0: FIRE=13
slot 1: FIRE=13".into())]);
        }
        stepper.apply_game_calls(&mut player);
        let api = stepper.api().lock().expect("api");
        assert_eq!((api.mouse_mode, api.zones.len()), (crate::coop::keymap::MouseMode::Buttons, 1));
    }

    /// Bad Ice-Cream, headless (needs COOP_TEST_SWF), and a Stepper for `local_slot`.
    fn bic(local_slot: u8) -> Option<(Arc<Mutex<Player>>, Stepper)> {
        let swf = std::env::var("COOP_TEST_SWF").ok()?;
        let player = crate::coop::replay::headless_player(
            std::path::Path::new(&swf),
            Some(Arc::new(Mutex::new(FrameClock::new(1_700_000_000_000)))),
            crate::coop::storage::SessionStorage::default(),
        )
        .expect("player");
        let keys = Game::built_in("bad-ice-cream").expect("built in").keys();
        Some((player, Stepper::new(keys, 2, local_slot)))
    }

    fn set_cameras(stepper: &Stepper, cameras: [Option<Camera>; 2]) {
        let mut api = stepper.api().lock().expect("api");
        api.cameras[..2].copy_from_slice(&cameras);
    }

    /// The share of pixels that differ between two pictures of the same size.
    fn differ(a: &image::RgbaImage, b: &image::RgbaImage) -> f64 {
        assert_eq!(a.dimensions(), b.dimensions());
        let changed = a.pixels().zip(b.pixels()).filter(|(p, q)| p != q).count();
        changed as f64 / (a.width() * a.height()) as f64
    }

    /// Every slot has its own camera and each PC draws with its own: the pictures differ, the
    /// game (its fingerprint) doesn't. A HUD clip is drawn as if there were no camera.
    #[test]
    fn cameras_change_the_picture_not_the_game() {
        let Some((host, mut host_stepper)) = bic(0) else {
            eprintln!("skipping: COOP_TEST_SWF not set");
            return;
        };
        let (friend, mut friend_stepper) = bic(1).expect("player");
        let (plain, mut plain_stepper) = bic(0).expect("player");
        let cameras = [
            Some(Camera { x: 100.0, y: 100.0, zoom: 2.0 }),
            Some(Camera { x: 400.0, y: 300.0, zoom: 1.5 }),
        ];
        set_cameras(&host_stepper, cameras);
        set_cameras(&friend_stepper, cameras);
        let (mut host, mut friend, mut plain) =
            (host.lock().expect("lock"), friend.lock().expect("lock"), plain.lock().expect("lock"));
        let inputs = [InputFrame { buttons: UP, mouse: None, ..Default::default() }, InputFrame::default()];
        for frame in 0..120 {
            host_stepper.step(&mut host, &inputs);
            friend_stepper.step(&mut friend, &inputs);
            plain_stepper.step(&mut plain, &inputs);
            let print = plain.coop_fingerprint();
            assert_eq!(host.coop_fingerprint(), print, "host, frame {frame}");
            assert_eq!(friend.coop_fingerprint(), print, "friend, frame {frame}");
        }
        let capture = crate::coop::replay::capture;
        let (host_picture, friend_picture, plain_picture) = (
            capture(&mut host).expect("capture"),
            capture(&mut friend).expect("capture"),
            capture(&mut plain).expect("capture"),
        );
        assert!(differ(&host_picture, &plain_picture) > 0.2, "the host's camera moved the picture");
        assert!(differ(&friend_picture, &host_picture) > 0.2, "each PC draws its own slot's camera");

        // the whole root as HUD: drawn without the camera, so like no camera at all
        host.mutate_with_update_context(|context| {
            assert!(ruffle_core::coop_view::set_hud(context, "_root"));
        });
        let hud_picture = capture(&mut host).expect("capture");
        assert!(differ(&hud_picture, &plain_picture) < 0.01, "{}", differ(&hud_picture, &plain_picture));
        assert_eq!(host.coop_fingerprint(), plain.coop_fingerprint(), "HUD doesn't touch the game");
    }

    /// This PC's cursor reaches the game through its camera: the middle of the window is the
    /// point the camera looks at, in stage twips, on every PC.
    #[test]
    fn the_mouse_goes_through_the_camera() {
        let Some((player, mut stepper)) = bic(0) else {
            eprintln!("skipping: COOP_TEST_SWF not set");
            return;
        };
        set_cameras(&stepper, [Some(Camera { x: 100.0, y: 50.0, zoom: 2.0 }), None]);
        let mut player = player.lock().expect("lock");
        stepper.step(&mut player, &[InputFrame::default(), InputFrame::default()]);
        let (width, height) = (player.movie_width() as f64, player.movie_height() as f64);
        let (x, y) = player.viewport_to_stage(width / 2.0, height / 2.0);
        assert!((x - 2000).abs() <= 1 && (y - 1000).abs() <= 1, "({x}, {y})");
        let label = player.coop_label_position("_root").expect("the root is on show");
        player.set_coop_camera(None);
        assert_ne!(player.coop_label_position("_root"), Some(label), "labels follow the camera");
    }

    /// `coop.keys`: the game's own map is used from the next frame, held keys are let go first,
    /// and giving the same map again (every frame, say) changes nothing.
    #[test]
    fn the_game_can_switch_the_key_map() {
        let player = PlayerBuilder::new().build();
        let mut player = player.lock().expect("player");
        let mut stepper = Stepper::new(Game::built_in("bad-ice-cream").expect("built in").keys(), 2, 0);
        let map = "slot 0: FIRE=65\nslot 1: FIRE=66";
        let give = |stepper: &Stepper| {
            let answer = stepper.api().lock().expect("api").answer("coop.keys", &[Value::String(map.into())]);
            assert_eq!(answer, Value::Bool(true));
        };
        give(&stepper);
        let fire = [InputFrame { buttons: FIRE, mouse: None, ..Default::default() }, InputFrame::default()];
        stepper.step(&mut player, &fire); // the switch happens at the end of this frame
        assert_eq!(stepper.game_keys.as_deref(), Some(map));
        // the new map: slot 0's FIRE is A now (it was Space), and nothing is held yet
        let events = stepper.keys.events(0, FIRE);
        assert!(matches!(events.first(), Some(PlayerEvent::KeyDown { key })
            if key.physical_key == ruffle_core::events::PhysicalKey::KeyA), "{events:?}");
        give(&stepper);
        stepper.step(&mut player, &fire);
        assert!(stepper.keys.events(0, FIRE).is_empty(), "FIRE stays held: the same map didn't reset it");
    }
}
