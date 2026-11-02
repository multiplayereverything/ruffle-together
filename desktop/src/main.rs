#![deny(clippy::unwrap_used)]
// By default, Windows creates an additional console window for our program.
//
//
// This is silently ignored on non-windows systems.
// See https://docs.microsoft.com/en-us/cpp/build/reference/subsystem?view=msvc-160 for details.
#![windows_subsystem = "windows"]

mod app;
mod backends;
mod cli;
mod coop;
mod custom_event;
mod dbus;
mod gui;
mod log;
mod player;
mod preferences;
#[cfg(feature = "tracy")]
mod tracy;
mod util;
#[cfg(windows)]
mod windows;

use crate::preferences::GlobalPreferences;
use anyhow::{Context, Error};
use app::App;
#[cfg(feature = "shell-completions")]
use clap::CommandFactory;
use clap::Parser;
use cli::Opt;
use rfd::MessageDialogResult;
use ruffle_core::StaticCallstack;
use std::cell::RefCell;
use std::env;
use std::fs::File;
use std::panic::PanicHookInfo;
use tracing_subscriber::fmt::Layer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use url::Url;

thread_local! {
    static CALLSTACK: RefCell<Option<StaticCallstack>> = RefCell::default();
    static RENDER_INFO: RefCell<Option<String>> = RefCell::default();
    static SWF_INFO: RefCell<Option<String>> = RefCell::default();
}

#[cfg(feature = "tracy")]
#[global_allocator]
static GLOBAL: tracing_tracy::client::ProfiledAllocator<std::alloc::System> =
    tracing_tracy::client::ProfiledAllocator::new(std::alloc::System, 0);

static RUFFLE_VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    "-",
    env!("CFG_RELEASE_CHANNEL"),
    " (",
    env!("VERGEN_GIT_SHA"),
    " ",
    env!("VERGEN_GIT_COMMIT_DATE"),
    ")"
);

fn truncate_to_line_boundary(text: String, max_len: usize) -> String {
    if text.len() <= max_len {
        return text;
    }
    let mut truncated = String::with_capacity(max_len + 10);
    for line in text.lines() {
        if truncated.len() + line.len() > max_len {
            break;
        }
        truncated.push_str(line);
        truncated.push('\n');
    }
    format!("{truncated}[...]")
}

fn panic_hook(info: &PanicHookInfo) {
    CALLSTACK.with(|callstack| {
        if let Some(callstack) = &*callstack.borrow() {
            callstack.avm2(|callstack| println!("AVM2 stack trace: {callstack}"))
        }
    });

    let message = info.payload_as_str().unwrap_or("panic occurred");

    // Ruffle Together: the crash goes in the log, and the other players hear why this PC left
    // (before the dialog below, which waits for a click)
    tracing::error!("Ruffle crashed: {info}");
    coop::goodbye(&format!("Ruffle crashed: {message}"));

    if rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("Ruffle")
        .set_description(format!(
            "Ruffle has encountered a fatal error, this is a bug.\n\n\
            {message}\n\n\
            Please report this to us so that we can fix it. Thank you!\n\
            Pressing Yes will open a browser window."
        ))
        .set_buttons(rfd::MessageButtons::YesNo)
        .show()
        == MessageDialogResult::Yes
    {
        let backtrace = std::backtrace::Backtrace::force_capture();
        let panic_text = format!("```\n{info}\n\nStack trace:\n{backtrace}\n```");

        // The URL cannot be too long.
        let panic_text = truncate_to_line_boundary(panic_text, 2048);

        let mut params = vec![
            ("panic_text", panic_text),
            ("platform", "Desktop app".to_string()),
            ("operating_system", os_info::get().to_string()),
            ("ruffle_version", RUFFLE_VERSION.to_string()),
        ];
        let mut extra_info = vec![];
        SWF_INFO.with(|i| {
            if let Some(swf_name) = i.take() {
                extra_info.push(format!("Filename: {swf_name}\n"));
                params.push(("title", format!("Crash on {swf_name}")));
            }
        });
        CALLSTACK.with(|callstack| {
            if let Some(callstack) = &*callstack.borrow() {
                callstack.avm2(|callstack| {
                    extra_info.push(format!("### AVM2 Callstack\n```{callstack}\n```\n"));
                });
            }
        });
        RENDER_INFO.with(|i| {
            if let Some(render_info) = i.take() {
                extra_info.push(format!("### Render Info\n{render_info}\n"));
            }
        });
        if !extra_info.is_empty() {
            params.push(("extra_info", extra_info.join("\n")));
        }
        if let Ok(url) = Url::parse_with_params(
            "https://github.com/ruffle-rs/ruffle/issues/new?assignees=&labels=bug&template=crash_report.yml",
            &params,
        ) {
            let _ = webbrowser::open(url.as_str());
        }
    }
}

