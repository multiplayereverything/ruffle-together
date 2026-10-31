//! Connections between players: direct (the host listens; works over Radmin, LAN or a forwarded
//! port) or through the central relay with a room code. Both carry NUL-framed protocol lines over
//! TCP with Nagle off. The host is player 0 and passes each friend's lines on to the other
//! friends, so more than 2 players work the same way.

use crate::coop::lockstep::MAX_PLAYERS;
use crate::coop::protocol::{Msg, decode, encode};
use std::io::{self, BufRead, BufReader, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// The relay's game id for Ruffle Together rooms.
pub const GAME_ID: &str = "ruffle-coop";

pub enum Role {
    HostDirect { port: u16 },
    JoinDirect { addr: String },
    HostRelay { relay: String },
    JoinRelay { relay: String, code: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Msg { from: u8, msg: Msg },
    Joined { player: u8, name: String },
    Left { player: u8 },
    /// The connection to the host (or relay) is gone, with the reason.
    Closed(String),
}

/// One peer's socket, written from several threads.
#[derive(Clone)]
struct Peer {
    id: u8,
    stream: Arc<Mutex<TcpStream>>,
}

impl Peer {
    fn new(id: u8, stream: TcpStream) -> io::Result<Self> {
        stream.set_nodelay(true)?;
        Ok(Self {
            id,
            stream: Arc::new(Mutex::new(stream)),
        })
    }

    fn write_line(&self, line: &str) {
        if let Ok(mut stream) = self.stream.lock() {
            let mut data = line.as_bytes().to_vec();
            data.push(0);
            let _ = stream.write_all(&data);
        }
    }

    fn shutdown(&self) {
        if let Ok(stream) = self.stream.lock() {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

/// Reads NUL-terminated lines; None at the end of the stream.
fn read_line(reader: &mut BufReader<TcpStream>) -> Option<String> {
    let mut buf = vec![];
    match reader.read_until(0, &mut buf) {
        Ok(0) | Err(_) => None,
        Ok(_) => {
            if buf.last() == Some(&0) {
                buf.pop();
            }
            Some(String::from_utf8_lossy(&buf).into_owned())
        }
    }
}

pub struct Link {
    /// Direct host: every friend. Everyone else: the one connection (to the host, or to the
    /// relay, which sends a relay host's lines to every friend).
    peers: Arc<Mutex<Vec<Peer>>>,
    events: Receiver<Event>,
    code: Option<String>,
    /// Direct host: whether new friends may still connect.
    accepting: Arc<AtomicBool>,
}

/// Sends on a link from another thread (the heartbeat) or when the game can't (a crash).
#[derive(Clone)]
pub struct LinkSender {
    peers: Arc<Mutex<Vec<Peer>>>,
}

impl LinkSender {
    pub fn send(&self, msg: &Msg) {
        let line = encode(msg);
        if let Ok(peers) = self.peers.lock() {
            for peer in peers.iter() {
                peer.write_line(&line);
            }
        }
    }
}

impl Link {
    pub fn send(&self, msg: &Msg) {
        self.sender().send(msg);
    }

    pub fn sender(&self) -> LinkSender {
        LinkSender {
            peers: self.peers.clone(),
        }
    }

    pub fn try_recv(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }

    /// Direct host: no more connections once the game has started (a port scanner or a late
    /// friend must not be able to disturb it).
    pub fn stop_accepting(&self) {
        self.accepting.store(false, Ordering::Relaxed);
    }

    /// Direct host: disconnects every friend except `keep`.
    pub fn keep_only(&self, keep: &[u8]) {
        if let Ok(mut peers) = self.peers.lock() {
            for peer in peers.iter().filter(|p| p.id != 0 && !keep.contains(&p.id)) {
                peer.shutdown();
            }
            peers.retain(|p| p.id == 0 || keep.contains(&p.id));
        }
    }

    /// The relay room's code (relay host only).
    pub fn room_code(&self) -> Option<String> {
        self.code.clone()
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        self.accepting.store(false, Ordering::Relaxed);
        if let Ok(peers) = self.peers.lock() {
            for peer in peers.iter() {
                peer.shutdown();
            }
        }
    }
}

pub fn connect(role: Role, version: &str, name: &str) -> io::Result<Link> {
    let (tx, events) = channel();
    let peers = Arc::new(Mutex::new(vec![]));
    let accepting = Arc::new(AtomicBool::new(true));
    let mut code = None;
    match role {
        Role::HostDirect { port } => {
            let listener = TcpListener::bind(("0.0.0.0", port))?;
            listener.set_nonblocking(true)?;
            let (peers, accepting) = (peers.clone(), accepting.clone());
            thread::spawn(move || accept_loop(listener, peers, accepting, tx));
        }
        Role::JoinDirect { addr } => {
            let stream = TcpStream::connect(addr)?;
            let peer = Peer::new(0, stream.try_clone()?)?;
            peers.lock().map_err(|_| io::Error::other("lock"))?.push(peer);
            thread::spawn(move || friend_loop(BufReader::new(stream), tx, false));
        }
        Role::HostRelay { relay } => {
            let (stream, mut reader) = relay_handshake(
                &relay,
                // unlisted room, up to MAX_PLAYERS (the relay caps it at mods-info's max_players), no
                // password, no handoff
                &format!("RELAY|2|HOST|{GAME_ID}|{version}|0|{MAX_PLAYERS}|||{}", clean_name(name)),
            )?;
            let mut room = None;
            loop {
                let line = read_line(&mut reader).ok_or_else(closed)?;
                if let Some(reason) = line.strip_prefix("E|") {
                    return Err(io::Error::other(reason.to_string()));
                } else if let Some(rest) = line.strip_prefix("ROOM|") {
                    room = rest.split('|').next().map(str::to_string);
                } else if line == "W|0" {
                    break;
                }
            }
            code = room;
            let peer = Peer::new(0, stream)?;
            peers.lock().map_err(|_| io::Error::other("lock"))?.push(peer.clone());
            thread::spawn(move || relay_host_loop(reader, peer, tx));
        }
        Role::JoinRelay { relay, code } => {
            let (stream, mut reader) = relay_handshake(
                &relay,
                &format!("RELAY|2|JOIN|{GAME_ID}|{version}|{code}||{}", clean_name(name)),
            )?;
            loop {
                let line = read_line(&mut reader).ok_or_else(closed)?;
                if let Some(reason) = line.strip_prefix("E|") {
                    return Err(io::Error::other(reason.to_string()));
                } else if line.starts_with("W|") {
                    break;
                }
            }
            let peer = Peer::new(0, stream)?;
            peers.lock().map_err(|_| io::Error::other("lock"))?.push(peer);
            thread::spawn(move || friend_loop(reader, tx, true));
        }
    }
    Ok(Link {
        peers,
        events,
        code,
        accepting,
    })
}

fn closed() -> io::Error {
    io::Error::new(io::ErrorKind::ConnectionAborted, "the relay closed the connection")
}

fn clean_name(name: &str) -> String {
    name.chars()
        .map(|c| if c == '|' || c.is_control() { ' ' } else { c })
        .collect()
}

fn relay_handshake(relay: &str, first: &str) -> io::Result<(TcpStream, BufReader<TcpStream>)> {
    let mut stream = TcpStream::connect(relay)?;
    stream.set_nodelay(true)?;
    stream.write_all(format!("{first}\0").as_bytes())?;
    let reader = BufReader::new(stream.try_clone()?);
    Ok((stream, reader))
}

/// Direct host: accepts friends (connections 1, 2, ...) until the game starts or the link is
/// dropped; the listening port closes then.
fn accept_loop(
    listener: TcpListener,
    peers: Arc<Mutex<Vec<Peer>>>,
    accepting: Arc<AtomicBool>,
    tx: Sender<Event>,
) {
    let mut next_id = 1u8;
    while accepting.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_nonblocking(false);
                let (Ok(reader), Ok(peer)) = (stream.try_clone(), Peer::new(next_id, stream)) else {
                    continue;
                };
                if let Ok(mut list) = peers.lock() {
                    list.push(peer.clone());
                }
                let _ = tx.send(Event::Joined {
                    player: next_id,
                    name: String::new(),
                });
                let (peers, tx) = (peers.clone(), tx.clone());
                thread::spawn(move || direct_guest_loop(BufReader::new(reader), peer, peers, tx));
                next_id += 1;
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20))
            }
            Err(_) => thread::sleep(Duration::from_millis(20)),
        }
    }
}

