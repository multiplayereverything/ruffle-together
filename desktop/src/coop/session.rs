//! A Ruffle Together session: the handshake, then every frame in lockstep with the other
//! players, a sync check every 25 frames, and what happens when someone leaves.

use crate::coop::frame::Stepper;
use crate::coop::keymap::Game;
use crate::coop::lockstep::{InputFrame, Lockstep, Typed};
use crate::coop::net::{Event, Link, LinkSender, NO_GOODBYE, Role, connect};
use crate::coop::protocol::Msg;
use crate::coop::script::Script;
use crate::coop::storage::{SaveGate, SaveStart, SessionStorage, coop_save_file};
use anyhow::{Result, anyhow, bail};
use ruffle_core::Player;
use ruffle_core::events::PhysicalKey;
use ruffle_core::frame_clock::FrameClock;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The sync check runs every this many frames.
const CHECK_EVERY: u32 = 25;
/// Most frames run in one tick when catching up after a stall.
const MAX_CATCH_UP: u32 = 10;
/// "Waiting for <name>" shows after this long without a frame.
const SHOW_WAITING: Duration = Duration::from_secs(2);
/// A player this long silent while the game waits is treated as gone. A player who leaves is
/// noticed at once (the connection closes); this is for a connection that stays open but stalls,
/// which an internet or relay hiccup does for several seconds (5 s ended a game over the relay).
const GIVE_UP: Duration = Duration::from_secs(60);
/// A stall this long is explained once in the log: frozen game, or nothing at all.
const EXPLAIN_STALL: Duration = Duration::from_secs(5);
/// A frame that runs this long on this PC is logged here (the others only see a stall).
const SLOW_FRAME: Duration = Duration::from_secs(2);
/// The heartbeat's period; a player heard from within 3 of them still has Ruffle running.
const HEARTBEAT: Duration = Duration::from_secs(1);
/// Before the start the host tells the waiting friends who has joined this often, so a friend who
/// joined early keeps waiting however long the last one takes.
const LOBBY_EVERY: Duration = Duration::from_secs(2);
/// A friend that hears nothing at all from the host for this long before the start gives up.
const HOST_SILENT: Duration = if cfg!(test) { Duration::from_secs(5) } else { Duration::from_secs(30) };
/// The longest chat line.
const CHAT_MAX: usize = 120;
/// How long a chat line stays on screen.
const CHAT_SHOWN: Duration = Duration::from_secs(12);
/// How often the host measures every friend's round trip (`coop.ping`).
const PING_EVERY: Duration = Duration::from_secs(2);

/// Ruffle Together's own version (the Ruffle it is built on has its own).
pub const VERSION: &str = "1.0.4";

/// This build, as the relay and the other players see it: a different build is refused.
pub fn build_id() -> String {
    format!("rc-{VERSION}#{:.10}", env!("VERGEN_GIT_SHA"))
}

pub struct SessionOpts {
    pub role: Role,
    pub name: String,
    /// Host: how many players the game starts with (2-4); it waits until all have joined.
    pub players: u8,
    /// Input delay in frames (host only; the friend uses the host's); None picks it from the ping.
    pub delay: Option<u32>,
    pub swf: PathBuf,
    /// Whose keys the inputs press.
    pub game: Game,
    /// Ruffle's save folder: the host shares its saves for this game.
    pub save_dir: Option<PathBuf>,
    /// Host: which save the game starts from.
    pub save_start: SaveStart,
    /// Testing: input from a script instead of the keyboard and mouse.
    pub script: Option<Script>,
    /// Host: game settings every player gets (`coop.option`); a friend's are ignored.
    pub options: Vec<(String, String)>,
    /// The game's mods-info id, for Discord's Join button (from the launcher).
    pub link_id: Option<String>,
    /// How long the host waits for a friend (None: until the window closes).
    pub host_wait: Option<Duration>,
    /// Testing: shift this PC's start date, to force a desync.
    pub start_skew_ms: i64,
    /// Run at the game's frame rate (always, except in fast tests).
    pub paced: bool,
    /// Player settings that change how a game behaves, as "name=value; ..." (see
    /// `settings_line`); every player must have the same.
    pub settings: String,
    /// The window's waiting screen: the room code and who has joined, filled in as they happen.
    pub lobby: Option<Arc<Mutex<crate::coop::Lobby>>>,
}

/// The player settings that change how a game behaves, for the handshake.
pub fn settings_line(launch: &crate::player::LaunchOptions) -> String {
    let p = &launch.player;
    [
        format!("flashvars={:?}", p.parameters),
        format!("quality={:?}", p.quality),
        format!("align={:?}", p.align),
        format!("force_align={:?}", p.force_align),
        format!("scale={:?}", p.scale),
        format!("force_scale={:?}", p.force_scale),
        format!("load_behavior={:?}", p.load_behavior),
        format!("letterbox={:?}", p.letterbox),
        format!("player_version={:?}", p.player_version),
        format!("player_runtime={:?}", p.player_runtime),
        format!("frame_rate={:?}", p.frame_rate),
        format!("dummy_external_interface={:?}", p.dummy_external_interface),
        format!("spoof_url={:?}", p.spoof_url.as_ref().map(|u| u.as_str())),
        format!("avm2_optimizer={:?}", launch.avm2_optimizer_enabled),
    ]
    .join("; ")
}

