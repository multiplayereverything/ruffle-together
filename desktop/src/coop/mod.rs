//! Ruffle Together: lockstep online play for local-multiplayer Flash games.
//! Every PC runs the same game; only inputs travel, and a frame runs once all players' inputs for it are in.

pub mod api;
pub mod discord;
pub mod frame;
pub mod keymap;
pub mod lockstep;
pub mod mice;
pub mod navigator;
pub mod net;
pub mod protocol;
pub mod script;
pub mod session;
pub mod storage;
pub mod replay;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

/// The session this process plays in, if any (set once, when the handshake is done).
static SESSION: OnceLock<Mutex<session::Session>> = OnceLock::new();

pub fn set_session(session: session::Session) {
    let _ = GOODBYE.set(session.sender());
    let _ = SESSION.set(Mutex::new(session));
}

/// The session's link, kept apart from the session so a goodbye still goes out when the
/// session can't be reached (a crash while it is locked). The session itself is never dropped.
static GOODBYE: OnceLock<(net::LinkSender, u8)> = OnceLock::new();
static SAID_GOODBYE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Tells the other players why this PC leaves ("closed the game", "Ruffle crashed: ..."), once,
/// so their log says so instead of a connection that just closed.
pub fn goodbye(reason: &str) {
    if let Some((sender, player)) = GOODBYE.get()
        && !SAID_GOODBYE.swap(true, std::sync::atomic::Ordering::Relaxed)
    {
        tracing::warn!("Ruffle Together: leaving: {reason}");
        sender.send(&protocol::Msg::Bye {
            player: *player,
            reason: reason.to_string(),
        });
    }
}

/// The mouse debug overlay (`--coop-mouse-debug`): every player's mouse, what it is on, and whether
/// the rules let it be there.
static MOUSE_DEBUG: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_mouse_debug(on: bool) {
    MOUSE_DEBUG.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub fn mouse_debug() -> bool {
    MOUSE_DEBUG.load(std::sync::atomic::Ordering::Relaxed)
}

/// The running session; None in normal (single-player) use, where nothing changes.
pub fn session() -> Option<MutexGuard<'static, session::Session>> {
    SESSION.get().and_then(|session| session.lock().ok())
}

/// What the window shows while the handshake runs on its own thread: the room code, who has
/// joined, or why it failed.
#[derive(Clone, Debug, Default)]
pub struct Lobby {
    pub hosting: bool,
    /// Connected to the host or relay (a friend), or the room is open (the host).
    pub connected: bool,
    /// The relay room code.
    pub room: Option<String>,
    /// Hosting directly: the port friends connect to.
    pub port: Option<u16>,
    /// The players in so far (the host first), and how many the game starts with (a friend
    /// hears both from the host; 0 until then).
    pub players: Vec<String>,
    pub needed: u8,
    /// The handshake failed: why.
    pub error: Option<String>,
}

static LOBBY: OnceLock<Arc<Mutex<Lobby>>> = OnceLock::new();

pub fn set_lobby(lobby: Arc<Mutex<Lobby>>) {
    let _ = LOBBY.set(lobby);
}

/// The lobby while the window waits for the session (None once the game runs, and in normal use).
pub fn waiting_lobby() -> Option<Lobby> {
    if SESSION.get().is_some() {
        return None;
    }
    LOBBY.get().and_then(|lobby| lobby.lock().ok()).map(|lobby| lobby.clone())
}

#[derive(clap::Args, Debug, Clone, Default)]
pub struct CoopOpt {
    /// Testing: run SWF headless with scripted input (--coop-script) and write the game's
    /// fingerprint for every frame (--coop-out), to prove the game runs the same every time.
    #[clap(long, value_name = "SWF", help_heading = "Ruffle Together")]
    pub coop_replay: Option<PathBuf>,

    /// Host a session; friends connect directly to this port (LAN, Radmin, or a forwarded port).
    #[clap(long, value_name = "PORT", help_heading = "Ruffle Together")]
    pub coop_host: Option<u16>,

    /// Host a session through a relay (host:port); prints the room code friends type.
    #[clap(long, value_name = "RELAY", help_heading = "Ruffle Together")]
    pub coop_host_relay: Option<String>,

    /// Join a session directly (host IP:port).
    #[clap(long, value_name = "ADDRESS", help_heading = "Ruffle Together")]
    pub coop_join: Option<String>,

    /// Join a session through a relay (host:port), with --coop-code.
    #[clap(long, value_name = "RELAY", help_heading = "Ruffle Together")]
    pub coop_join_relay: Option<String>,

    /// The room code to join through the relay.
    #[clap(long, value_name = "CODE", help_heading = "Ruffle Together")]
    pub coop_code: Option<String>,

    /// Host: input delay in frames (the same for every player); without it the host measures
    /// the ping when the friend connects and picks the smallest delay that runs smoothly.
    #[clap(long, help_heading = "Ruffle Together")]
    pub coop_delay: Option<u32>,

    /// Host: how many players the game starts with, 2 to 8 (as many as the game has keys for);
    /// it starts once all have joined.
    #[clap(long, default_value_t = 2, help_heading = "Ruffle Together")]
    pub coop_players: u8,