fn main() -> Result<(), Error> {
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        prev_hook(info);
        panic_hook(info);
    }));

    #[cfg(windows)]
    let _console = windows::Console::attach();

    let opt = Opt::parse();

    if opt.coop.coop_replay.is_some() {
        return coop::replay::run_cli(&opt.coop);
    }

    #[cfg(feature = "shell-completions")]
    if let Some(shell) = opt.completions {
        let mut cmd = Opt::command();
        clap_complete::generate(shell, &mut cmd, "ruffle", &mut std::io::stdout());
        return Ok(());
    }

    let mut preferences = GlobalPreferences::load(opt)?;

    let logs_path = &preferences.cli.cache_directory.join("log");
    let headless = preferences.cli.coop.coop_headless;
    let log_path = log::log_path(preferences.log_filename_pattern(), logs_path, headless);
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if let Err(err) = migrate_logs(&preferences, logs_path) {
        tracing::warn!("Failed to migrate logs: {}", err);
    }

    // [NA] `_guard` cannot be `_` or it'll immediately drop
    // https://docs.rs/tracing-appender/latest/tracing_appender/non_blocking/index.html
    if preferences.log_filename_pattern() == log::FilenamePattern::SingleFile && !headless {
        log::keep_old_logs(&log_path, log::OLD_LOGS);
    }
    let (non_blocking_file, _file_guard) = tracing_appender::non_blocking(File::create(log_path)?);
    let (non_blocking_stdout, _stdout_guard) = tracing_appender::non_blocking(std::io::stdout());

    let env_filter = tracing_subscriber::EnvFilter::builder().parse_lossy(
        env::var("RUST_LOG")
            .as_deref()
            .unwrap_or("warn,ruffle=info,avm_trace=info"),
    );

    let subscriber = tracing_subscriber::registry()
        .with(env_filter)
        .with(Layer::new().with_writer(non_blocking_stdout))
        .with(Layer::new().with_writer(non_blocking_file).with_ansi(false));

    #[cfg(feature = "tracy")]
    let subscriber = {
        let tracy_subscriber = tracing_tracy::TracyLayer::new(tracy::RuffleTracyConfig::default());
        subscriber.with(tracy_subscriber)
    };

    subscriber.init();

    // one Ruffle window at a time: opening another game closes the one still open (before a
    // session starts, so a direct host's port is free again)
    if !preferences.cli.coop.coop_headless {
        close_previous_window(&preferences.cli.cache_directory);
    }

    // Ruffle Together: connect and agree on the game before the window opens
    if let Some(role) = preferences.cli.coop.role()? {
        let coop = &preferences.cli.coop;
        let swf = preferences
            .cli
            .movie_url
            .as_ref()
            .and_then(|url| url.to_file_path().ok())
            .context("Ruffle Together needs the game file (a local SWF path)")?;
        let script = match &coop.coop_script {
            Some(path) => Some(
                coop::script::Script::parse(&std::fs::read_to_string(path)?)
                    .map_err(|e| anyhow::anyhow!(e))?,
            ),
            None => None,
        };
        // the window shows the room code and who has joined while the handshake runs (headless:
        // no window, the handshake runs here)
        let lobby = (!coop.coop_headless).then(|| {
            std::sync::Arc::new(std::sync::Mutex::new(coop::Lobby {
                hosting: matches!(role, coop::net::Role::HostDirect { .. } | coop::net::Role::HostRelay { .. }),
                port: match role {
                    coop::net::Role::HostDirect { port } => Some(port),
                    _ => None,
                },
                needed: coop.coop_players,
                ..Default::default()
            }))
        });
        coop::set_mouse_debug(coop.coop_mouse_debug);
        let opts = coop::session::SessionOpts {
            role,
            name: coop.coop_name.clone(),
            delay: coop.coop_delay,
            swf: swf.clone(),
            game: coop.game()?,
            players: coop.players()?,
            save_dir: Some(preferences.cli.save_directory.clone()),
            save_start: coop.coop_save,
            script,
            options: coop.options()?,
            link_id: coop.coop_link_id.clone(),
            host_wait: None,
            start_skew_ms: 0,
            paced: true,
            // the keys must match too: a different key map is refused by name, not a desync
            settings: format!(
                "{}; keys={}",
                coop::session::settings_line(&player::LaunchOptions::from(&preferences)),
                coop.game()?.id()
            ),
            lobby: lobby.clone(),
        };
        match lobby {
            None => {
                let session = coop::session::Session::start(opts).inspect_err(|error| {
                    tracing::error!("Ruffle Together: {error:#}");
                })?;
                return coop::session::run_headless(session, &swf, coop.coop_frames);
            }
            Some(lobby) => {
                coop::set_lobby(lobby.clone());
                std::thread::spawn(move || match coop::session::Session::start(opts) {
                    Ok(session) => coop::set_session(session),
                    Err(error) => {
                        tracing::error!("Ruffle Together: {error:#}");
                        if let Ok(mut lobby) = lobby.lock() {
                            lobby.error = Some(format!("{error:#}"));
                        }
                    }
                });
            }
        }
        // no menu in a session: it changes things the game sees (scale, quality, fullscreen)
        // or opens another game
        preferences.cli.no_gui = true;
    }

    // Ruffle Together TV mode (from the launcher's TV mode): full screen, played with a controller
    // (a lab's pad script plays it in a window)
    if coop::tv::requested() {
        preferences.cli.fullscreen = true;
    }
    if coop::tv::requested() || preferences.cli.coop.coop_pad_script.is_some() {
        let script = match &preferences.cli.coop.coop_pad_script {
            Some(path) => Some(coop::tv::PadScript::parse(&std::fs::read_to_string(path)?).map_err(|e| anyhow::anyhow!(e))?),
            None => None,
        };
        coop::tv::start(script);
        preferences.cli.no_gui = true;
    }

    let result = App::new(preferences).and_then(|(mut app, event_loop)| {
        event_loop.run_app(&mut app).context("Event loop failure")
    });

    #[cfg(windows)]
    if let Err(error) = &result {
        eprintln!("{:?}", error)
    }

    result
}