/// The first setting that differs, e.g. "frame_rate (Some(60.0) here, None on the other PC)".
fn settings_difference(mine: &str, theirs: &str) -> Option<String> {
    let theirs: HashMap<&str, &str> =
        theirs.split("; ").filter_map(|kv| kv.split_once('=')).collect();
    mine.split("; ").filter_map(|kv| kv.split_once('=')).find_map(|(name, value)| {
        let other = theirs.get(name).copied().unwrap_or("missing");
        (other != value).then(|| format!("{name} ({value} here, {other} on the other PC)"))
    })
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionState {
    pub waiting_on: Vec<String>,
    pub desync_at: Option<u32>,
    pub closed: Option<String>,
}

pub struct Session {
    link: Link,
    lockstep: Lockstep,
    stepper: Stepper,
    game: Game,
    /// The game's file name without `.swf`, for Discord.
    swf_name: String,
    /// The game's mods-info id, for Discord's Join button.
    link_id: Option<String>,
    local_index: u8,
    names: Vec<String>,
    /// Connection number -> player slot of the session's players (see `from` in net events).
    peers: HashMap<u8, u8>,
    hosting: bool,
    room: Option<String>,
    clock: Arc<Mutex<FrameClock>>,
    storage: Option<SessionStorage>,
    /// Freezes this PC's co-op save once the games are out of sync.
    save_gate: Option<SaveGate>,
    script: Option<Script>,
    /// How many local inputs were sent (the next one is for frame `delay + local_sent`).
    local_sent: u32,
    held: HashSet<PhysicalKey>,
    focused: bool,
    mouse: Option<(i32, i32, bool)>,
    /// What this PC typed and hasn't sent yet (up to `Typed::MAX` go with each frame).
    typed: VecDeque<char>,
    my_prints: HashMap<u32, u64>,
    peer_prints: HashMap<(u8, u32), u64>,
    /// Readable game state at recent check frames, written out when a desync shows up.
    my_states: HashMap<u32, String>,
    last_frame_at: Instant,
    paced: bool,
    /// When the next frame is due (paced sessions).
    next_due: Option<Instant>,
    /// When each local input was taken, by the frame it is for: the time until that frame
    /// runs is the input delay this player feels.
    taken_at: HashMap<u32, Instant>,
    felt_delay: Vec<Duration>,
    timing: Timing,
    picture: PictureGate,
    state: SessionState,
    heartbeat: Arc<Heartbeat>,
    /// When each player's heartbeat last came, and the frame their game was at.
    alive: HashMap<u8, (Instant, u32)>,
    /// Each player's round trip to the host (the host measures, then tells everyone).
    round_trips: [Option<u32>; crate::coop::lockstep::MAX_PLAYERS],
    /// Host: the last ping sent, when, and its number.
    ping_sent: Option<(u32, Instant)>,
    /// A short message for the window ("Sam left"), and when it came.
    notice: Option<(String, Instant)>,
    /// The chat box while it is open (what's typed so far); keys go there, not to the game.
    chat_box: Option<String>,
    /// This PC's chat line, sent with the next input.
    chat_to_send: Option<String>,
    /// Chat lines by the frame the games hear them on.
    chat_at: std::collections::BTreeMap<u32, Vec<(u8, String)>>,
    /// Chat lines heard (name, text, when), for the window.
    chat_log: VecDeque<(String, String, Instant)>,
    /// Each player's mouse input and when it last moved, so an idle friend's cursor fades.
    cursor_moved: [Option<((i32, i32), Instant)>; crate::coop::lockstep::MAX_PLAYERS],
    stall_explained: bool,
    /// Tests: leave without a goodbye, as a killed Ruffle does.
    #[cfg(test)]
    vanish: bool,
}

/// Another player's cursor as this PC draws it.
#[derive(Clone, Debug, PartialEq)]
pub struct FriendCursor {
    pub name: String,
    pub slot: u8,
    /// Stage twips.
    pub x: i32,
    pub y: i32,
    pub down: bool,
    /// 1 shown, fading to 0 while it doesn't move.
    pub alpha: f32,
}

/// What the heartbeat thread knows about this PC's game, shared with the session.
struct Heartbeat {
    epoch: Instant,
    frame: AtomicU32,
    /// When the frame that runs now started (ms after `epoch`, plus 1), or 0 between frames.
    step_started: AtomicU64,
}

impl Heartbeat {
    fn ms(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64 + 1
    }
}

/// Every second, whatever the game does: tells the others this PC's Ruffle is running (and at
/// which frame), and logs here when this PC's own game is stuck in one frame. Ends with the
/// session.
fn start_heartbeat(heartbeat: Weak<Heartbeat>, sender: LinkSender, player: u8) {
    std::thread::spawn(move || {
        let mut logged_for = 0;
        while let Some(beat) = heartbeat.upgrade() {
            let frame = beat.frame.load(Ordering::Relaxed);
            sender.send(&Msg::Alive { player, frame });
            let started = beat.step_started.load(Ordering::Relaxed);
            if started != 0 {
                let stuck = Duration::from_millis(beat.ms().saturating_sub(started));
                if stuck > EXPLAIN_STALL && logged_for != started {
                    tracing::warn!(
                        "Ruffle Together: this PC's game has been running frame {frame} for {} s \
                         (stuck in the game's own scripts); the other players are waiting",
                        stuck.as_secs()
                    );
                    logged_for = started;
                }
            }
            drop(beat);
            std::thread::sleep(HEARTBEAT);
        }
    });
}

/// Why `name`'s input hasn't come for `stalled`, from their heartbeat: `alive` is how long ago
/// it last came and the frame their game was at then.
fn stall_reason(name: &str, stalled: Duration, alive: Option<(Duration, u32)>) -> String {
    match alive {
        Some((ago, frame)) if ago < HEARTBEAT * 3 => format!(
            "{name}'s Ruffle is still running, but its game hasn't moved past frame {frame} for {} s: \
             it froze on that PC (its log says more)",
            stalled.as_secs()
        ),
        Some((ago, _)) => format!(
            "nothing at all from {name} for {} s: its Ruffle stopped (crashed, killed or frozen hard) \
             or the network went down",
            ago.as_secs()
        ),
        None => format!(
            "nothing at all from {name} for {} s: its Ruffle stopped or the network went down",
            stalled.as_secs()
        ),
    }
}

/// When the window shows a picture in a session: once per new game frame. Every key press and
/// mouse move also asks for a repaint, and each repaint waits for the screen's refresh, which
/// held up the game's frames (stutter while playing). A picture with no new frame only goes
/// through once it is `STALE`, so a resized or uncovered window still repaints while the game waits.
#[derive(Default)]
struct PictureGate {
    frame: Option<u32>,
    at: Option<Instant>,
}

impl PictureGate {
    const STALE: Duration = Duration::from_millis(100);

    fn allow(&mut self, frame: u32, now: Instant) -> bool {
        let new_frame = self.frame != Some(frame);
        let stale = self.at.is_none_or(|at| now - at >= Self::STALE);
        if new_frame || stale {
            self.frame = Some(frame);
            self.at = Some(now);
        }
        new_frame || stale
    }
}

/// Where the time goes, logged every few seconds, so a slow session shows why.
struct Timing {
    since: Instant,
    last_tick: Option<Instant>,
    ticks: u32,
    frames: u32,
    /// Times the window put a picture on screen.
    shown: u32,
    longest_tick_gap: Duration,
    step_total: Duration,
    step_longest: Duration,
    /// Time a frame was due but someone's input wasn't in yet.
    waiting: Duration,
    longest_wait: Duration,
    waiting_since: Option<Instant>,
    /// Frames that ran more than 1.25 frame times after the previous one (a visible hitch).
    late: u32,
    /// Ticks that ran 2+ frames, so a picture was skipped.
    doubled: u32,
    last_step: Option<Instant>,
    frames_this_tick: u32,
    /// Input taken -> its frame runs, for this PC's inputs.
    felt_total: Duration,
    felt_longest: Duration,
    felt_count: u32,
}

impl Timing {
    const EVERY: Duration = Duration::from_secs(5);

    fn new() -> Self {
        Self {
            since: Instant::now(),
            last_tick: None,
            ticks: 0,
            frames: 0,
            shown: 0,
            longest_tick_gap: Duration::ZERO,
            step_total: Duration::ZERO,
            step_longest: Duration::ZERO,
            waiting: Duration::ZERO,
            longest_wait: Duration::ZERO,
            waiting_since: None,
            late: 0,
            doubled: 0,
            last_step: None,
            frames_this_tick: 0,
            felt_total: Duration::ZERO,
            felt_longest: Duration::ZERO,
            felt_count: 0,
        }
    }

    fn tick(&mut self, now: Instant) {
        if let Some(last) = self.last_tick {
            self.longest_tick_gap = self.longest_tick_gap.max(now - last);
        }
        self.last_tick = Some(now);
        self.ticks += 1;
        self.frames_this_tick = 0;
    }

    fn step(&mut self, started: Instant, took: Duration, frame_time: Duration) {
        if self.last_step.is_some_and(|last| started - last > frame_time * 5 / 4) {
            self.late += 1;
        }
        self.last_step = Some(started);
        self.frames_this_tick += 1;
        if self.frames_this_tick == 2 {
            self.doubled += 1;
        }
        self.frames += 1;
        self.step_total += took;
        self.step_longest = self.step_longest.max(took);
    }

    fn felt(&mut self, delay: Duration) {
        self.felt_total += delay;
        self.felt_longest = self.felt_longest.max(delay);
        self.felt_count += 1;
    }

    fn waiting(&mut self, now: Instant, waiting: bool) {
        match (waiting, self.waiting_since) {
            (true, None) => self.waiting_since = Some(now),
            (false, Some(since)) => {
                self.waiting += now - since;
                self.longest_wait = self.longest_wait.max(now - since);
                self.waiting_since = None;
            }
            _ => {}
        }
    }

    /// The summary line once per `EVERY`, then starts over.
    fn report(&mut self, now: Instant, frame_rate: f64) -> Option<String> {
        let span = now - self.since;
        if span < Self::EVERY {
            return None;
        }
        if let Some(since) = self.waiting_since {
            self.waiting += now - since;
            self.longest_wait = self.longest_wait.max(now - since);
        }
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        let line = format!(
            "Ruffle Together timing: {:.1} fps (game {:.0}), {:.1} shown per second, {} ticks, longest tick gap {:.0} ms, \
             frame cost avg {:.1} / max {:.0} ms, waited for input {:.0} ms of {:.0} (longest {:.0} ms), {} late frames, {} doubled ticks, input delay avg {:.0} / max {:.0} ms",
            self.frames as f64 / span.as_secs_f64(),
            frame_rate,
            self.shown as f64 / span.as_secs_f64(),
            self.ticks,
            ms(self.longest_tick_gap),
            ms(self.step_total) / self.frames.max(1) as f64,
            ms(self.step_longest),
            ms(self.waiting),
            ms(span),
            ms(self.longest_wait),
            self.late,
            self.doubled,
            ms(self.felt_total) / self.felt_count.max(1) as f64,
            ms(self.felt_longest),
        );
        let still_waiting = self.waiting_since.is_some();
        let last_step = self.last_step;
        *self = Self::new();
        self.last_step = last_step;
        self.since = now;
        self.last_tick = Some(now);
        self.waiting_since = still_waiting.then_some(now);
        Some(line)
    }
}

/// Pings the host sends before the start; the worst round trip picks the input delay.
const PINGS: u32 = 5;
/// Room for jitter on top of the worst ping.
const JITTER_ROOM: Duration = Duration::from_millis(15);
/// The delay when no ping answer comes back.
const FALLBACK_DELAY: u32 = 3;

/// The smallest input delay that runs clean at this ping: a delay of d frames has 2d frames
/// minus the ping to spare (measured with tools/coop-lab/lab.py through a laggy relay: delay 1
/// stalled at 60-70 ms, delay 2 at 140-150 ms, delay 3 ran clean at 160-170 ms).
fn auto_delay(worst_ping: Duration, frame: Duration) -> u32 {
    let frames = (worst_ping + JITTER_ROOM).as_secs_f64() / (2.0 * frame.as_secs_f64());
    (frames.ceil() as u32).clamp(1, 10)
}

/// The game's frame time, from the SWF header.
fn swf_frame_time(path: &Path) -> Result<Duration> {
    let swf = ruffle_core::swf::decompress_swf(std::fs::File::open(path)?)?;
    let rate = swf.header.frame_rate().to_f64();
    if rate <= 0.0 {
        bail!("the game file has no frame rate");
    }
    Ok(Duration::from_secs_f64(1.0 / rate))
}

/// Host: each friend's worst round trip over a few pings, None if one doesn't answer.
fn measure_ping(link: &Link, friends: &[u8]) -> Option<Vec<Duration>> {
    let mut worst: HashMap<u8, Duration> = HashMap::new();
    for seq in 0..PINGS {
        let sent = Instant::now();
        link.send(&Msg::Ping { seq });
        let mut waiting: HashSet<u8> = friends.iter().copied().collect();
        while !waiting.is_empty() {
            let left = Duration::from_secs(2).checked_sub(sent.elapsed())?;
            match next_event(link, left) {
                Some(Event::Msg { from, msg: Msg::Pong { seq: s } }) if s == seq && waiting.remove(&from) => {
                    let ping = worst.entry(from).or_default();
                    *ping = (*ping).max(sent.elapsed());
                }
                Some(_) => {}
                None => return None,
            }
        }
    }
    Some(friends.iter().map(|f| worst[f]).collect())
}

/// The longest trip a player's keys take: to the host and back with one friend; with more, a
/// friend's keys reach another friend through the host, so the two worst pings added.
fn path_ping(pings: &[Duration]) -> Duration {
    let mut pings = pings.to_vec();
    pings.sort_by(|a, b| b.cmp(a));
    pings.iter().take(2).sum()
}

fn hash_file(path: &Path) -> Result<String> {
    let data = std::fs::read(path).map_err(|e| anyhow!("can't read {}: {e}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(data)))
}

fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

/// The next event, waiting up to `timeout`.
fn next_event(link: &Link, timeout: Duration) -> Option<Event> {
    let end = Instant::now() + timeout;
    loop {
        if let Some(event) = link.try_recv() {
            return Some(event);
        }
        if Instant::now() >= end {
            return None;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

impl Session {
    /// Connects and does the handshake; returns once the game can start.
    pub fn start(opts: SessionOpts) -> Result<Session> {
        let build = build_id();
        let swf_sha256 = hash_file(&opts.swf)?;
        let hosting = matches!(opts.role, Role::HostDirect { .. } | Role::HostRelay { .. });
        let link = connect(opts.role, &build, &opts.name)?;
        let room = link.room_code();
        let update_lobby = |f: &dyn Fn(&mut crate::coop::Lobby)| {
            if let Some(lobby) = &opts.lobby
                && let Ok(mut lobby) = lobby.lock()
            {
                f(&mut lobby);
            }
        };
        update_lobby(&|lobby| {
            lobby.connected = true;
            lobby.room = room.clone();
            lobby.players = vec![opts.name.clone()];
        });
        if let Some(code) = &room {
            tracing::info!("Ruffle Together: room code {code}");
            println!("Ruffle Together room code: {code}");
        }
        let settings = opts.settings.replace('|', " ");
        // the host is 0; a friend picks a random id that names it until the start
        let my_id = if hosting { 0 } else { rand::random::<u64>() | 1 };
        let me = Msg::Hello {
            build: build.clone(),
            swf_sha256: swf_sha256.clone(),
            settings: settings.clone(),
            name: opts.name.clone(),
            id: my_id,
        };
        let mismatch = |their_build: &str, their_swf: &str, their_settings: &str| {
            if their_build != build {
                Some(format!("different Ruffle Together build ({their_build}, this one is {build})"))
            } else if their_swf != swf_sha256 {
                Some("different game file".to_string())
            } else {
                settings_difference(&settings, their_settings)
                    .map(|what| format!("different player settings: {what}"))
            }
        };

        let (start, peers) = if hosting {
            let deadline = opts.host_wait.map(|wait| Instant::now() + wait);
            // accepted friends in join order: connection, Hello id, name
            let mut friends: Vec<(u8, u64, String)> = vec![];
            let mut told: Option<(Instant, Vec<String>)> = None;
            while friends.len() + 1 < opts.players as usize {
                let mut names = vec![opts.name.clone()];
                names.extend(friends.iter().map(|(_, _, name)| name.clone()));
                update_lobby(&|lobby| lobby.players = names.clone());
                if told.as_ref().is_none_or(|(at, was)| at.elapsed() >= LOBBY_EVERY || *was != names) {
                    link.send(&Msg::Lobby {
                        needed: opts.players,
                        players: names.clone(),
                    });
                    told = Some((Instant::now(), names));
                }
                let left = deadline.map_or(Duration::from_secs(3600), |d| {
                    d.saturating_duration_since(Instant::now())
                });
                if left.is_zero() {
                    bail!("not everyone joined ({} of {} players)", friends.len() + 1, opts.players);
                }
                match next_event(&link, left.min(Duration::from_millis(500))) {
                    Some(Event::Msg {
                        from,
                        msg: Msg::Hello { build: b, swf_sha256: s, settings: o, name, id },
                    }) => {
                        if let Some(reason) = mismatch(&b, &s, &o) {
                            tracing::warn!("Ruffle Together: refused {name}: {reason}");
                            link.send(&Msg::Refuse { id, reason });
                            continue;
                        }
                        friends.retain(|(conn, ..)| *conn != from);
                        friends.push((from, id, name));
                    }
                    Some(Event::Left { player: conn }) => friends.retain(|(c, ..)| *c != conn),
                    Some(Event::Closed(reason)) => bail!("{reason}"),
                    _ => {}
                }
            }
            let save = match &opts.save_dir {
                Some(dir) => SessionStorage::export_for_session(
                    dir,
                    &opts.swf.file_name().unwrap_or_default().to_string_lossy(),
                    opts.save_start,
                ),
                None => vec![],
            };
            // the game starts: only these friends stay connected, and nobody new
            let conns: Vec<u8> = friends.iter().map(|(conn, ..)| *conn).collect();
            link.keep_only(&conns);
            link.stop_accepting();
            let delay = match opts.delay {
                Some(delay) => delay,
                None => match (measure_ping(&link, &conns), swf_frame_time(&opts.swf)) {
                    (Some(pings), Ok(frame)) => {
                        let ping = path_ping(&pings);
                        let delay = auto_delay(ping, frame);
                        tracing::info!(
                            "Ruffle Together: ping {} ms, input delay {delay} frames",
                            ping.as_millis()
                        );
                        delay
                    }
                    _ => {
                        tracing::warn!("Ruffle Together: no ping answer, input delay {FALLBACK_DELAY} frames");
                        FALLBACK_DELAY
                    }
                },
            };
            let mut players = vec![(0, opts.name.clone())];
            players.extend(friends.iter().map(|(_, id, name)| (*id, name.clone())));
            let start = Msg::Start {
                // a frame's own input is read one frame ahead at the latest
                delay: delay.max(1),
                start_unix_ms: now_unix_ms(),
                frame_rate_milli: 0,
                save,
                players,
                options: opts.options.clone(),
            };
            link.send(&me);
            link.send(&start);
            // the host knows its friends by connection number; their slots follow join order
            let peers = conns.iter().enumerate().map(|(i, conn)| (*conn, i as u8 + 1)).collect();
            (start, peers)
        } else {
            link.send(&me);
            loop {
                match next_event(&link, HOST_SILENT) {
                    Some(Event::Msg { msg: Msg::Refuse { id, reason }, .. }) if id == my_id => bail!("{reason}"),
                    Some(Event::Msg { msg: Msg::Lobby { needed, players }, .. }) => {
                        update_lobby(&|lobby| {
                            lobby.needed = needed;
                            lobby.players = players.clone();
                        });
                    }
                    // only the host's Hello (id 0) is checked: the host passes the other friends' on
                    Some(Event::Msg {
                        msg: Msg::Hello { build: b, swf_sha256: s, settings: o, id: 0, .. },
                        ..
                    }) => {
                        if let Some(reason) = mismatch(&b, &s, &o) {
                            bail!("{reason}");
                        }
                    }
                    Some(Event::Msg { msg: Msg::Ping { seq }, .. }) => link.send(&Msg::Pong { seq }),
                    Some(Event::Msg { msg: start @ Msg::Start { .. }, .. }) => {
                        // a friend hears everything from the host
                        break (start, HashMap::from([(0, 0)]));
                    }
                    Some(Event::Closed(reason)) => bail!("{reason}"),
                    Some(_) => {}
                    None => bail!("the host didn't answer"),
                }
            }
        };

        let Msg::Start { delay, start_unix_ms, save, players, options, .. } = start else {
            unreachable!("start is always a Start message");
        };
        let player_count = players.len() as u8;
        let your_index = players
            .iter()
            .position(|(id, _)| *id == my_id)
            .ok_or_else(|| anyhow!("the game started without you (the room is full)"))? as u8;
        let names: Vec<String> = players.into_iter().map(|(_, name)| name).collect();
        let mut stepper = Stepper::new(opts.game.keys(), player_count, your_index);
        stepper.set_names(&names);
        for slot in 0..player_count {
            stepper.add_event(format!("join {slot}"));
        }
        stepper.set_options(&options);
        let storage = SessionStorage::from_bytes(&save).ok_or_else(|| anyhow!("bad save data"))?;
        // every PC keeps what the game saves in its own co-op save for this game
        let (storage, save_gate) = match &opts.save_dir {
            Some(dir) => {
                let file = coop_save_file(dir, &opts.swf.file_name().unwrap_or_default().to_string_lossy());
                let (storage, gate) = storage.with_coop_save(file);
                (storage, Some(gate))
            }
            None => (storage, None),
        };
        tracing::info!(
            "Ruffle Together: playing as player {} of {player_count}, input delay {delay} frames",
            your_index + 1
        );
        let heartbeat = Arc::new(Heartbeat {
            epoch: Instant::now(),
            frame: AtomicU32::new(0),
            step_started: AtomicU64::new(0),
        });
        start_heartbeat(Arc::downgrade(&heartbeat), link.sender(), your_index);
        Ok(Session {
            link,
            lockstep: Lockstep::new(player_count, your_index, delay),
            stepper,
            game: opts.game.clone(),
            swf_name: opts.swf.file_stem().unwrap_or_default().to_string_lossy().into_owned(),
            link_id: opts.link_id.clone(),
            local_index: your_index,
            peers,
            hosting,
            names,
            room,
            clock: Arc::new(Mutex::new(FrameClock::new(start_unix_ms + opts.start_skew_ms))),
            storage: Some(storage),
            save_gate,
            script: opts.script,
            local_sent: 0,
            held: HashSet::new(),
            focused: true,
            mouse: None,
            typed: VecDeque::new(),
            my_prints: HashMap::new(),
            peer_prints: HashMap::new(),
            my_states: HashMap::new(),
            last_frame_at: Instant::now(),
            paced: opts.paced,
            next_due: None,
            taken_at: HashMap::new(),
            felt_delay: vec![],
            timing: Timing::new(),
            picture: PictureGate::default(),
            state: SessionState::default(),
            heartbeat,
            alive: HashMap::new(),
            round_trips: [None; crate::coop::lockstep::MAX_PLAYERS],
            ping_sent: None,
            notice: None,
            chat_box: None,
            chat_to_send: None,
            chat_at: Default::default(),
            chat_log: VecDeque::new(),
            cursor_moved: [None; crate::coop::lockstep::MAX_PLAYERS],
            stall_explained: false,
            #[cfg(test)]
            vanish: false,
        })
    }

    /// Sends on this session's link without the session (for a goodbye from a crash).
    pub fn sender(&self) -> (LinkSender, u8) {
        (self.link.sender(), self.local_index)
    }

    /// The clock the player must use (give it to `set_frame_clock` and a `FrameClockLocale`).
    pub fn clock(&self) -> Arc<Mutex<FrameClock>> {
        self.clock.clone()
    }

    /// The save data the player must use for this session (once).
    pub fn take_storage(&mut self) -> SessionStorage {
        self.storage.take().unwrap_or_default()
    }

    /// The next frame to run.
    #[cfg(test)]
    pub fn frame(&self) -> u32 {
        self.lockstep.frame()
    }

    #[cfg(test)]
    pub fn print_at(&self, frame: u32) -> Option<u64> {
        self.my_prints.get(&frame).copied()
    }

    pub fn key(&mut self, key: PhysicalKey, down: bool) {
        if let Some(text) = &mut self.chat_box {
            if down {
                match key {
                    PhysicalKey::Enter | PhysicalKey::NumpadEnter => {
                        let line: String = text.trim().chars().take(CHAT_MAX).collect();
                        if !line.is_empty() {
                            self.chat_to_send = Some(line);
                        }
                        self.chat_box = None;
                    }
                    PhysicalKey::Escape => self.chat_box = None,
                    PhysicalKey::Backspace => {
                        text.pop();
                    }
                    _ => {}
                }
            }
            return;
        }
        // "/" opens the chat box, unless the key map gives it to the game
        if down && key == PhysicalKey::Slash && !self.game.has_local_code(191) {
            self.chat_box = Some(String::new());
            self.held.clear();
            return;
        }
        if down {
            self.held.insert(key);
        } else {
            self.held.remove(&key);
        }
    }

    /// The host's mouse, in stage twips (see `Player::viewport_to_stage`).
    pub fn mouse(&mut self, x: i32, y: i32, down: Option<bool>) {
        let was_down = self.mouse.is_some_and(|(_, _, down)| down);
        self.mouse = Some((x, y, down.unwrap_or(was_down)));
    }

    /// A character this PC typed (or a text key, see `lockstep::text_control_char`): every PC
    /// types it into the selected text box, on the frame it travels with. Only while a text box is
    /// selected or the game reads `coop.typed`; otherwise the key map's keys type their own
    /// characters (keyPress buttons, see `Stepper::step`), so a stray key can't trigger a game's
    /// shortcut for everyone.
    pub fn typed(&mut self, c: char, text_box: bool) {
        if let Some(text) = &mut self.chat_box {
            // the "/" that opened the box types itself too: leave it out
            if !(text.is_empty() && c == '/') && !c.is_control() && !crate::coop::lockstep::is_text_control(c) && text.chars().count() < CHAT_MAX {
                text.push(c);
            }
            return;
        }
        if self.focused && (text_box || self.stepper.wants_text()) {
            self.typed.push_back(c);
        }
    }

    /// Keys held while the window loses focus would stay pressed; let go of them.
    pub fn focus(&mut self, focused: bool) {
        self.focused = focused;
        if !focused {
            self.held.clear();
            // a window that loses focus lets go of its mouse button too (its release may never come)
            if let Some((x, y, true)) = self.mouse {
                self.mouse = Some((x, y, false));
            }
        }
    }

    fn local_input(&mut self) -> InputFrame {
        if let Some(script) = &self.script {
            return script.input(self.local_sent as usize, self.local_index as usize);
        }
        let mut text = Typed::default();
        while let Some(&c) = self.typed.front()
            && text.push(c)
        {
            self.typed.pop_front();
        }
        InputFrame {
            buttons: if self.focused { self.game.local_buttons(&self.held) } else { 0 },
            // every PC's mouse travels as its slot's mouse; only the host's moves Ruffle's mouse
            mouse: self.mouse,
            text,
        }
    }

    /// Runs every frame that can run now. Call often (every event-loop pass).
    pub fn tick(&mut self, player: &mut Player) -> SessionState {
        if self.state.closed.is_some() {
            return self.state.clone();
        }
        while let Some(event) = self.link.try_recv() {
            match event {
                Event::Msg { from, msg } => {
                    // only the session's players count, and each only for its own slot (the
                    // host passes the other friends' messages on, so a friend trusts the host)
                    let Some(&slot) = self.peers.get(&from) else {
                        continue;
                    };
                    let player_of = |p: u8| !self.hosting || p == slot;
                    match msg {
                        Msg::Input { player: p, frame, input }
                            if p != self.local_index && player_of(p) =>
                        {
                            self.lockstep.push_remote(p, frame, input);
                        }
                        Msg::Print { player: p, frame, hash }
                            if p != self.local_index && player_of(p) =>
                        {
                            self.peer_prints.insert((p, frame), hash);
                            self.compare(frame);
                        }
                        // the host's goodbye ends the game; a friend's lets them go and the rest
                        // play on (the host tells everyone the frame, see `Msg::Left`)
                        Msg::Bye { player: 0, reason } if !self.hosting => {
                            self.close(format!("{} left ({reason})", self.name(0)));
                        }
                        Msg::Bye { player: p, reason } if self.hosting && player_of(p) => {
                            self.let_go(p, format!("{} left ({reason})", self.name(p)));
                        }
                        Msg::Left { player: p, frame } if !self.hosting && p != self.local_index => {
                            self.lockstep.leave(p, frame);
                            self.alive.remove(&p);
                            self.notice = Some((format!("{} left", self.name(p)), Instant::now()));
                        }
                        Msg::Alive { player: p, frame } if p != self.local_index && player_of(p) => {
                            self.alive.insert(p, (Instant::now(), frame));
                        }
                        Msg::Ping { seq } if !self.hosting => self.link.send(&Msg::Pong { seq }),
                        Msg::Pong { seq } if self.hosting => {
                            if let Some((sent, at)) = self.ping_sent
                                && sent == seq
                            {
                                self.round_trips[slot as usize] = Some(at.elapsed().as_millis() as u32);
                            }
                        }
                        Msg::Chat { player: p, frame, text } if p != self.local_index && player_of(p) => {
                            let text: String = text.chars().filter(|c| !c.is_control()).take(CHAT_MAX).collect();
                            self.chat_at.entry(frame).or_default().push((p, text));
                        }
                        Msg::Pings { ms } if !self.hosting => {
                            for (slot, ms) in ms.into_iter().enumerate().take(self.round_trips.len()) {
                                self.round_trips[slot] = ms;
                            }
                        }
                        _ => {}
                    }
                }
                Event::Left { player: conn } => {
                    if let Some(&slot) = self.peers.get(&conn) {
                        let reason = format!("{}'s connection closed {NO_GOODBYE}", self.name(slot));
                        if self.hosting {
                            self.let_go(slot, reason);
                        } else {
                            self.close(reason);
                        }
                    }
                }
                Event::Closed(reason) => self.close(reason),
                _ => {}
            }
        }
        if self.state.closed.is_some() {
            self.stepper.release_all(player);
            return self.state.clone();
        }

        let now = Instant::now();
        if self.hosting && self.ping_sent.is_none_or(|(_, at)| now - at >= PING_EVERY) {
            // the last round's results go out, then the next round starts
            self.round_trips[0] = Some(0);
            self.link.send(&Msg::Pings { ms: self.round_trips.to_vec() });
            let seq = self.ping_sent.map_or(1_000_000, |(seq, _)| seq + 1);
            self.link.send(&Msg::Ping { seq });
            self.ping_sent = Some((seq, now));
        }
        self.stepper.set_pings(self.pings());
        self.timing.tick(now);
        let frame_time = Duration::from_secs_f64(1.0 / player.frame_rate());
        for _ in 0..MAX_CATCH_UP {
            // paced: a frame runs when it is due by this PC's clock; a PC that fell behind
            // (waiting for a late input) runs several in a row to catch up
            let due = *self.next_due.get_or_insert(now);
            if self.paced && now < due {
                break;
            }
            let Some(inputs) = self.lockstep.ready() else {
                self.timing.waiting(now, true);
                break;
            };
            self.timing.waiting(now, false);
            // this PC's input for `delay` frames ahead, read as this frame runs (not before
            // waiting for it, which added the wait to this player's delay)
            while self.local_sent <= self.lockstep.frame() {
                let input = self.local_input();
                let (frame, input) = self.lockstep.push_local(input);
                // a chat line goes just before its frame's input, so every game hears it then
                if let Some(text) = self.chat_to_send.take() {
                    self.link.send(&Msg::Chat { player: self.local_index, frame, text: text.clone() });
                    self.chat_at.entry(frame).or_default().push((self.local_index, text));
                }
                self.taken_at.insert(frame, Instant::now());
                self.link.send(&Msg::Input { player: self.local_index, frame, input });
                self.local_sent += 1;
            }
            let step_started = Instant::now();
            let frame = self.lockstep.frame();
            self.heartbeat.frame.store(frame, Ordering::Relaxed);
            self.heartbeat.step_started.store(self.heartbeat.ms(), Ordering::Relaxed);
            for slot in self.lockstep.leaving_now() {
                self.stepper.add_event(format!("leave {slot}"));
            }
            if let Some(mut lines) = self.chat_at.remove(&frame) {
                lines.sort_by_key(|(slot, _)| *slot);
                for (slot, text) in lines {
                    self.stepper.add_event(format!("chat {slot} {text}"));
                    self.chat_log.push_back((self.name(slot), text, Instant::now()));
                    if self.chat_log.len() > 8 {
                        self.chat_log.pop_front();
                    }
                }
            }
            for (slot, input) in inputs.iter().enumerate().take(self.cursor_moved.len()) {
                if let Some((x, y, _)) = input.mouse
                    && self.cursor_moved[slot].is_none_or(|(at, _)| at != (x, y))
                {
                    self.cursor_moved[slot] = Some(((x, y), Instant::now()));
                }
            }
            self.stepper.step(player, &inputs);
            for (who, whose) in self.stepper.take_busy() {
                if who == self.local_index {
                    let text = format!("{} has that open: wait until they close it", self.name(whose));
                    tracing::info!("Ruffle Together: {text}");
                    self.notice = Some((text, Instant::now()));
                }
            }
            self.heartbeat.step_started.store(0, Ordering::Relaxed);
            let took = step_started.elapsed();
            if took > SLOW_FRAME {
                tracing::warn!(
                    "Ruffle Together: frame {frame} took {:.1} s on this PC (the game's own scripts)",
                    took.as_secs_f64()
                );
            }
            self.timing.step(step_started, took, frame_time);
            // after a long stall, don't sprint through more than the catch-up allows
            self.next_due = Some((due + frame_time).max(now - frame_time * MAX_CATCH_UP));
            let frame = self.lockstep.frame();
            if let Some(taken) = self.taken_at.remove(&frame) {
                self.felt_delay.push(taken.elapsed());
                self.timing.felt(taken.elapsed());
            }
            self.lockstep.advance();
            let stalled = self.last_frame_at.elapsed();
            if stalled > SHOW_WAITING {
                tracing::warn!("Ruffle Together: the game waited {:.1} s for the other players", stalled.as_secs_f64());
            }
            self.last_frame_at = Instant::now();
            if frame % CHECK_EVERY == CHECK_EVERY - 1 {
                let hash = player.coop_fingerprint();
                self.my_prints.insert(frame, hash);
                self.my_states.insert(frame, player.coop_describe());
                self.my_states.retain(|f, _| f + CHECK_EVERY * 8 > frame);
                self.link.send(&Msg::Print { player: self.local_index, frame, hash });
                self.compare(frame);
                if frame % (CHECK_EVERY * 30) == CHECK_EVERY - 1 {
                    tracing::info!("Ruffle Together: frame {frame}, fingerprint {hash:016x}");
                }
            }
        }

        if let Some(line) = self.timing.report(Instant::now(), player.frame_rate()) {
            tracing::info!("{line}");
        }
        let stalled = self.last_frame_at.elapsed();
        self.state.waiting_on = if stalled > SHOW_WAITING {
            self.lockstep.waiting_on().into_iter().map(|p| self.name(p)).collect()
        } else {
            vec![]
        };
        if stalled > EXPLAIN_STALL && !self.stall_explained && !self.state.waiting_on.is_empty() {
            for reason in self.stall_reasons(stalled) {
                tracing::warn!("Ruffle Together: waiting: {reason}");
            }
            self.stall_explained = true;
        } else if stalled < EXPLAIN_STALL {
            self.stall_explained = false;
        }
        if stalled > GIVE_UP && !self.state.waiting_on.is_empty() {
            let reason = self.stall_reasons(stalled).join("; ");
            self.close(format!("stopped waiting: {reason}"));
            self.stepper.release_all(player);
        }
        self.state.clone()
    }

    /// Why each player this PC waits for hasn't sent their input (see `stall_reason`).
    fn stall_reasons(&self, stalled: Duration) -> Vec<String> {
        self.lockstep
            .waiting_on()
            .into_iter()
            .map(|p| {
                let alive = self.alive.get(&p).map(|(at, frame)| (at.elapsed(), *frame));
                stall_reason(&self.name(p), stalled, alive)
            })
            .collect()
    }

    fn name(&self, player: u8) -> String {
        self.names
            .get(player as usize)
            .filter(|n| !n.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("Player {}", player + 1))
    }

    /// Host: a friend left. Everyone plays on without them: they hold nothing from the frame
    /// after their last input the host has (every friend got those inputs through the host, so
    /// every PC can stop them at the same frame), and the host tells everyone that frame.
    fn let_go(&mut self, slot: u8, reason: String) {
        if self.lockstep.is_gone(slot) {
            return;
        }
        let frame = self.lockstep.leave_frame(slot);
        self.lockstep.leave(slot, frame);
        self.link.send(&Msg::Left { player: slot, frame });
        self.alive.remove(&slot);
        self.peers.retain(|_, s| *s != slot);
        tracing::warn!("Ruffle Together: {reason}; the others play on from frame {frame}");
        self.notice = Some((reason, Instant::now()));
    }

    /// For the window: recent chat lines ("Sam: gg") and the open chat box's text, if open.
    pub fn chat_view(&self) -> (Vec<String>, Option<String>) {
        let lines = self
            .chat_log
            .iter()
            .filter(|(.., at)| self.chat_box.is_some() || at.elapsed() < CHAT_SHOWN)
            .map(|(name, text, _)| format!("{name}: {text}"))
            .collect();
        (lines, self.chat_box.clone())
    }

    /// A message for the window while it's fresh (a few seconds): "Sam left".
    pub fn notice(&self) -> Option<String> {
        self.notice
            .as_ref()
            .filter(|(_, at)| at.elapsed() < Duration::from_secs(6))
            .map(|(text, _)| text.clone())
    }

    fn close(&mut self, reason: String) {
        if self.state.closed.is_none() {
            tracing::warn!("Ruffle Together: {reason}");
            self.state.closed = Some(reason);
        }
    }

    /// The first frame where the PCs' fingerprints differ is reported once, on every PC.
    fn compare(&mut self, frame: u32) {
        let Some(&mine) = self.my_prints.get(&frame) else {
            return;
        };
        let differs = self
            .peer_prints
            .iter()
            .any(|(&(_, f), &theirs)| f == frame && theirs != mine);
        if differs && self.state.desync_at.is_none_or(|at| frame < at) {
            self.state.desync_at = Some(frame);
            tracing::error!("Ruffle Together: the games are out of sync at frame {frame}");
            if let Some(gate) = &self.save_gate {
                gate.freeze();
            }
            self.write_desync_report(frame);
        }
    }

    fn write_desync_report(&self, frame: u32) {
        let mut text = format!(
            "Ruffle Together desync at frame {frame}\nthis PC: player {}\n",
            self.local_index + 1
        );
        if let Some(mine) = self.my_prints.get(&frame) {
            text.push_str(&format!("this PC's fingerprint: {mine:016x}\n"));
        }
        for ((p, f), theirs) in &self.peer_prints {
            if *f == frame {
                text.push_str(&format!("{}'s fingerprint: {theirs:016x}\n", self.name(*p)));
            }
        }
        if let Some(state) = self.my_states.get(&frame) {
            text.push_str("\nthis PC's game state:\n");
            text.push_str(state);
        }
        let path = std::env::temp_dir().join(format!(
            "ruffle-together-desync-{}-p{}.txt",
            now_unix_ms(),
            self.local_index + 1
        ));
        match std::fs::write(&path, text) {
            Ok(()) => tracing::error!("Ruffle Together: desync report written to {}", path.display()),
            Err(e) => tracing::warn!("Ruffle Together: can't write the desync report: {e}"),
        }
    }

    /// Not closed yet.
    pub fn is_running(&self) -> bool {
        self.state.closed.is_none()
    }

    /// Whether the window should put a picture on screen now (see `PictureGate`).
    pub fn allow_picture(&mut self) -> bool {
        if !self.is_running() {
            return true;
        }
        let allowed = self.picture.allow(self.lockstep.frame(), Instant::now());
        if allowed {
            self.timing.shown += 1;
        }
        allowed
    }

    /// How long this PC's inputs took to reach the game (input taken -> its frame runs).
    pub fn felt_delay(&self) -> &[Duration] {
        &self.felt_delay
    }

    /// The next frame to run (for logs).
    /// This PC's round trip to each player, in ms (`coop.ping`): to the host it's this PC's own;
    /// to another friend it goes through the host, so both trips add up; to itself it's 0.
    pub fn pings(&self) -> [Option<u32>; crate::coop::lockstep::MAX_PLAYERS] {
        let me = self.local_index as usize;
        let mut pings = [None; crate::coop::lockstep::MAX_PLAYERS];
        for (slot, ping) in pings.iter_mut().enumerate().take(self.names.len()) {
            *ping = if slot == me {
                Some(0)
            } else if self.hosting {
                self.round_trips[slot]
            } else if slot == 0 {
                self.round_trips[me]
            } else {
                self.round_trips[me].zip(self.round_trips[slot]).map(|(a, b)| a + b)
            };
        }
        pings
    }

    /// The other players' cursors for the window to draw, as the `cursors` rule says, each with
    /// how visible it is: one that hasn't moved for a while fades out (the picture only).
    pub fn friend_cursors(&self, player: &mut ruffle_core::Player) -> Vec<FriendCursor> {
        const SHOWN: Duration = Duration::from_secs(3);
        const FADE: Duration = Duration::from_millis(500);
        self.stepper
            .friend_cursors(player)
            .into_iter()
            .filter(|&(slot, ..)| !self.lockstep.is_gone(slot))
            .filter_map(|(slot, x, y, down)| {
                let idle = self.cursor_moved[slot as usize].map_or(Duration::ZERO, |(_, at)| at.elapsed());
                let alpha = if down || idle < SHOWN {
                    1.0
                } else {
                    1.0 - (idle - SHOWN).as_secs_f32() / FADE.as_secs_f32()
                };
                (alpha > 0.0).then(|| FriendCursor { name: self.name(slot), slot, x, y, down, alpha })
            })
            .collect()
    }

    /// Every player's mouse for the mouse debug overlay (`--coop-mouse-debug`), with their name.
    pub fn mouse_debug(&self, player: &mut ruffle_core::Player) -> Vec<(String, crate::coop::mice::MouseDebug)> {
        self.stepper.mouse_debug(player).into_iter().map(|mouse| (self.name(mouse.slot), mouse)).collect()
    }

    /// The name tags the game asked for (`coop.label`), for the window to draw.
    pub fn labels(&self) -> Vec<crate::coop::api::Label> {
        self.stepper.labels()
    }

    pub fn frames_run(&self) -> u32 {
        self.lockstep.frame()
    }

    /// What Discord shows: the game, the room (or direct), how many play, and a Join button for
    /// a relay room when the launcher said which game this is.
    pub fn presence(&self) -> crate::coop::discord::Presence {
        let playing = (0..self.names.len() as u8).filter(|&p| !self.lockstep.is_gone(p)).count() as u8;
        let where_ = self.room.as_ref().map_or("direct game".to_string(), |code| format!("room {code}"));
        let join = self
            .room
            .as_ref()
            .zip(self.link_id.as_ref())
            .and_then(|(code, id)| crate::coop::discord::join_link(id, code));
        crate::coop::discord::Presence {
            details: format!("Playing {}", self.swf_name),
            state: where_,
            party: Some((playing, self.names.len() as u8)),
            join,
        }
    }

    /// What the window title shows.
    pub fn title(&self) -> String {
        let mut title = String::from("Ruffle Together");
        if let Some(code) = &self.room {
            title.push_str(&format!(" - room {code}"));
        }
        title.push_str(&format!(" - player {}", self.local_index + 1));
        if let Some(reason) = &self.state.closed {
            title.push_str(&format!(" - {reason}"));
        } else if self.state.desync_at.is_some() {
            title.push_str(" - THE TWO GAMES ARE OUT OF SYNC: restart the level");
        } else if !self.state.waiting_on.is_empty() {
            title.push_str(&format!(" - waiting for {}...", self.state.waiting_on.join(", ")));
        }
        title
    }
}

/// `--coop-headless`: plays `frames` frames without a window and prints one result line.
pub fn run_headless(mut session: Session, swf: &Path, frames: u32) -> Result<()> {
    let player =
        crate::coop::replay::headless_player(swf, Some(session.clock()), session.take_storage())?;
    let mut player = player.lock().map_err(|_| anyhow!("player lock"))?;
    let started = Instant::now();
    let state = loop {
        let state = session.tick(&mut player);
        if session.frames_run() >= frames || state.closed.is_some() {
            break state;
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    let mut delays: Vec<f64> = session.felt_delay().iter().map(|d| d.as_secs_f64() * 1000.0).collect();
    delays.sort_by(f64::total_cmp);
    let at = |q: f64| delays.get(((delays.len() as f64 - 1.0) * q) as usize).copied().unwrap_or(-1.0);
    println!(
        "COOP RESULT player={} frames={} seconds={:.1} desync_at={} closed={} \
         felt_delay_ms_median={:.0} p95={:.0} max={:.0}",
        session.local_index + 1,
        session.frames_run(),
        started.elapsed().as_secs_f64(),
        state.desync_at.map_or("none".into(), |f| f.to_string()),
        state.closed.as_deref().unwrap_or("no"),
        at(0.5),
        at(0.95),
        at(1.0),
    );
    Ok(())
}

impl Drop for Session {
    fn drop(&mut self) {
        #[cfg(test)]
        if self.vanish {
            return;
        }
        self.link.send(&Msg::Bye {
            player: self.local_index,
            reason: "closed the game".into(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coop::lockstep::MAX_PLAYERS;
    use crate::coop::replay::headless_player;
    use ruffle_core::external::Value;
    use std::net::TcpListener;
    use ruffle_core::backend::storage::StorageBackend;
    use std::fs;
    use std::thread;

    fn free_port() -> u16 {
        const RESERVED: [u16; 8] = [6420, 7777, 7778, 7779, 27015, 27016, 27020, 27021];
        loop {
            let port = TcpListener::bind("127.0.0.1:0")
                .and_then(|l| l.local_addr())
                .map(|a| a.port())
                .expect("free port");
            if !RESERVED.contains(&port) {
                return port;
            }
        }
    }

    fn test_swf() -> Option<PathBuf> {
        match std::env::var("COOP_TEST_SWF") {
            Ok(path) => Some(PathBuf::from(path)),
            Err(_) => {
                eprintln!("skipping: COOP_TEST_SWF not set");
                None
            }
        }
    }

    fn script() -> Script {
        Script::parse(include_str!("../../tests/coop/bic-two-players.txt")).expect("script")
    }

    fn opts(role: Role, swf: &Path, start_skew_ms: i64) -> SessionOpts {
        SessionOpts {
            role,
            name: "tester".into(),
            players: 2,
            delay: Some(3),
            swf: swf.to_path_buf(),
            game: Game::built_in("bad-ice-cream").expect("built in"),
            save_dir: None,
            save_start: SaveStart::Coop,
            options: vec![],
            link_id: None,
            script: Some(script()),
            host_wait: Some(Duration::from_secs(20)),
            start_skew_ms,
            paced: false,
            settings: "frame_rate=None; player_version=None".into(),
            lobby: None,
        }
    }

    /// Runs one PC: starts the session, then plays until `frames` (or until `stop_at`, when it
    /// drops the session to leave). Returns the session's final state and its fingerprints.
    fn play(opts: SessionOpts, frames: u32, stop_at: Option<u32>) -> (SessionState, Session) {
        let swf = opts.swf.clone();
        let mut session = Session::start(opts).expect("session");
        let player = headless_player(&swf, Some(session.clock()), session.take_storage())
            .expect("player");
        let mut player = player.lock().expect("player");
        let end = Instant::now() + Duration::from_secs(120);
        loop {
            let state = session.tick(&mut player);
            if session.frame() >= frames || state.closed.is_some() || Instant::now() > end {
                return (state, session);
            }
            if stop_at.is_some_and(|stop| session.frame() >= stop) {
                return (state, session);
            }
            thread::sleep(Duration::from_millis(1));
        }
    }

    /// Input events ask for repaints too, and each repaint waits for the screen; only a new frame
    /// (or a picture gone stale, e.g. after a resize while the game waits) is shown.
    #[test]
    fn a_picture_is_shown_once_per_frame() {
        let mut gate = PictureGate::default();
        let t = Instant::now();
        assert!(gate.allow(1, t), "a new frame is shown");
        assert!(!gate.allow(1, t + Duration::from_millis(5)), "a repaint from input is not");
        assert!(!gate.allow(1, t + Duration::from_millis(99)));
        assert!(gate.allow(2, t + Duration::from_millis(100)), "the next frame is");
        assert!(gate.allow(2, t + Duration::from_millis(200)), "a stale picture is redrawn");
    }

    #[test]
    fn build_id_names_this_release() {
        assert!(build_id().starts_with("rc-1.0.4#"), "{}", build_id());
        // the relay keeps 32 characters of a version
        assert!(build_id().len() <= 32);
    }

    #[test]
    fn two_sessions_stay_in_sync() {
        let Some(swf) = test_swf() else { return };
        let port = free_port();
        let host_swf = swf.clone();
        let host = thread::spawn(move || play(opts(Role::HostDirect { port }, &host_swf, 0), 1000, None));
        thread::sleep(Duration::from_millis(300));
        let join = thread::spawn(move || {
            let addr = format!("127.0.0.1:{port}");
            play(opts(Role::JoinDirect { addr }, &swf, 0), 1000, None)
        });
        let (host_state, host_session) = host.join().expect("host");
        let (join_state, join_session) = join.join().expect("join");
        assert_eq!(host_state.desync_at, None);
        assert_eq!(join_state.desync_at, None);
        assert!(
            host_session.frame() >= 1000 && join_session.frame() >= 1000,
            "host at {} {:?}, friend at {} {:?}",
            host_session.frame(),
            host_state,
            join_session.frame(),
            join_state
        );
        // the same game on both PCs, checked every 25 frames (frames 24, 49, ... 974 are check frames)
        assert_eq!(host_session.print_at(974), join_session.print_at(974));
        assert!(host_session.print_at(974).is_some());
    }

    /// The host measures every friend's round trip and tells everyone: both PCs know the ping
    /// between them (`coop.ping`) after a couple of rounds.
    #[test]
    fn both_pcs_learn_their_ping() {
        let Some(swf) = test_swf() else { return };
        let port = free_port();
        let host_swf = swf.clone();
        let run = |opts: SessionOpts| {
            let swf = opts.swf.clone();
            let mut session = Session::start(opts).expect("session");
            let player = headless_player(&swf, Some(session.clock()), session.take_storage()).expect("player");
            let mut player = player.lock().expect("player");
            let end = Instant::now() + Duration::from_secs(6);
            while Instant::now() < end && session.state.closed.is_none() {
                session.tick(&mut player);
                thread::sleep(Duration::from_millis(5));
            }
            session
        };
        let host = thread::spawn(move || {
            let mut opts = opts(Role::HostDirect { port }, &host_swf, 0);
            opts.paced = true;
            run(opts)
        });
        thread::sleep(Duration::from_millis(300));
        let join = thread::spawn(move || {
            let mut opts = opts(Role::JoinDirect { addr: format!("127.0.0.1:{port}") }, &swf, 0);
            opts.paced = true;
            run(opts)
        });
        let (host, join) = (host.join().expect("host"), join.join().expect("join"));
        assert_eq!(host.pings()[0], Some(0), "to itself");
        let host_to_friend = host.pings()[1].expect("the host measured the friend");
        let friend_to_host = join.pings()[0].expect("the friend heard its round trip");
        assert!(host_to_friend < 500 && friend_to_host < 500, "{host_to_friend} {friend_to_host} ms on one PC");
        assert_eq!(join.pings()[1], Some(0));
        assert_eq!(join.pings()[2], None, "only 2 players");
    }

    /// The chat box: "/" opens it (keys stop reaching the game), typing fills it, Enter sends the
    /// line, and every PC's game hears it as an event; both windows show it.
    #[test]
    fn a_chat_line_reaches_every_game() {
        let Some(swf) = test_swf() else { return };
        let port = free_port();
        let host_swf = swf.clone();
        let run = |opts: SessionOpts, chat: bool| {
            let swf = opts.swf.clone();
            let mut session = Session::start(opts).expect("session");
            let player = headless_player(&swf, Some(session.clock()), session.take_storage()).expect("player");
            let mut player = player.lock().expect("player");
            let end = Instant::now() + Duration::from_secs(60);
            let mut said = false;
            while Instant::now() < end && session.state.closed.is_none() && session.frame() < 300 {
                if chat && !said && session.frame() >= 50 {
                    session.key(PhysicalKey::Slash, true);
                    session.typed('/', false);
                    assert_eq!(session.chat_view().1.as_deref(), Some(""), "open, without the /");
                    for c in "hi therex".chars() {
                        session.typed(c, false);
                    }
                    session.key(PhysicalKey::Backspace, true);
                    session.key(PhysicalKey::Enter, true);
                    assert_eq!(session.chat_view().1, None, "Enter closes it");
                    said = true;
                }
                session.tick(&mut player);
                thread::sleep(Duration::from_millis(1));
            }
            session
        };
        let host = thread::spawn(move || run(opts(Role::HostDirect { port }, &host_swf, 0), true));
        thread::sleep(Duration::from_millis(300));
        let join = thread::spawn(move || run(opts(Role::JoinDirect { addr: format!("127.0.0.1:{port}") }, &swf, 0), false));
        let (host, join) = (host.join().expect("host"), join.join().expect("join"));
        for session in [&host, &join] {
            let events = session.stepper.api().lock().expect("api").events.clone();
            assert!(events.contains(&"chat 0 hi there".to_string()), "{events:?}");
            assert_eq!(session.chat_view().0, vec!["tester: hi there".to_string()]);
        }
        assert_eq!(host.state.desync_at, None);
        assert_eq!(join.state.desync_at, None);
    }

    /// The host's `--coop-option`s reach every player's `coop.option`; a friend's own are ignored.
    #[test]
    fn every_player_gets_the_hosts_options() {
        let Some(swf) = test_swf() else { return };
        let port = free_port();
        let host_swf = swf.clone();
        let host = thread::spawn(move || {
            let mut opts = opts(Role::HostDirect { port }, &host_swf, 0);
            opts.options = vec![("map".into(), "3".into()), ("mode".into(), "team".into())];
            play(opts, 30, None)
        });
        thread::sleep(Duration::from_millis(300));
        let join = thread::spawn(move || {
            let mut opts = opts(Role::JoinDirect { addr: format!("127.0.0.1:{port}") }, &swf, 0);
            opts.options = vec![("map".into(), "9".into())];
            play(opts, 30, None)
        });
        let (_, host_session) = host.join().expect("host");
        let (_, join_session) = join.join().expect("join");
        for session in [&host_session, &join_session] {
            let mut api = session.stepper.api().lock().expect("api");
            assert_eq!(api.answer("coop.option", &[Value::String("map".into())]), Value::String("3".into()));
            assert_eq!(api.answer("coop.option", &[Value::String("mode".into())]), Value::String("team".into()));
        }
    }

    /// Measured in the lab (tools/coop-lab/lab.py, relay with lag): a delay of d frames runs
    /// clean while the ping stays under 2d frames minus ~15 ms of jitter room.
    /// Every PC keeps the session's saves in its own co-op save, the same bytes on each (each PC
    /// keeps the game in a different folder), and the solo save is left alone.
    #[test]
    fn every_pc_keeps_the_same_coop_save() {
        let Some(swf) = test_swf() else { return };
        let port = free_port();
        let name = swf.file_name().expect("file name").to_string_lossy().to_string();
        let dirs = [tempfile::tempdir().expect("temp dir"), tempfile::tempdir().expect("temp dir")];
        let run = |mut o: SessionOpts, dir: PathBuf, folder: &'static str| {
            o.save_dir = Some(dir);
            let name = name.clone();
            move || {
                let swf = o.swf.clone();
                let mut session = Session::start(o).expect("session");
                let player = headless_player(&swf, Some(session.clock()), session.take_storage()).expect("player");
                let mut player = player.lock().expect("player");
                while session.frame() < 300 && session.tick(&mut player).closed.is_none() {
                    thread::sleep(Duration::from_millis(1));
                }
                // what SharedObject.flush does, from where this PC keeps the game
                player.storage_mut().put(&format!("localhost/{folder}/{name}/so_progress"), &[1, 2, 3]);
                session.frame()
            }
        };
        let host = thread::spawn(run(
            opts(Role::HostDirect { port }, &swf, 0),
            dirs[0].path().to_path_buf(),
            "Users/host/games",
        ));
        thread::sleep(Duration::from_millis(300));
        let join = thread::spawn(run(
            opts(Role::JoinDirect { addr: format!("127.0.0.1:{port}") }, &swf, 0),
            dirs[1].path().to_path_buf(),
            "Downloads",
        ));
        assert!(host.join().expect("host") >= 300 && join.join().expect("join") >= 300, "both played");
        let saves: Vec<Vec<u8>> = dirs
            .iter()
            .map(|dir| fs::read(coop_save_file(dir.path(), &name)).expect("a co-op save on every PC"))
            .collect();
        assert_eq!(saves[0], saves[1], "the same save on both PCs");
        let next = SessionStorage::from_bytes(&SessionStorage::export_for_session(dirs[1].path(), &name, SaveStart::Coop))
            .expect("valid save");
        assert_eq!(next.get(&format!("localhost/elsewhere/{name}/so_progress")), Some(vec![1, 2, 3]), "a friend can host next");
        for dir in &dirs {
            let entries: Vec<_> = fs::read_dir(dir.path()).expect("read dir").flatten().map(|e| e.file_name()).collect();
            assert_eq!(entries, vec![std::ffi::OsString::from("coop")], "no solo save was written");
        }
    }

    #[test]
    fn delay_is_picked_from_the_ping() {
        let frame = Duration::from_secs_f64(1.0 / 30.0);
        let ms = Duration::from_millis;
        assert_eq!(auto_delay(ms(1), frame), 1, "same PC or LAN");
        assert_eq!(auto_delay(ms(10), frame), 1, "Radmin");
        assert_eq!(auto_delay(ms(50), frame), 1);
        assert_eq!(auto_delay(ms(60), frame), 2, "delay 1 stalled at 60-70 ms");
        assert_eq!(auto_delay(ms(100), frame), 2, "delay 2 ran clean at 100-110 ms");
        assert_eq!(auto_delay(ms(155), frame), 3, "the central server from here");
        assert_eq!(auto_delay(ms(5000), frame), 10, "at most 10 frames");
    }

    /// Without a set delay the host pings the friend; on one PC that picks 1 frame for both.
    #[test]
    fn host_picks_the_delay_from_the_ping() {
        let Some(swf) = test_swf() else { return };
        let port = free_port();
        let host_swf = swf.clone();
        let auto = |mut o: SessionOpts| {
            o.paced = true;
            o.delay = None;
            o
        };
        let host = thread::spawn(move || play(auto(opts(Role::HostDirect { port }, &host_swf, 0)), 90, None));
        thread::sleep(Duration::from_millis(300));
        let addr = format!("127.0.0.1:{port}");
        let (_, friend) = play(auto(opts(Role::JoinDirect { addr }, &swf, 0)), 90, None);
        let (host_state, host) = host.join().expect("host");
        assert_eq!(host_state.desync_at, None);
        let frame_ms = 1000.0 / 30.0;
        for (who, session) in [("host", &host), ("friend", &friend)] {
            let mut felt: Vec<f64> =
                session.felt_delay()[30..].iter().map(|d| d.as_secs_f64() * 1000.0).collect();
            felt.sort_by(f64::total_cmp);
            let median = felt[felt.len() / 2];
            assert!(median < frame_ms + 12.0, "{who} felt {median:.0} ms, expected 1 frame");
        }
    }

    /// The PC that started first waits for the other's inputs; reading its own keys only while
    /// it waited added that wait to its delay (host 94 ms at 2 frames). Both feel the setting.
    #[test]
    fn both_players_feel_the_input_delay_setting() {
        let Some(swf) = test_swf() else { return };
        let port = free_port();
        let host_swf = swf.clone();
        let two_frames = |mut o: SessionOpts| {
            o.paced = true;
            o.delay = Some(2);
            o
        };
        let host = thread::spawn(move || {
            play(two_frames(opts(Role::HostDirect { port }, &host_swf, 0)), 150, None)
        });
        thread::sleep(Duration::from_millis(300));
        let join = thread::spawn(move || {
            let addr = format!("127.0.0.1:{port}");
            // the friend's game starts later (in real use: a slower load), so the host is ahead
            let opts = two_frames(opts(Role::JoinDirect { addr }, &swf, 0));
            let mut session = Session::start(opts).expect("session");
            let player = headless_player(&swf, Some(session.clock()), session.take_storage())
                .expect("player");
            let mut player = player.lock().expect("player");
            thread::sleep(Duration::from_millis(200));
            while session.frame() < 150 && session.tick(&mut player).closed.is_none() {
                thread::sleep(Duration::from_millis(1));
            }
            session
        });
        let (_, host) = host.join().expect("host");
        let friend = join.join().expect("join");
        let frame_ms = 1000.0 / 30.0;
        for (who, session) in [("host", &host), ("friend", &friend)] {
            // after start-up
            let mut felt: Vec<f64> =
                session.felt_delay()[30..].iter().map(|d| d.as_secs_f64() * 1000.0).collect();
            felt.sort_by(f64::total_cmp);
            let median = felt[felt.len() / 2];
            assert!(median < 2.0 * frame_ms + 12.0, "{who} felt {median:.0} ms at 2 frames");
        }
    }

    #[test]
    fn paced_sessions_run_at_the_games_speed() {
        let Some(swf) = test_swf() else { return };
        let port = free_port();
        let host_swf = swf.clone();
        let paced = |mut o: SessionOpts| {
            o.paced = true;
            o
        };
        let started = Instant::now();
        let host = thread::spawn(move || {
            play(paced(opts(Role::HostDirect { port }, &host_swf, 0)), 100, None)
        });
        thread::sleep(Duration::from_millis(300));
        let join = thread::spawn(move || {
            let addr = format!("127.0.0.1:{port}");
            play(paced(opts(Role::JoinDirect { addr }, &swf, 0)), 100, None)
        });
        let (host_state, _) = host.join().expect("host");
        join.join().expect("join");
        assert_eq!(host_state.desync_at, None);
        // Bad Ice-Cream runs at 25 fps: 100 frames take 4 s (plus the handshake)
        let took = started.elapsed();
        assert!(
            took >= Duration::from_millis(3800) && took < Duration::from_secs(7),
            "100 frames took {took:?}"
        );
    }

    #[test]
    fn desync_is_reported_on_both_pcs_at_the_same_frame() {
        let Some(swf) = test_swf() else { return };
        let port = free_port();
        let host_swf = swf.clone();
        let host = thread::spawn(move || play(opts(Role::HostDirect { port }, &host_swf, 0), 1000, None));
        thread::sleep(Duration::from_millis(300));
        // the friend's clock is 1 ms off: its Math.random seed differs, as if nothing were synced
        let join = thread::spawn(move || {
            let addr = format!("127.0.0.1:{port}");
            play(opts(Role::JoinDirect { addr }, &swf, 1), 1000, None)
        });
        let (host_state, _) = host.join().expect("host");
        let (join_state, _) = join.join().expect("join");
        assert!(host_state.desync_at.is_some(), "host saw no desync");
        assert_eq!(host_state.desync_at, join_state.desync_at);
    }

    #[test]
    fn friend_leaving_lets_the_host_play_on() {
        let Some(swf) = test_swf() else { return };
        let port = free_port();
        let host_swf = swf.clone();
        let host = thread::spawn(move || play(opts(Role::HostDirect { port }, &host_swf, 0), 5000, None));
        thread::sleep(Duration::from_millis(300));
        let join = thread::spawn(move || {
            let addr = format!("127.0.0.1:{port}");
            play(opts(Role::JoinDirect { addr }, &swf, 0), 5000, Some(300))
        });
        let (_, join_session) = join.join().expect("join");
        let left_at = Instant::now();
        drop(join_session);
        let (host_state, host_session) = host.join().expect("host");
        assert_eq!(host_state.closed, None, "the host plays on");
        assert!(host_session.frame() >= 5000, "at {}", host_session.frame());
        assert_eq!(host_session.notice.as_ref().map(|(n, _)| n.as_str()), Some("tester left (closed the game)"));
        assert!(left_at.elapsed() < Duration::from_secs(60));
        // the game heard it: everyone joined, then the friend left
        let events = host_session.stepper.api().lock().expect("api").events.clone();
        assert_eq!(events.front().map(String::as_str), Some("join 0"));
        assert!(events.contains(&"leave 1".to_string()), "{events:?}");
    }

    /// The three ways a player stops, worded so a log tells them apart.
    #[test]
    fn a_stall_says_whether_the_game_froze_or_everything_stopped() {
        let s = Duration::from_secs;
        let frozen = stall_reason("Nick", s(7), Some((Duration::from_millis(400), 3024)));
        assert!(frozen.contains("Nick's Ruffle is still running"), "{frozen}");
        assert!(frozen.contains("frame 3024 for 7 s"), "{frozen}");
        let gone = stall_reason("Nick", s(9), Some((s(8), 3024)));
        assert!(gone.contains("nothing at all from Nick for 8 s"), "{gone}");
        let never = stall_reason("Nick", s(9), None);
        assert!(never.contains("nothing at all from Nick for 9 s"), "{never}");
    }

    /// A friend whose game is stuck in a frame still sends its heartbeat: the host can tell
    /// that apart from a friend that is gone.
    #[test]
    fn a_frozen_friend_is_told_apart_by_its_heartbeat() {
        let Some(swf) = test_swf() else { return };
        let port = free_port();
        let friend_swf = swf.clone();
        let friend = thread::spawn(move || {
            thread::sleep(Duration::from_millis(300));
            let addr = format!("127.0.0.1:{port}");
            let (_, session) = play(opts(Role::JoinDirect { addr }, &friend_swf, 0), 5000, Some(100));
            // the game "freezes": no more frames, but Ruffle (and its heartbeat) runs on
            thread::sleep(Duration::from_secs(9));
            drop(session);
        });
        let mut host = Session::start(opts(Role::HostDirect { port }, &swf, 0)).expect("session");
        let player = headless_player(&swf, Some(host.clock()), host.take_storage()).expect("player");
        let mut player = player.lock().expect("player");
        let end = Instant::now() + Duration::from_secs(60);
        let mut reasons = vec![];
        while Instant::now() < end && host.tick(&mut player).closed.is_none() && host.notice.is_none() {
            let stalled = host.last_frame_at.elapsed();
            if stalled > EXPLAIN_STALL && reasons.is_empty() {
                reasons = host.stall_reasons(stalled);
            }
            thread::sleep(Duration::from_millis(5));
        }
        friend.join().expect("friend");
        assert_eq!(reasons.len(), 1, "{reasons:?}");
        assert!(reasons[0].contains("tester's Ruffle is still running"), "{reasons:?}");
        // when the frozen friend finally quits, the host plays on without them
        assert_eq!(host.state.closed, None);
        assert_eq!(host.notice.as_ref().map(|(n, _)| n.as_str()), Some("tester left (closed the game)"));
    }

    /// A Ruffle that is killed sends no goodbye: the others say the connection closed without one.
    #[test]
    fn a_friend_that_vanishes_is_reported_as_such() {
        let Some(swf) = test_swf() else { return };
        let port = free_port();
        let host_swf = swf.clone();
        let host = thread::spawn(move || play(opts(Role::HostDirect { port }, &host_swf, 0), 5000, None));
        thread::sleep(Duration::from_millis(300));
        let addr = format!("127.0.0.1:{port}");
        let (_, mut friend) = play(opts(Role::JoinDirect { addr }, &swf, 0), 5000, Some(100));
        friend.vanish = true;
        drop(friend);
        let (host_state, host_session) = host.join().expect("host");
        assert_eq!(host_state.closed, None, "the host plays on");
        let notice = host_session.notice.as_ref().map(|(n, _)| n.clone()).unwrap_or_default();
        assert!(notice.contains("tester's connection closed without a goodbye"), "{notice}");
    }

    /// Every PC sends its own mouse as its slot's mouse, and every PC sees all of them the same.
    #[test]
    fn every_player_sends_their_own_mouse() {
        let Some(swf) = test_swf() else { return };
        let port = free_port();
        let three = |mut o: SessionOpts| {
            o.players = 3;
            o.game = Game::built_in("gun-mayhem-2").expect("built in");
            o.script = Some(
                Script::parse("move 100 50 0\nmove 200 60 1\nmove 300 70 2\nwait 30\nclick 310 71 2\nwait 30")
                    .expect("script"),
            );
            o
        };
        let host_swf = swf.clone();
        let host = thread::spawn(move || play(three(opts(Role::HostDirect { port }, &host_swf, 0)), 120, None));
        thread::sleep(Duration::from_millis(300));
        let friends: Vec<_> = (0..2)
            .map(|_| {
                let swf = swf.clone();
                thread::sleep(Duration::from_millis(100));
                thread::spawn(move || {
                    let addr = format!("127.0.0.1:{port}");
                    play(three(opts(Role::JoinDirect { addr }, &swf, 0)), 120, None)
                })
            })
            .collect();
        let mut all = vec![host.join().expect("host")];
        all.extend(friends.into_iter().map(|f| f.join().expect("friend")));
        let expected = [(2000, 1000, false), (4000, 1200, false), (6200, 1420, false), (0, 0, false)];
        for (state, session) in &all {
            assert_eq!(state.desync_at, None);
            let api = session.stepper.api().lock().expect("api").clone();
            assert_eq!(api.mouse[..4], expected, "player {}", session.local_index + 1);
            assert_eq!(api.local_slot, session.local_index);
        }
    }

    /// A connection that stalls for a while (an internet or relay hiccup) but stays open must not
    /// end the game: both PCs wait, then carry on (a GM2 game over the relay ended on a 5 s stall).
    #[test]
    fn a_long_stall_is_waited_out() {
        let Some(swf) = test_swf() else { return };
        let port = free_port();
        let host_swf = swf.clone();
        let host = thread::spawn(move || play(opts(Role::HostDirect { port }, &host_swf, 0), 400, None));
        thread::sleep(Duration::from_millis(300));
        let addr = format!("127.0.0.1:{port}");
        let mut friend = Session::start(opts(Role::JoinDirect { addr }, &swf, 0)).expect("session");
        let player = headless_player(&swf, Some(friend.clock()), friend.take_storage()).expect("player");
        let mut player = player.lock().expect("player");
        let mut state = SessionState::default();
        let mut stalled = false;
        while friend.frame() < 400 && state.closed.is_none() {
            if friend.frame() >= 100 && !stalled {
                // nothing sent or read for 8 s, the connection stays open
                thread::sleep(Duration::from_secs(8));
                stalled = true;
            }
            state = friend.tick(&mut player);
            thread::sleep(Duration::from_millis(1));
        }
        let (host_state, _) = host.join().expect("host");
        assert_eq!(host_state.closed, None, "the host gave up");
        assert_eq!(state.closed, None, "the friend gave up");
        assert_eq!(host_state.desync_at, None);
        assert!(friend.frame() >= 400, "the friend carried on");
    }

    /// A friend's keys reach another friend through the host: that trip is the two friends' pings
    /// added (3 players at ~90 ms ping ran 26 s for 20 s of game on the single worst ping).
    #[test]
    fn friend_to_friend_trips_count_both_pings() {
        let ms = Duration::from_millis;
        assert_eq!(path_ping(&[ms(40)]), ms(40), "one friend: its own ping");
        assert_eq!(path_ping(&[ms(40), ms(90), ms(60)]), ms(150), "the two worst, added");
    }

    /// Four PCs (host + 3 friends): the host waits for all three, each gets its own slot, and
    /// every game runs the same.
    #[test]
    fn three_players_play_on_in_sync_after_one_leaves() {
        let Some(swf) = test_swf() else { return };
        let port = free_port();
        let three = |mut o: SessionOpts| {
            o.players = 3;
            o.game = Game::built_in("gun-mayhem-2").expect("built in");
            o
        };
        let host_swf = swf.clone();
        let host = thread::spawn(move || play(three(opts(Role::HostDirect { port }, &host_swf, 0)), 1500, None));
        thread::sleep(Duration::from_millis(300));
        // the first friend quits at frame 300; the second plays to the end with the host
        let friends: Vec<_> = [Some(300), None]
            .into_iter()
            .enumerate()
            .map(|(i, stop_at)| {
                let swf = swf.clone();
                thread::sleep(Duration::from_millis(100 * i as u64));
                thread::spawn(move || {
                    let addr = format!("127.0.0.1:{port}");
                    let (state, session) = play(three(opts(Role::JoinDirect { addr }, &swf, 0)), 1500, stop_at);
                    let prints = session.my_prints.clone();
                    let notice = session.notice();
                    (state, session.local_index, session.frame(), prints, notice)
                })
            })
            .collect();
        let results: Vec<_> = friends.into_iter().map(|f| f.join().expect("friend")).collect();
        let (host_state, host_session) = host.join().expect("host");
        assert_eq!(host_state.closed, None);
        assert_eq!(host_state.desync_at, None);
        let stayer = results.iter().find(|(_, _, frame, ..)| *frame >= 1500).expect("one friend played on");
        let (state, slot, _, prints, notice) = stayer;
        assert_eq!(state.desync_at, None, "player {}", slot + 1);
        assert_eq!(state.closed, None);
        assert_eq!(notice.as_deref(), Some("tester left"), "the friend who stayed was told");
        // the same game on both PCs before and after the leave
        let host_prints = &host_session.my_prints;
        assert!(host_prints.keys().any(|&f| f > 1000), "checks ran after the leave");
        for (frame, print) in prints {
            if let Some(host_print) = host_prints.get(frame) {
                assert_eq!(print, host_print, "frame {frame}");
            }
        }
    }

    #[test]
    fn four_players_stay_in_sync() {
        let Some(swf) = test_swf() else { return };
        let port = free_port();
        let four = |mut o: SessionOpts| {
            o.players = 4;
            o.game = Game::built_in("gun-mayhem-2").expect("built in");
            o
        };
        let host_swf = swf.clone();
        let host = thread::spawn(move || play(four(opts(Role::HostDirect { port }, &host_swf, 0)), 300, None));
        thread::sleep(Duration::from_millis(300));
        let friends: Vec<_> = (0..3)
            .map(|i| {
                let swf = swf.clone();
                thread::sleep(Duration::from_millis(100 * i));
                thread::spawn(move || {
                    let addr = format!("127.0.0.1:{port}");
                    play(four(opts(Role::JoinDirect { addr }, &swf, 0)), 300, None)
                })
            })
            .collect();
        let mut all = vec![host.join().expect("host")];
        all.extend(friends.into_iter().map(|f| f.join().expect("friend")));
        let mut slots: Vec<u8> = all.iter().map(|(_, s)| s.local_index).collect();
        slots.sort();
        assert_eq!(slots, vec![0, 1, 2, 3], "each PC its own player slot");
        let host_prints = &all[0].1.my_prints;
        assert!(host_prints.len() >= 10, "sync checks ran");
        for (state, session) in &all {
            assert_eq!(state.desync_at, None);
            assert_eq!(state.closed, None);
            assert_eq!(&session.my_prints, host_prints, "player {}", session.local_index + 1);
        }
    }

    /// The most players Ruffle Together takes: 8 PCs, each its own slot, the same frames.
    #[test]
    fn eight_players_stay_in_sync() {
        let Some(swf) = test_swf() else { return };
        let port = free_port();
        let keys: String = (0..MAX_PLAYERS).map(|slot| format!("slot {slot}: FIRE={}
", 65 + slot)).collect();
        let game = Game::parse("eight", &keys).expect("8 slots");
        assert_eq!(game.max_players() as usize, MAX_PLAYERS);
        let eight = move |mut o: SessionOpts| {
            o.players = MAX_PLAYERS as u8;
            o.game = game.clone();
            o
        };
        let host_swf = swf.clone();
        let host_opts = eight(opts(Role::HostDirect { port }, &host_swf, 0));
        let host = thread::spawn(move || play(host_opts, 300, None));
        thread::sleep(Duration::from_millis(300));
        let friends: Vec<_> = (1..MAX_PLAYERS)
            .map(|_| {
                thread::sleep(Duration::from_millis(100));
                let o = eight(opts(Role::JoinDirect { addr: format!("127.0.0.1:{port}") }, &swf, 0));
                thread::spawn(move || play(o, 300, None))
            })
            .collect();
        let mut all = vec![host.join().expect("host")];
        all.extend(friends.into_iter().map(|f| f.join().expect("friend")));
        let mut slots: Vec<u8> = all.iter().map(|(_, s)| s.local_index).collect();
        slots.sort();
        assert_eq!(slots, (0..MAX_PLAYERS as u8).collect::<Vec<_>>(), "each PC its own player slot");
        let host_prints = &all[0].1.my_prints;
        assert!(host_prints.len() >= 10, "sync checks ran");
        for (state, session) in &all {
            assert_eq!(state.desync_at, None);
            assert_eq!(state.closed, None);
            assert_eq!(&session.my_prints, host_prints, "player {}", session.local_index + 1);
        }
    }

    /// A friend who joins first waits as long as the last one takes (it used to give up after
    /// 30 s of silence from the host), and sees who is in while it waits.
    #[test]
    fn an_early_friend_waits_for_the_last_one_and_sees_who_is_in() {
        let Some(swf) = test_swf() else { return };
        let port = free_port();
        let three = |mut o: SessionOpts| {
            o.players = 3;
            o.game = Game::built_in("gun-mayhem-2").expect("built in");
            o
        };
        let host_swf = swf.clone();
        let host = thread::spawn(move || play(three(opts(Role::HostDirect { port }, &host_swf, 0)), 100, None));
        thread::sleep(Duration::from_millis(300));
        let lobby = Arc::new(Mutex::new(crate::coop::Lobby::default()));
        let early = {
            let (swf, lobby) = (swf.clone(), lobby.clone());
            thread::spawn(move || {
                let mut o = three(opts(Role::JoinDirect { addr: format!("127.0.0.1:{port}") }, &swf, 0));
                o.lobby = Some(lobby);
                play(o, 100, None)
            })
        };
        thread::sleep(Duration::from_secs(3));
        {
            let lobby = lobby.lock().expect("lobby");
            assert_eq!(lobby.needed, 3, "the friend hears how many the game needs");
            assert_eq!(lobby.players.len(), 2, "and who is in: {:?}", lobby.players);
        }
        // the last friend comes well after the friend's silence limit
        thread::sleep(HOST_SILENT + Duration::from_secs(3));
        let late = thread::spawn(move || {
            play(three(opts(Role::JoinDirect { addr: format!("127.0.0.1:{port}") }, &swf, 0)), 100, None)
        });
        for (name, pc) in [("host", host), ("early friend", early), ("late friend", late)] {
            let (state, session) = pc.join().expect(name);
            assert_eq!(state.closed, None, "{name}");
            assert!(session.frame() >= 100, "{name} played");
        }
    }

    #[test]
    fn stray_connection_during_the_game_is_ignored() {
        let Some(swf) = test_swf() else { return };
        let port = free_port();
        let host_swf = swf.clone();
        // paced (30 fps): 150 frames take 5 s, so the stray arrives mid-game
        let paced = |mut o: SessionOpts| {
            o.paced = true;
            o
        };
        let host = thread::spawn(move || {
            play(paced(opts(Role::HostDirect { port }, &host_swf, 0)), 150, None)
        });
        thread::sleep(Duration::from_millis(300));
        let join = thread::spawn(move || {
            let addr = format!("127.0.0.1:{port}");
            play(paced(opts(Role::JoinDirect { addr }, &swf, 0)), 150, None)
        });
        // a port scanner (or anyone) connects and leaves while the game runs
        thread::sleep(Duration::from_millis(2000));
        if let Ok(mut stray) = std::net::TcpStream::connect(("127.0.0.1", port)) {
            use std::io::Write;
            let _ = stray.write_all(b"IN|1|200|31|-|-|-\0");
        }
        let (host_state, host_session) = host.join().expect("host");
        let (join_state, _) = join.join().expect("join");
        assert_eq!(host_state.closed, None, "a stray connection ended the session");
        assert_eq!(host_state.desync_at, None);
        assert_eq!(join_state.desync_at, None);
        assert!(host_session.frame() >= 150);
    }

    #[test]
    fn refused_friend_can_retry_with_the_right_file() {
        let Some(swf) = test_swf() else { return };
        let dir = tempfile::tempdir().expect("temp dir");
        let wrong = dir.path().join("wrong.swf");
        std::fs::write(&wrong, b"another game").expect("write");
        let port = free_port();
        let host_swf = swf.clone();
        let host = thread::spawn(move || play(opts(Role::HostDirect { port }, &host_swf, 0), 300, None));
        thread::sleep(Duration::from_millis(300));
        let addr = format!("127.0.0.1:{port}");
        let refused = Session::start(opts(Role::JoinDirect { addr: addr.clone() }, &wrong, 0));
        assert!(refused.is_err());
        let (join_state, join_session) = play(opts(Role::JoinDirect { addr }, &swf, 0), 300, None);
        let (host_state, _) = host.join().expect("host");
        assert_eq!(join_state.closed, None);
        assert_eq!(host_state.closed, None);
        assert_eq!(join_session.local_index, 1);
    }

    #[test]
    fn different_player_settings_are_refused_by_name() {
        let dir = tempfile::tempdir().expect("temp dir");
        let game = dir.path().join("game.swf");
        std::fs::write(&game, b"one game").expect("write");
        let port = free_port();
        let host_game = game.clone();
        let host = thread::spawn(move || {
            let mut o = opts(Role::HostDirect { port }, &host_game, 0);
            o.host_wait = Some(Duration::from_secs(3));
            Session::start(o).err().map(|e| e.to_string())
        });
        thread::sleep(Duration::from_millis(300));
        let mut friend = opts(Role::JoinDirect { addr: format!("127.0.0.1:{port}") }, &game, 0);
        friend.settings = "frame_rate=Some(60.0); player_version=None".into();
        let err = Session::start(friend).err().expect("different settings must be refused");
        assert!(err.to_string().contains("frame_rate"), "{err}");
        host.join().expect("host");
    }

    #[test]
    fn different_game_file_is_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (a, b) = (dir.path().join("a.swf"), dir.path().join("b.swf"));
        std::fs::write(&a, b"one game").expect("write");
        std::fs::write(&b, b"another game").expect("write");
        let port = free_port();
        let host = thread::spawn(move || {
            let mut o = opts(Role::HostDirect { port }, &a, 0);
            o.host_wait = Some(Duration::from_secs(3));
            Session::start(o).err().map(|e| e.to_string())
        });
        thread::sleep(Duration::from_millis(300));
        let addr = format!("127.0.0.1:{port}");
        let err = Session::start(opts(Role::JoinDirect { addr }, &b, 0))
            .err()
            .expect("a different game file must be refused");
        assert!(err.to_string().contains("different game file"), "{err}");
        assert!(host.join().expect("host").is_some(), "host should give up after host_wait");
    }
}
