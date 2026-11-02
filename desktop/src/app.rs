use crate::custom_event::{OpenType, RuffleEvent};
use crate::gui::{GuiController, MENU_HEIGHT};
use crate::player::{LaunchOptions, PlayerController};
use crate::preferences::GlobalPreferences;
use crate::util::{
    get_screen_size, gilrs_button_to_gamepad_button, winit_input_to_ruffle_key_descriptor,
    winit_to_ruffle_text_control,
};
use anyhow::Error;
use gilrs::{Event, EventType, Gilrs};
use ruffle_core::FloatDuration;
use ruffle_core::PlayerEvent;
use ruffle_core::events::{ImeEvent, ImeNotification, PlayerNotification};
use ruffle_core::swf::HeaderExt;
use ruffle_frontend_utils::content::ContentDescriptor;
use ruffle_render::backend::ViewportDimensions;
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize, Size};
use winit::event::{ElementState, Ime, KeyEvent, Modifiers, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Fullscreen, Icon, WindowAttributes, WindowId};

struct MainWindow {
    preferences: GlobalPreferences,
    gui: GuiController,
    player: PlayerController,
    minimized: bool,
    /// Set from `WindowEvent::Occluded`, which winit only reports on macOS and X11
    /// (and on X11 only without a compositor). Always false elsewhere.
    occluded: bool,
    mouse_pos: PhysicalPosition<f64>,

    /// Whether `mouse_pos` changed since the last `MouseMove` sent to the player.
    /// Moves are coalesced, so that a movie with slow mouse handlers sees only
    /// the latest position instead of every move queued while it was busy.
    mouse_move_pending: bool,

    modifiers: Modifiers,
    min_window_size: LogicalSize<u32>,
    max_window_size: PhysicalSize<u32>,
    no_gui: bool,
    preferred_width: Option<f64>,
    preferred_height: Option<f64>,
    start_fullscreen: bool,
    loaded: LoadingState,
    time: Instant,
    next_frame_time: Option<Instant>,
    /// Ruffle Together: the window title last shown for the session.
    coop_title: String,
    /// Ruffle Together: the window shows the waiting screen; the game loads once the session starts.
    coop_waiting: bool,
    /// Ruffle Together TV mode without a session: the keys the controller presses (slot 0 of
    /// `--coop-keys`, or the built-in `controller` map) and the buttons they have.
    tv_keys: Option<(crate::coop::keymap::GameKeys, u16)>,
    /// TV mode has read the name of the controller connected at start.
    tv_pad_named: bool,
    event_loop_proxy: EventLoopProxy<RuffleEvent>,
}