    /// Host: which save everyone starts from. coop: this game's co-op save (the solo save the
    /// first time); solo: the solo save; new: no save. With solo or new, the co-op save is
    /// replaced the first time the game saves.
    #[clap(long, value_enum, default_value_t = storage::SaveStart::Coop, help_heading = "Ruffle Together")]
    pub coop_save: storage::SaveStart,

    /// The game's keys: bad-ice-cream or gun-mayhem-2 (every player must use the same).
    #[clap(long, default_value = "bad-ice-cream", help_heading = "Ruffle Together")]
    pub coop_keys: String,

    /// Host: a game setting every player gets, as NAME=VALUE (repeat it for more); the game
    /// reads it with coop.option("NAME"). Friends' own are ignored.
    #[clap(long = "coop-option", value_name = "NAME=VALUE", help_heading = "Ruffle Together")]
    pub coop_option: Vec<String>,

    /// The game's id in mods-info (e.g. gm2), from the launcher: a relay host's Discord profile
    /// then gets a Join button (ruffle-together://join/ID/CODE through the join page).
    #[clap(long, value_name = "ID", help_heading = "Ruffle Together")]
    pub coop_link_id: Option<String>,

    /// Testing: show every player's mouse, what it is on, and whether the rules let it be there.
    #[clap(long, help_heading = "Ruffle Together")]
    pub coop_mouse_debug: bool,

    /// Your name, shown to the other players.
    #[clap(long, default_value = "Player", help_heading = "Ruffle Together")]
    pub coop_name: String,

    /// Testing: play the session without a window for --coop-frames frames, then print the
    /// result (sync, input delay) and exit.
    #[clap(long, help_heading = "Ruffle Together")]
    pub coop_headless: bool,

    /// Testing: scripted input instead of the keyboard and mouse.
    #[clap(long, value_name = "FILE", help_heading = "Ruffle Together")]
    pub coop_script: Option<PathBuf>,

    /// Replay: number of frames to run.
    #[clap(long, default_value_t = 1500, help_heading = "Ruffle Together")]
    pub coop_frames: u32,

    /// Replay: sleep a random 0..N ms between frames (the result must not change).
    #[clap(long, default_value_t = 0, help_heading = "Ruffle Together")]
    pub coop_jitter_ms: u64,

    /// Replay: use the real clock (upstream behavior), as the control run that must differ.
    #[clap(long, help_heading = "Ruffle Together")]
    pub coop_no_clock: bool,

    /// Replay: the date the game sees at frame 0, in Unix ms (sessions use the host's).
    #[clap(long, default_value_t = 1_700_000_000_000, help_heading = "Ruffle Together")]
    pub coop_start_ms: i64,

    /// Replay: where to write "frame fingerprint" lines.
    #[clap(long, value_name = "FILE", help_heading = "Ruffle Together")]
    pub coop_out: Option<PathBuf>,

    /// Replay: frames to save as PNG next to --coop-out, e.g. 100,250.
    #[clap(long, value_delimiter = ',', help_heading = "Ruffle Together")]
    pub coop_shot: Vec<u32>,
}

impl CoopOpt {
    /// The host's game settings (`--coop-option NAME=VALUE`), in the order given.
    pub fn options(&self) -> anyhow::Result<Vec<(String, String)>> {
        self.coop_option
            .iter()
            .map(|option| {
                let (name, value) = option
                    .split_once('=')
                    .ok_or_else(|| anyhow::anyhow!("--coop-option {option}: expected NAME=VALUE"))?;
                Ok((name.trim().to_string(), value.to_string()))
            })
            .collect()
    }

    /// The game named by --coop-keys.
    pub fn game(&self) -> anyhow::Result<keymap::Game> {
        keymap::Game::load(&self.coop_keys).map_err(|e| anyhow::anyhow!("--coop-keys: {e}"))
    }

    /// --coop-players, checked against the game's keys.
    pub fn players(&self) -> anyhow::Result<u8> {
        let game = self.game()?;
        let max = game.max_players();
        if !(2..=max).contains(&self.coop_players) {
            anyhow::bail!("--coop-players: {} has keys for 2 to {max} players", game.name());
        }
        Ok(self.coop_players)
    }

    /// The session these options ask for, if any.
    pub fn role(&self) -> anyhow::Result<Option<net::Role>> {
        use net::Role;
        let roles = [
            self.coop_host.map(|port| Role::HostDirect { port }),
            self.coop_host_relay.clone().map(|relay| Role::HostRelay { relay }),
            self.coop_join.clone().map(|addr| Role::JoinDirect { addr }),
            self.coop_join_relay.clone().map(|relay| Role::JoinRelay {
                relay,
                code: self.coop_code.clone().unwrap_or_default().to_uppercase(),
            }),
        ];
        let mut roles = roles.into_iter().flatten();
        let role = roles.next();
        if roles.next().is_some() {
            anyhow::bail!("choose one of --coop-host, --coop-host-relay, --coop-join, --coop-join-relay");
        }
        if matches!(role, Some(Role::JoinRelay { ref code, .. }) if code.is_empty()) {
            anyhow::bail!("--coop-join-relay needs --coop-code");
        }
        Ok(role)
    }
}