/// Closes the Ruffle window this cache directory last started, if it is still open, and records
/// this one. Labs give each window its own cache directory, so they can run several.
fn close_previous_window(cache_directory: &std::path::Path) {
    let pid_file = cache_directory.join("window.pid");
    #[cfg(windows)]
    if let Ok(text) = std::fs::read_to_string(&pid_file)
        && let Ok(pid) = text.trim().parse::<u32>()
        && pid != std::process::id()
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let run = |program: &str, args: &[&str]| {
            std::process::Command::new(program)
                .args(args)
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .map(|out| String::from_utf8_lossy(&out.stdout).to_lowercase())
                .unwrap_or_default()
        };
        let filter = format!("PID eq {pid}");
        let is_ruffle = || run("tasklist", &["/FI", &filter, "/NH"]).contains("ruffle_desktop");
        // process numbers are reused: only a Ruffle process is closed
        if is_ruffle() {
            tracing::info!("Closing the Ruffle window that was already open (process {pid})");
            run("taskkill", &["/PID", &pid.to_string(), "/F"]);
            for _ in 0..40 {
                if !is_ruffle() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    }
    let _ = std::fs::create_dir_all(cache_directory);
    let _ = std::fs::write(&pid_file, std::process::id().to_string());
}

/// Move logs from config directory into proper log directory.
///
/// This exists because in older versions Ruffle created log files in the config directory.
///
/// TODO Remove this after some time.
fn migrate_logs(
    preferences: &GlobalPreferences,
    log_path: &std::path::Path,
) -> std::io::Result<()> {
    tracing::debug!("Migrating logs from config directory to log directory");
    let config_path = &preferences.cli.config;
    let paths = std::fs::read_dir(config_path)?;

    for dir in paths.flatten() {
        if dir.file_name().as_encoded_bytes().ends_with(b".log") {
            let src = dir.path();
            let dest = log_path.join(dir.file_name());
            tracing::info!(
                "Moving log file {} -> {}",
                src.to_string_lossy(),
                dest.to_string_lossy()
            );
            if let Err(err) = std::fs::rename(&src, &dest) {
                tracing::warn!(
                    "Failed to move log file {} -> {}: {err}",
                    src.to_string_lossy(),
                    dest.to_string_lossy()
                )
            }
        }
    }

    Ok(())
}