impl MainWindow {
    pub fn window_event(&mut self, event_loop: &ActiveEventLoop, event: WindowEvent) {
        if matches!(event, WindowEvent::RedrawRequested) {
            // Don't render when nobody can see the result: while minimized, the
            // surface also keeps the size it had before, as `GuiController::resize`
            // refuses to reconfigure it to a zero size.
            // Ruffle Together: in a session, only a new game frame is shown (see PictureGate)
            if !self.minimized
                && !self.occluded
                && crate::coop::session().is_none_or(|mut session| session.allow_picture())
            {
                self.gui.render(self.player.get());
            }

            // Important that we return here, or we'll get a feedback loop with egui
            // (winit says redraw, egui hears redraw and says redraw, we hear redraw and tell winit to redraw...)
            return;
        }

        if self.gui.handle_event(&event) {
            // Event consumed by GUI.
            return;
        }
        match event {
            WindowEvent::CloseRequested => {
                crate::coop::goodbye("closed the game");
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                // How minimizing shows up on Windows; winit still has no
                // `WindowEvent::Minimize`. (`Window::is_minimized` does exist by
                // now, but it has to be polled, and it wouldn't tell us anything
                // new: macOS reports minimized windows as occluded instead, and
                // on Wayland it always returns `None`.)
                self.minimized = size.width == 0 && size.height == 0;

                if let Some(mut player) = self.player.get() {
                    let viewport_scale_factor = self.gui.window().scale_factor();
                    player.set_viewport_dimensions(ViewportDimensions {
                        width: size.width,
                        height: size.height.saturating_sub(self.gui.height_offset() as u32),
                        scale_factor: viewport_scale_factor,
                    });
                }
                self.gui.window().request_redraw();
                if matches!(self.loaded, LoadingState::WaitingForResize) {
                    self.loaded = LoadingState::Loaded;
                }
            }
            WindowEvent::Occluded(occluded) => {
                self.occluded = occluded;
                if !occluded {
                    // Nothing repaints us on its own if the movie is paused.
                    self.gui.window().request_redraw();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if self.gui.is_context_menu_visible() {
                    return;
                }

                // This is needed because some platforms (like macOS)
                // will fire this event at the same time as WindowEvent::MouseInput,
                // which may cause inaccurate behavior in the movie
                if position == self.mouse_pos {
                    return;
                }

                self.mouse_pos = position;
                // TV mode: the right stick carries on from the real mouse
                if let Some(mut tv) = crate::coop::tv::tv() {
                    tv.mouse_moved(position.x, position.y);
                }

                // We can't handle mouse move here, doing so would put Ruffle in
                // a busy loop processing every queued CursorMoved event. Flash
                // Player does not queue mouse moves and instead skips them
                // if they happen when the movie is busy.
                self.mouse_move_pending = true;
            }
            WindowEvent::DroppedFile(file) if crate::coop::session().is_none() => {
                if let Some(content_descriptor) = ContentDescriptor::new_local(&file, None) {
                    self.gui.create_movie(
                        &mut self.player,
                        LaunchOptions::from(&self.preferences),
                        content_descriptor,
                    );
                }
            }
            WindowEvent::Focused(focused) => {
                // TV mode's controller counts only while this window is in front, like the keys
                if let Some(mut tv) = crate::coop::tv::tv() {
                    tv.focus(focused);
                }
                self.handle_player_event(if focused { PlayerEvent::FocusGained } else { PlayerEvent::FocusLost });
            }
            WindowEvent::MouseInput { button, state, .. } => {
                if self.gui.is_context_menu_visible() {
                    return;
                }

                use ruffle_core::events::MouseButton as RuffleMouseButton;
                use winit::event::MouseButton;
                let (x, y) = self.gui.window_to_movie_position(self.mouse_pos);
                let button = match button {
                    MouseButton::Left => RuffleMouseButton::Left,
                    MouseButton::Right => RuffleMouseButton::Right,
                    MouseButton::Middle => RuffleMouseButton::Middle,
                    _ => RuffleMouseButton::Unknown,
                };
                let event = match state {
                    // TODO We should get information about click index from the OS,
                    //   but winit does not support that yet.
                    ElementState::Pressed => PlayerEvent::MouseDown {
                        x,
                        y,
                        button,
                        index: None,
                    },
                    ElementState::Released => PlayerEvent::MouseUp { x, y, button },
                };
                let handled = self.handle_player_event(event);
                if !handled && state == ElementState::Pressed && button == RuffleMouseButton::Right
                {
                    // Show context menu.
                    if let Some(mut player) = self.player.get() {
                        let context_menu = player.prepare_context_menu();

                        // MouseUp event will be ignored when the context menu is shown,
                        // but it has to be dispatched when the menu closes.
                        let close_event = PlayerEvent::MouseUp {
                            x,
                            y,
                            button: RuffleMouseButton::Right,
                        };
                        self.gui.show_context_menu(context_menu, close_event);
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if self.gui.is_context_menu_visible() {
                    return;
                }

                use ruffle_core::events::MouseWheelDelta;
                use winit::event::MouseScrollDelta;
                let delta = match delta {
                    MouseScrollDelta::LineDelta(_, dy) => MouseWheelDelta::Lines(dy.into()),
                    MouseScrollDelta::PixelDelta(pos) => MouseWheelDelta::Pixels(pos.y),
                };
                let event = PlayerEvent::MouseWheel { delta };
                self.handle_player_event(event);
            }
            WindowEvent::CursorEntered { .. } if crate::coop::session().is_none() => {
                if let Some(mut player) = self.player.get() {
                    player.set_mouse_in_stage(true);
                }
            }
            WindowEvent::CursorLeft { .. } if crate::coop::session().is_none() => {
                // The movie has to see the latest position while the mouse
                // is still in the stage.
                self.flush_mouse_move();
                if let Some(mut player) = self.player.get() {
                    player.set_mouse_in_stage(false);
                }
                self.handle_player_event(PlayerEvent::MouseLeave);
            }
            WindowEvent::ModifiersChanged(new_modifiers) => {
                self.modifiers = new_modifiers;
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if self.gui.is_context_menu_visible() {
                    return;
                }

                // TV mode: hints name the keyboard while it's in use
                if event.state == ElementState::Pressed
                    && let Some(mut tv) = crate::coop::tv::tv()
                {
                    tv.keys_used();
                }

                // Handle escaping from fullscreen.
                if let KeyEvent {
                    state: ElementState::Pressed,
                    logical_key: Key::Named(NamedKey::Escape),
                    ..
                } = event
                {
                    let _ = self
                        .event_loop_proxy
                        .send_event(RuffleEvent::ExitFullScreen);
                }

                let key = winit_input_to_ruffle_key_descriptor(&event);
                match event.state {
                    ElementState::Pressed => {
                        self.handle_player_event(PlayerEvent::KeyDown { key });
                        if let Some(control_code) =
                            winit_to_ruffle_text_control(&event, self.modifiers)
                        {
                            self.handle_player_event(PlayerEvent::TextControl {
                                code: control_code,
                            });
                        } else if let Some(text) = event.text {
                            for codepoint in text.chars() {
                                self.handle_player_event(PlayerEvent::TextInput { codepoint });
                            }
                        }
                    }
                    ElementState::Released => {
                        self.handle_player_event(PlayerEvent::KeyUp { key });
                    }
                };
            }
            WindowEvent::Ime(ime) => match ime {
                Ime::Enabled => {}
                Ime::Preedit(text, cursor) => {
                    self.handle_player_event(PlayerEvent::Ime(ImeEvent::Preedit(text, cursor)));
                }
                Ime::Commit(text) => {
                    self.handle_player_event(PlayerEvent::Ime(ImeEvent::Commit(text)));
                }
                Ime::Disabled => {}
            },
            _ => (),
        }
    }

    fn on_metadata(&mut self, swf_header: HeaderExt) {
        let height_offset = if self.gui.window().fullscreen().is_some() || self.no_gui {
            0.0
        } else {
            MENU_HEIGHT as f64
        };

        // To prevent issues like waiting on resize indefinitely (#11364) or desyncing the window state on Windows,
        // do not resize while window is maximized.
        let should_resize = !self.gui.window().is_maximized();

        let (viewport_size, state) = if should_resize {
            let movie_width = swf_header.stage_size().width().to_pixels();
            let movie_height = swf_header.stage_size().height().to_pixels();

            let window_size: Size = match (self.preferred_width, self.preferred_height) {
                (None, None) => LogicalSize::new(movie_width, movie_height + height_offset).into(),
                (Some(width), None) => {
                    let scale = width / movie_width;
                    let height = movie_height * scale;
                    PhysicalSize::new(
                        width.max(1.0),
                        height.max(1.0) + height_offset * self.gui.window().scale_factor(),
                    )
                    .into()
                }
                (None, Some(height)) => {
                    let scale = height / movie_height;
                    let width = movie_width * scale;
                    PhysicalSize::new(
                        width.max(1.0),
                        height.max(1.0) + height_offset * self.gui.window().scale_factor(),
                    )
                    .into()
                }
                (Some(width), Some(height)) => PhysicalSize::new(
                    width.max(1.0),
                    height.max(1.0) + height_offset * self.gui.window().scale_factor(),
                )
                .into(),
            };

            let window_size = Size::clamp(
                window_size,
                self.min_window_size.into(),
                self.max_window_size.into(),
                self.gui.window().scale_factor(),
            );

            let viewport_size = self.gui.window().inner_size();
            let mut window_resize_denied = false;

            if let Some(new_viewport_size) = self.gui.window().request_inner_size(window_size) {
                if new_viewport_size != viewport_size {
                    self.gui.resize(new_viewport_size);
                } else {
                    tracing::warn!("Unable to resize window");
                    window_resize_denied = true;
                }
            }

            let viewport_size = self.gui.window().inner_size();

            // On X11 (and possibly other platforms), the window size is not updated immediately.
            // On a successful resize request, wait for the window to be resized to the requested size
            // before we start running the SWF (which can observe the viewport size in "noScale" mode)
            let state = if !window_resize_denied && window_size != viewport_size.into() {
                LoadingState::WaitingForResize
            } else {
                LoadingState::Loaded
            };

            (viewport_size, state)
        } else {
            (self.gui.window().inner_size(), LoadingState::Loaded)
        };

        self.loaded = state;

        self.gui.window().set_fullscreen(if self.start_fullscreen {
            Some(Fullscreen::Borderless(None))
        } else {
            None
        });
        self.gui.window().set_visible(true);

        let viewport_scale_factor = self.gui.window().scale_factor();
        if let Some(mut player) = self.player.get() {
            player.set_viewport_dimensions(ViewportDimensions {
                width: viewport_size.width,
                height: viewport_size.height - (height_offset * viewport_scale_factor) as u32,
                scale_factor: viewport_scale_factor,
            });
        }
    }

    fn about_to_wait(&mut self, mut gilrs: Option<&mut Gilrs>) {
        // When we don't have any more pending events, flush any pending mouse
        // moves, so that they can get coalesced properly.
        self.flush_mouse_move();

        let tv = crate::coop::tv::on();
        if tv {
            self.tv_input(gilrs.take());
        }

        // Ruffle Together: the waiting screen until everyone has joined, then the game
        if self.coop_waiting {
            if crate::coop::session().is_some()
                && let Some(movie_url) = self.preferences.cli.movie_url.clone()
            {
                self.coop_waiting = false;
                self.gui.create_movie(
                    &mut self.player,
                    LaunchOptions::from(&self.preferences),
                    ContentDescriptor {
                        url: movie_url,
                        root_content_path: None,
                    },
                );
            } else {
                self.gui.window().request_redraw();
                self.next_frame_time = Some(Instant::now() + std::time::Duration::from_millis(100));
                return;
            }
        }

        if let Some(Event { event, .. }) = gilrs.and_then(|gilrs| gilrs.next_event()) {
            match event {
                EventType::ButtonPressed(button, _) => {
                    if let Some(button) = gilrs_button_to_gamepad_button(button) {
                        self.handle_player_event(PlayerEvent::GamepadButtonDown { button });
                    }
                }
                EventType::ButtonReleased(button, _) => {
                    if let Some(button) = gilrs_button_to_gamepad_button(button) {
                        self.handle_player_event(PlayerEvent::GamepadButtonUp { button });
                    }
                }
                _ => {}
            }
        }

        // Core loop
        // [NA] This used to be called `MainEventsCleared`, but I think the behaviour is different now.
        // We should look at changing our tick to happen somewhere else if we see any behavioural problems.
        if matches!(self.loaded, LoadingState::Loaded)
            && let Some(mut session) = crate::coop::session()
        {
            // Ruffle Together: frames run when every player's input is in, not by the clock
            if let Some(mut player) = self.player.get() {
                let before = session.frames_run();
                session.tick(&mut player);
                // the step already drew the frame (clearing needs_render), so the window has to
                // be told to show it
                if session.frames_run() != before {
                    self.gui.window().request_redraw();
                }
            }
            let title = session.title();
            if title != self.coop_title {
                tracing::info!("{title}");
                self.gui.window().set_title(&title);
                self.coop_title = title;
                crate::coop::discord::show(session.presence());
            }
            self.next_frame_time = Some(Instant::now() + std::time::Duration::from_millis(2));
        } else if matches!(self.loaded, LoadingState::Loaded) {
            let new_time = Instant::now();
            let dt = FloatDuration::from_std(new_time.duration_since(self.time));
            if dt.as_millis() > 0.0 {
                self.time = new_time;
                self.next_frame_time = self.player.get().map(|mut player| {
                    player.tick(dt);
                    new_time + player.time_til_next_frame()
                });
            }
        }

        self.render_movie_if_needed();
        if tv {
            // the controller is read on these passes: keep them coming
            let soon = Instant::now() + std::time::Duration::from_millis(16);
            self.next_frame_time = Some(self.next_frame_time.map_or(soon, |at| at.min(soon)));
        }
    }

    /// Ruffle Together TV mode: the controller's buttons go to the game as the key map's buttons
    /// (through the session, or as slot 0's keys without one), its right stick is the mouse, and
    /// the TV menu can quit.
    fn tv_input(&mut self, gilrs: Option<&mut Gilrs>) {
        use crate::coop::tv::{Action, Pad, Stick};
        use gilrs::{Axis, Button};
        let Some(mut tv) = crate::coop::tv::tv() else {
            return;
        };
        let now = Instant::now();
        let menu_before = tv.menu();
        if let Some(gilrs) = gilrs {
            if !std::mem::replace(&mut self.tv_pad_named, true)
                && let Some((_, pad)) = gilrs.gamepads().next()
            {
                tv.connected(pad.name());
            }
            while let Some(Event { id, event, .. }) = gilrs.next_event() {
                let pad = |button| match button {
                    Button::South => Some(Pad::A),
                    Button::East => Some(Pad::B),
                    Button::West => Some(Pad::X),
                    Button::North => Some(Pad::Y),
                    Button::LeftTrigger => Some(Pad::Lb),
                    Button::RightTrigger => Some(Pad::Rb),
                    Button::LeftTrigger2 => Some(Pad::Lt),
                    Button::RightTrigger2 => Some(Pad::Rt),
                    Button::Select => Some(Pad::View),
                    Button::Start => Some(Pad::Start),
                    Button::LeftThumb => Some(Pad::L3),
                    Button::RightThumb => Some(Pad::R3),
                    Button::DPadUp => Some(Pad::Up),
                    Button::DPadDown => Some(Pad::Down),
                    Button::DPadLeft => Some(Pad::Left),
                    Button::DPadRight => Some(Pad::Right),
                    _ => None,
                };
                match event {
                    EventType::Connected => tv.connected(gilrs.gamepad(id).name()),
                    EventType::ButtonPressed(button, _) => {
                        if let Some(pad) = pad(button) {
                            tv.button(pad, true, now);
                        }
                    }
                    EventType::ButtonReleased(button, _) => {
                        if let Some(pad) = pad(button) {
                            tv.button(pad, false, now);
                        }
                    }
                    EventType::AxisChanged(axis, value, _) => {
                        let stick = match axis {
                            Axis::LeftStickX => Some(Stick::LeftX),
                            Axis::LeftStickY => Some(Stick::LeftY),
                            Axis::RightStickX => Some(Stick::RightX),
                            Axis::RightStickY => Some(Stick::RightY),
                            _ => None,
                        };
                        if let Some(stick) = stick {
                            tv.stick(stick, value);
                        }
                    }
                    _ => {}
                }
            }
        }
        tv.run_script(now);
        let session = crate::coop::session();
        let uses = match (&session, &self.tv_keys) {
            (Some(session), _) => session.buttons_used(),
            (None, Some((_, uses))) => *uses,
            (None, None) => 0,
        };
        let size = self.gui.window().inner_size();
        tv.update(now, (size.width as f64, size.height as f64), uses);
        let buttons = tv.buttons(now, uses);
        let actions = tv.take_actions();
        let menu_changed = tv.menu() != menu_before;
        drop(tv);
        if let Some(mut session) = session {
            session.pad(buttons);
        } else if let Some((keys, _)) = &mut self.tv_keys
            && matches!(self.loaded, LoadingState::Loaded)
        {
            for event in keys.events(0, buttons) {
                self.player.handle_event(event);
            }
        }
        for action in actions {
            use ruffle_core::events::MouseButton;
            let event = match action {
                Action::MouseMove { x, y } => PlayerEvent::MouseMove { x, y },
                Action::MouseDown { x, y } => PlayerEvent::MouseDown { x, y, button: MouseButton::Left, index: None },
                Action::MouseUp { x, y } => PlayerEvent::MouseUp { x, y, button: MouseButton::Left },
                Action::Quit => {
                    crate::coop::goodbye("left from the TV menu");
                    let _ = self.event_loop_proxy.send_event(RuffleEvent::ExitRequested);
                    continue;
                }
            };
            self.handle_player_event(event);
        }
        if menu_changed {
            self.gui.window().request_redraw();
        }
    }

    fn handle_player_event(&mut self, event: PlayerEvent) -> bool {
        // Apply any pending mouse moves, so that the movie sees the latest
        // position before e.g. a click.
        self.flush_mouse_move();
        if crate::coop::session().is_some() {
            return self.coop_input(event);
        }
        let handled = self.player.handle_event(event);

        // Handling an event may have made the movie need rendering (on demand).
        self.render_movie_if_needed();
        handled
    }

    /// Ruffle Together: this PC's keys and mouse go to the session, which sends them to everyone
    /// and presses them on every PC's game at the same frame. Nothing reaches the game directly.
    fn coop_input(&self, event: PlayerEvent) -> bool {
        // the most a paste types (it travels with the inputs, 8 characters a frame)
        const PASTE_LIMIT: usize = 64;
        let Some(mut session) = crate::coop::session() else {
            return false;
        };
        match event {
            PlayerEvent::KeyDown { key } => session.key(key.physical_key, true),
            PlayerEvent::KeyUp { key } => session.key(key.physical_key, false),
            PlayerEvent::TextInput { .. } | PlayerEvent::TextControl { .. } => {
                use ruffle_core::events::TextControlCode;
                let Some(mut player) = self.player.get() else {
                    return true;
                };
                let text_box = player.coop_text_box_focused();
                match event {
                    PlayerEvent::TextInput { codepoint } => session.typed(codepoint, text_box),
                    PlayerEvent::TextControl { code: TextControlCode::Paste } => {
                        // this PC's clipboard, as typed characters every PC gets
                        let pasted = player.ui_mut().clipboard_content();
                        for c in pasted.chars().filter(|c| !c.is_control()).take(PASTE_LIMIT) {
                            session.typed(c, text_box);
                        }
                    }
                    PlayerEvent::TextControl { code: TextControlCode::Copy } => {}
                    PlayerEvent::TextControl { code } => {
                        let code = if code == TextControlCode::Cut { TextControlCode::Delete } else { code };
                        session.typed(crate::coop::lockstep::text_control_char(code), text_box);
                    }
                    _ => {}
                }
            }
            PlayerEvent::FocusGained => session.focus(true),
            PlayerEvent::FocusLost => session.focus(false),
            PlayerEvent::MouseMove { x, y }
            | PlayerEvent::MouseDown { x, y, .. }
            | PlayerEvent::MouseUp { x, y, .. } => {
                use ruffle_core::events::MouseButton;
                // only the left button clicks the game (a right click opens Ruffle's menu, which
                // swallows its release)
                let down = match event {
                    PlayerEvent::MouseDown { button: MouseButton::Left, .. } => Some(true),
                    PlayerEvent::MouseUp { button: MouseButton::Left, .. } => Some(false),
                    _ => None,
                };
                if let Some(mut player) = self.player.get() {
                    let (x, y) = player.viewport_to_stage(x, y);
                    session.mouse(x, y, down);
                }
            }
            _ => {}
        }
        true
    }

    fn flush_mouse_move(&mut self) {
        if !std::mem::take(&mut self.mouse_move_pending) {
            return;
        }

        let (x, y) = self.gui.window_to_movie_position(self.mouse_pos);
        if crate::coop::session().is_some() {
            self.coop_input(PlayerEvent::MouseMove { x, y });
            return;
        }
        self.player.handle_event(PlayerEvent::MouseMove { x, y });
        self.render_movie_if_needed();
    }

    fn render_movie_if_needed(&self) {
        let mut player = self.player.get();
        let player_needs_render = player.as_ref().is_some_and(|p| p.needs_render());
        if player_needs_render
            && !self.minimized
            && !self.occluded
            && let Some(player) = player.as_deref_mut()
        {
            player.render();
        }
        // Ruffle Together: a running session asks for its own repaints (one per frame); egui's
        // repaint wish stays set while those skip it, which would ask on every loop pass
        let egui_repaint = self.gui.needs_render()
            && crate::coop::session().is_none_or(|session| !session.is_running());
        if player_needs_render || egui_repaint {
            self.gui.window().request_redraw();
        }
    }
}

pub struct App {
    main_window: Option<MainWindow>,
    runtime: Option<tokio::runtime::Runtime>,
    gilrs: Option<Gilrs>,
    event_loop_proxy: EventLoopProxy<RuffleEvent>,
    preferences: GlobalPreferences,
    font_database: fontdb::Database,
}

/// Enters the tokio runtime context.
/// This cannot be a method, as the borrow-checker would complain.
macro_rules! enter_runtime {
    ($this:expr) => {
        let _guard = $this.runtime.as_ref().map(|runtime| runtime.enter());
    };
}

impl App {
    pub fn new(preferences: GlobalPreferences) -> Result<(Self, EventLoop<RuffleEvent>), Error> {
        let event_loop = EventLoop::with_user_event().build()?;

        let mut font_database = fontdb::Database::default();
        font_database.load_system_fonts();

        let gilrs = Gilrs::new()
            .inspect_err(|err| {
                tracing::warn!("Gamepad support could not be initialized: {err}");
            })
            .ok();
        let event_loop_proxy = event_loop.create_proxy();
        let runtime = tokio::runtime::Runtime::new()?;

        Ok((
            Self {
                main_window: None,
                runtime: Some(runtime),
                gilrs,
                event_loop_proxy,
                font_database,
                preferences,
            },
            event_loop,
        ))
    }
}

impl ApplicationHandler<RuffleEvent> for App {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        enter_runtime!(self);

        if cause == StartCause::Init {
            let movie_url = self.preferences.cli.movie_url.clone();
            let icon_bytes = include_bytes!("../assets/favicon-32.rgba");
            let icon =
                Icon::from_rgba(icon_bytes.to_vec(), 32, 32).expect("App icon should be correct");

            let no_gui = self.preferences.cli.no_gui;
            let min_window_size = if no_gui {
                (16, 16)
            } else {
                (350, MENU_HEIGHT + 16)
            }
            .into();
            let preferred_width = self.preferences.cli.width;
            let preferred_height = self.preferences.cli.height;
            let start_fullscreen = self.preferences.cli.fullscreen;

            #[cfg_attr(not(target_os = "linux"), allow(unused_mut))]
            let mut window_attributes = WindowAttributes::default()
                .with_visible(false)
                .with_title("Ruffle")
                .with_window_icon(Some(icon))
                .with_min_inner_size(min_window_size);

            #[cfg(target_os = "linux")]
            {
                use winit::platform::startup_notify::{
                    self, EventLoopExtStartupNotify, WindowAttributesExtStartupNotify,
                };
                use winit::platform::wayland::WindowAttributesExtWayland;
                window_attributes = window_attributes.with_name("rs.ruffle.Ruffle", "main");
                if let Some(token) = event_loop.read_token_from_env() {
                    startup_notify::reset_activation_token_env();
                    window_attributes = window_attributes.with_activation_token(token);
                }
            }

            let event_loop_proxy = self.event_loop_proxy.clone();
            let preferences = self.preferences.clone();
            let window = event_loop
                .create_window(window_attributes)
                .expect("Window should be created");
            let max_window_size = get_screen_size(&window);
            window.set_max_inner_size(Some(max_window_size));
            let window = Arc::new(window);
            let font_database = self.font_database.clone();

            let mut gui = GuiController::new(
                window.clone(),
                event_loop_proxy.clone(),
                preferences.clone(),
                &font_database,
                movie_url.clone(),
                no_gui,
            )
            .expect("GUI controller should be created");

            let mut player = PlayerController::new(
                event_loop_proxy.clone(),
                window.clone(),
                gui.descriptors().clone(),
                font_database,
                preferences.clone(),
                gui.file_picker(),
            );

            // Ruffle Together: while the handshake runs, the window shows the waiting screen
            let coop_waiting = crate::coop::waiting_lobby().is_some();
            if coop_waiting {
                let _ = window.request_inner_size(LogicalSize::new(640.0, 420.0));
                window.set_title("Ruffle Together");
                if start_fullscreen {
                    window.set_fullscreen(Some(Fullscreen::Borderless(None)));
                }
                window.set_visible(true);
            } else if let Some(movie_url) = &movie_url {
                gui.create_movie(
                    &mut player,
                    LaunchOptions::from(&preferences),
                    ContentDescriptor {
                        url: movie_url.clone(),
                        root_content_path: None,
                    },
                );
            } else {
                gui.show_open_dialog();
            }

            // TV mode without a session: the controller presses slot 0's keys of the key map named
            // on the command line, or else the keys most Flash games use
            let tv_keys = (crate::coop::tv::on() && preferences.cli.coop.role().ok().flatten().is_none()).then(|| {
                let named = std::env::args().any(|arg| arg == "--coop-keys" || arg.starts_with("--coop-keys="));
                let game = named
                    .then(|| preferences.cli.coop.game().ok())
                    .flatten()
                    .unwrap_or_else(|| crate::coop::keymap::Game::built_in("controller").expect("built in"));
                (game.keys(), game.buttons_used())
            });

            let mut loaded = LoadingState::Loading;

            if movie_url.is_none() {
                // No SWF provided on command line; show window with dummy movie immediately.
                window.set_visible(true);
                loaded = LoadingState::Loaded;
            }

            self.main_window = Some(MainWindow {
                preferences,
                gui,
                player,
                min_window_size,
                max_window_size,
                no_gui,
                preferred_width,
                preferred_height,
                start_fullscreen,
                loaded,
                minimized: false,
                occluded: false,
                mouse_pos: PhysicalPosition::new(0.0, 0.0),
                mouse_move_pending: false,
                modifiers: Modifiers::default(),
                time: Instant::now(),
                next_frame_time: None,
                coop_title: String::new(),
                coop_waiting,
                tv_keys,
                tv_pad_named: false,
                event_loop_proxy,
            });
        }
    }

    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: RuffleEvent) {
        enter_runtime!(self);

        match (&mut self.main_window, event) {
            (Some(main_window), RuffleEvent::TaskPoll(task)) => main_window.player.poll(task),

            (Some(main_window), RuffleEvent::OnMetadata(swf_header)) => {
                main_window.on_metadata(swf_header)
            }

            (Some(main_window), RuffleEvent::ContextMenuItemClicked(index)) => {
                if let Some(mut player) = main_window.player.get() {
                    player.run_context_menu_callback(index);
                }
            }

            (Some(main_window), RuffleEvent::BrowseAndOpen(options, open_type)) => {
                let event_loop = main_window.event_loop_proxy.clone();
                let picker = main_window.gui.file_picker();
                tokio::spawn(async move {
                    let picked = match open_type {
                        OpenType::File => {
                            picker.pick_ruffle_file(None).await.map(|file| (file, None))
                        }
                        OpenType::Directory => picker
                            .pick_ruffle_directory_and_content(None)
                            .await
                            .map(|(dir, file)| (file, Some(dir))),
                    };

                    if let Some(desc) =
                        picked.and_then(|(file, dir)| ContentDescriptor::new_local(&file, dir))
                    {
                        let _ = event_loop.send_event(RuffleEvent::Open(desc, options));
                    }
                });
            }

            (Some(main_window), RuffleEvent::Open(desc, options))
                if crate::coop::session().is_none() || main_window.player.get().is_none() =>
            {
                main_window
                    .gui
                    .create_movie(&mut main_window.player, *options, desc);
            }

            (Some(main_window), RuffleEvent::OpenDialog(descriptor)) => {
                main_window.gui.open_dialog(descriptor);
            }

            (Some(main_window), RuffleEvent::CloseFile) if crate::coop::session().is_none() => {
                main_window.gui.window().set_title("Ruffle"); // Reset title since file has been closed.
                main_window.gui.close_movie(&mut main_window.player);
            }

            (Some(main_window), RuffleEvent::ExportBundle) => {
                main_window.gui.export_bundle();
            }

            (Some(main_window), RuffleEvent::EnterFullScreen) if crate::coop::session().is_none() => {
                if let Some(mut player) = main_window.player.get()
                    && player.is_playing()
                {
                    player.set_fullscreen(true);
                }
            }

            // TV mode stays full screen (Esc is a game key; Alt+F4 leaves)
            (Some(main_window), RuffleEvent::ExitFullScreen)
                if crate::coop::session().is_none() && !crate::coop::tv::on() =>
            {
                if let Some(mut player) = main_window.player.get()
                    && player.is_playing()
                {
                    player.set_fullscreen(false);
                }
            }

            (Some(main_window), RuffleEvent::PlayerNotification(notification)) => {
                match notification {
                    PlayerNotification::ImeNotification(ImeNotification::ImeReady {
                        purpose,
                        cursor_area,
                    }) => {
                        let ime_enabled = main_window.preferences.ime_enabled().unwrap_or(false);
                        main_window.gui.set_ime_allowed(ime_enabled);
                        main_window.gui.set_ime_purpose(purpose);
                        main_window.gui.set_ime_cursor_area(cursor_area);
                    }
                    PlayerNotification::ImeNotification(ImeNotification::ImePurposeUpdated(
                        purpose,
                    )) => {
                        main_window.gui.set_ime_purpose(purpose);
                    }
                    PlayerNotification::ImeNotification(ImeNotification::ImeCursorAreaUpdated(
                        cursor_area,
                    )) => {
                        main_window.gui.set_ime_cursor_area(cursor_area);
                    }
                    PlayerNotification::ImeNotification(ImeNotification::ImeNotReady) => {
                        main_window.gui.set_ime_allowed(false);
                    }
                }
            }

            (_, RuffleEvent::ExitRequested) => {
                event_loop.exit();
            }

            _ => {}
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        enter_runtime!(self);

        if let Some(main_window) = &mut self.main_window {
            main_window.window_event(event_loop, event);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        enter_runtime!(self);

        if let Some(main_window) = &mut self.main_window {
            main_window.about_to_wait(self.gilrs.as_mut());

            // The event loop is finished; let's find out how long we need to wait for.
            // We don't need to worry about earlier update requests, as it's the
            // only place where we're setting control flow, and events cancel wait.
            // Note: the control flow might be set to `ControlFlow::WaitUntil` with a
            // timestamp in the past! Take that into consideration when changing this code.
            if let Some(next_frame_time) = main_window.next_frame_time {
                event_loop.set_control_flow(ControlFlow::WaitUntil(next_frame_time));
            }
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        // `MainWindow` needs a tokio context to properly drop.
        {
            enter_runtime!(self);
            let _ = self.main_window.take();
        }

        // Manually stop the tokio runtime: this makes sure that any pending Player-bound futures
        // are properly cancelled and put back on the winit event loop before it closes, preventing
        // them from being dropped on the wrong thread and causing a panic.
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_timeout(std::time::Duration::from_secs(1));
        }
    }
}

enum LoadingState {
    Loading,
    WaitingForResize,
    Loaded,
}