/// Direct host: one friend's lines, passed on to the other friends.
fn direct_guest_loop(
    mut reader: BufReader<TcpStream>,
    peer: Peer,
    peers: Arc<Mutex<Vec<Peer>>>,
    tx: Sender<Event>,
) {
    while let Some(line) = read_line(&mut reader) {
        if let Ok(list) = peers.lock() {
            for other in list.iter().filter(|p| p.id != peer.id) {
                other.write_line(&line);
            }
        }
        if let Ok(msg) = decode(&line) {
            let _ = tx.send(Event::Msg { from: peer.id, msg });
        }
    }
    if let Ok(mut list) = peers.lock() {
        list.retain(|p| p.id != peer.id);
    }
    let _ = tx.send(Event::Left { player: peer.id });
}

/// Relay host: friends arrive as J|id|name, their lines as G|id|line, leaving as L|id.
fn relay_host_loop(mut reader: BufReader<TcpStream>, relay: Peer, tx: Sender<Event>) {
    let mut friends = 0usize;
    while let Some(line) = read_line(&mut reader) {
        let mut parts = line.splitn(3, '|');
        match (parts.next(), parts.next(), parts.next()) {
            (Some("J"), Some(id), name) => {
                if let Ok(player) = id.parse() {
                    friends += 1;
                    let name = name.unwrap_or_default().to_string();
                    let _ = tx.send(Event::Joined { player, name });
                }
            }
            (Some("L"), Some(id), _) => {
                if let Ok(player) = id.parse() {
                    friends = friends.saturating_sub(1);
                    let _ = tx.send(Event::Left { player });
                }
            }
            (Some("G"), Some(id), Some(body)) => {
                // the relay only sends host lines to everyone, so the host passes a friend's
                // lines on when there are other friends to see them
                if friends > 1 {
                    relay.write_line(body);
                }
                if let (Ok(from), Ok(msg)) = (id.parse(), decode(body)) {
                    let _ = tx.send(Event::Msg { from, msg });
                }
            }
            (Some("E"), reason, _) => {
                let _ = tx.send(Event::Closed(reason.unwrap_or_default().to_string()));
                return;
            }
            _ => {}
        }
    }
    let _ = tx.send(Event::Closed("the relay closed the connection".into()));
}

/// A friend: everything arrives from the host (directly, or through the relay unchanged).
fn friend_loop(mut reader: BufReader<TcpStream>, tx: Sender<Event>, relay: bool) {
    while let Some(line) = read_line(&mut reader) {
        if relay && line == "X" {
            break;
        }
        if relay && let Some(reason) = line.strip_prefix("E|") {
            let _ = tx.send(Event::Closed(reason.to_string()));
            return;
        }
        if let Ok(msg) = decode(&line) {
            let _ = tx.send(Event::Msg { from: 0, msg });
        }
    }
    let _ = tx.send(Event::Closed(format!("the host's connection closed {NO_GOODBYE}")));
}

/// Why a connection closed without a Bye message (a closed window and a crash both send one).
pub const NO_GOODBYE: &str =
    "without a goodbye: its Ruffle was killed or froze and was ended, or the network dropped";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coop::lockstep::{InputFrame, UP};
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

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

    /// Waits up to `secs` for an event matching `want`, skipping others.
    fn wait_for(link: &Link, secs: u64, want: impl Fn(&Event) -> bool) -> Event {
        let end = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < end {
            if let Some(event) = link.try_recv() {
                if want(&event) {
                    return event;
                }
            } else {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        panic!("timed out waiting for an event");
    }

    fn hello(name: &str) -> Msg {
        Msg::Hello {
            build: "test".into(),
            swf_sha256: "abc".into(),
            settings: String::new(),
            name: name.into(),
            id: 1,
        }
    }

    struct Relay(Child, u16);

    impl Drop for Relay {
        fn drop(&mut self) {
            let _ = self.0.kill();
        }
    }

    /// The real relay (global-mods-server/init.py) on a free port with a `ruffle-coop` star game,
    /// or None when it isn't next to this repo (set COOP_RELAY to its init.py).
    fn start_relay() -> Option<Relay> {
        let init = std::env::var("COOP_RELAY").map(PathBuf::from).unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../global-mods-server/init.py")
        });
        if !init.exists() {
            eprintln!("skipping: no relay at {}", init.display());
            return None;
        }
        let dir = tempfile::tempdir().expect("temp dir").keep();
        let games = dir.join("games.json");
        std::fs::write(
            &games,
            r#"{"ruffle-coop": {"name": "Ruffle Together", "mode": "star", "max_players": 8}}"#,
        )
        .expect("games.json");
        let port = free_port();
        let child = Command::new("python")
            .arg(&init)
            .args(["--port", &port.to_string(), "--no-mods-info", "--no-discord", "--games"])
            .arg(&games)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start relay");
        let relay = Relay(child, port);
        let end = Instant::now() + Duration::from_secs(10);
        while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
            assert!(Instant::now() < end, "relay didn't start");
            std::thread::sleep(Duration::from_millis(100));
        }
        Some(relay)
    }

    #[test]
    fn direct_roundtrip() {
        let port = free_port();
        let host = connect(Role::HostDirect { port }, "v#1", "Host").expect("host");
        let join = connect(
            Role::JoinDirect {
                addr: format!("127.0.0.1:{port}"),
            },
            "v#1",
            "Friend",
        )
        .expect("join");
        wait_for(&host, 5, |e| matches!(e, Event::Joined { player: 1, .. }));
        join.send(&hello("Friend"));
        let got = wait_for(&host, 5, |e| matches!(e, Event::Msg { .. }));
        assert_eq!(
            got,
            Event::Msg {
                from: 1,
                msg: hello("Friend")
            }
        );
        host.send(&hello("Host"));
        let got = wait_for(&join, 5, |e| matches!(e, Event::Msg { .. }));
        assert_eq!(
            got,
            Event::Msg {
                from: 0,
                msg: hello("Host")
            }
        );
    }

    #[test]
    fn relay_roundtrip() {
        let Some(relay) = start_relay() else { return };
        let addr = format!("127.0.0.1:{}", relay.1);
        let host = connect(Role::HostRelay { relay: addr.clone() }, "v#1", "Host").expect("host");
        let code = host.room_code().expect("room code");
        assert_eq!(code.len(), 5);
        let join = connect(
            Role::JoinRelay {
                relay: addr,
                code,
            },
            "v#1",
            "Friend",
        )
        .expect("join");
        let joined = wait_for(&host, 5, |e| matches!(e, Event::Joined { .. }));
        assert_eq!(
            joined,
            Event::Joined {
                player: 1,
                name: "Friend".into()
            }
        );
        let input = Msg::Input {
            player: 1,
            frame: 3,
            input: InputFrame {
                buttons: UP,
                ..Default::default()
            },
        };
        join.send(&input);
        assert_eq!(
            wait_for(&host, 5, |e| matches!(e, Event::Msg { .. })),
            Event::Msg {
                from: 1,
                msg: input
            }
        );
        host.send(&hello("Host"));
        assert_eq!(
            wait_for(&join, 5, |e| matches!(e, Event::Msg { .. })),
            Event::Msg {
                from: 0,
                msg: hello("Host")
            }
        );
    }

    #[test]
    fn relay_version_mismatch() {
        let Some(relay) = start_relay() else { return };
        let addr = format!("127.0.0.1:{}", relay.1);
        let host = connect(Role::HostRelay { relay: addr.clone() }, "v#1", "Host").expect("host");
        let code = host.room_code().expect("room code");
        let result = connect(Role::JoinRelay { relay: addr, code }, "v#2", "Friend");
        let err = result.err().expect("a different build must be refused");
        assert!(err.to_string().contains("Version mismatch"), "{err}");
    }

    #[test]
    fn friend_leaving_is_reported() {
        let port = free_port();
        let host = connect(Role::HostDirect { port }, "v#1", "Host").expect("host");
        let join = connect(
            Role::JoinDirect {
                addr: format!("127.0.0.1:{port}"),
            },
            "v#1",
            "Friend",
        )
        .expect("join");
        wait_for(&host, 5, |e| matches!(e, Event::Joined { .. }));
        drop(join);
        wait_for(&host, 5, |e| matches!(e, Event::Left { player: 1 }));
    }

    #[test]
    fn host_leaving_closes_the_friend() {
        let Some(relay) = start_relay() else { return };
        let addr = format!("127.0.0.1:{}", relay.1);
        let host = connect(Role::HostRelay { relay: addr.clone() }, "v#1", "Host").expect("host");
        let code = host.room_code().expect("room code");
        let join = connect(Role::JoinRelay { relay: addr, code }, "v#1", "Friend").expect("join");
        wait_for(&host, 5, |e| matches!(e, Event::Joined { .. }));
        drop(host);
        wait_for(&join, 5, |e| matches!(e, Event::Closed(_)));
    }
}
