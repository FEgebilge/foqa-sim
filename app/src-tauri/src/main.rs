//! FOQA-Sim desktop app. Sits in the tray and records every MSFS flight in
//! process (the aircraft segment), the way a QAR records whenever the
//! aircraft is powered; after each landing it notifies the pilot with the
//! touchdown and the worst event, and the window shows the debrief (the
//! ground segment). Commands and events: `docs/debrief-api.md`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod attempts;
mod commands;
#[cfg(feature = "connect")]
mod connect;
mod exports;
mod insim;
mod library;
mod links;
mod setup;

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use ::debrief::{LandingNotice, RecorderStatus, Station};
use attempts::Attempts;
use capture::{Happening, Service};
use commands::{Shared, lock};
use library::Library;
use serde::Serialize;
use setup::Setup;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WindowEvent, Wry};
use tauri_plugin_notification::NotificationExt;

/// How long Quit waits for the recorder to close an open flight's files.
const QUIT_GRACE: Duration = Duration::from_secs(3);
/// The notification's payload key naming the flight to open on a click.
const FLIGHT_ID: &str = "flight_id";
/// The notification's payload key naming the challenge to open on a click.
pub(crate) const CHALLENGE_KEY: &str = "challenge_key";

/// The core commands, and those of the optional modules given.
macro_rules! handler {
    ($($extra:path),*) => {
        tauri::generate_handler![
            commands::list_flights,
            commands::debrief,
            commands::delete_flight,
            commands::place_challenge,
            commands::challenges_list,
            commands::challenge_import,
            commands::challenge_remove,
            commands::challenge_briefing,
            commands::challenge_preflight,
            commands::attempt_start,
            commands::attempt_fly_again,
            commands::attempt_finish,
            commands::attempt_abandon,
            commands::attempts_list,
            commands::recorder_status,
            commands::open_flights_folder,
            commands::storage_usage,
            commands::setup_status,
            commands::setup_install,
            commands::setup_remove,
            commands::setup_use_community,
            commands::setup_pick_community,
            exports::export_flight,
            exports::export_logbook,
            exports::save_image
            $(, $extra)*
        ]
    };
}

type Commands = fn(tauri::ipc::Invoke<Wry>) -> bool;

fn main() {
    #[cfg(feature = "connect")]
    let commands: Commands = handler!(
        connect::connect_available,
        connect::sites_list,
        connect::site_add,
        connect::site_sign_in,
        connect::site_sign_in_cancel,
        connect::site_sign_out,
        connect::site_remove,
        connect::site_challenges,
        connect::site_challenge_take,
        connect::site_attempts,
        connect::attempt_upload,
        connect::site_open,
        connect::deep_link_pending,
        connect::deep_link_consent,
        connect::deep_link_open,
        connect::deep_link_done,
        connect::site_settings,
        connect::site_settings_set
    );
    #[cfg(not(feature = "connect"))]
    let commands: Commands = handler!(links::deep_link_pending, links::deep_link_done);
    let builder = tauri::Builder::default()
        // First, so a second launch only brings this one's window up, and
        // hands on a link it was started with, as its argument (B2).
        .plugin(tauri_plugin_single_instance::init(|app, args, _| {
            show(app, None);
            #[cfg(feature = "connect")]
            if let Some(connect) = app.try_state::<Arc<connect::Connect>>() {
                links::in_args(args).for_each(|link| connect.open_url(&link));
            }
            #[cfg(not(feature = "connect"))]
            links::arrived(app, args);
        }));
    // The `foqa-sim` scheme (tauri.conf.json; the installer registers it).
    // Links themselves are read from the command line, not from this plugin,
    // which hands them on already parsed (normalised).
    #[cfg(feature = "connect")]
    let builder = builder.plugin(tauri_plugin_deep_link::init());
    #[cfg(not(feature = "connect"))]
    let builder = builder.manage(links::Disabled::default());
    builder
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        // Off until the user enables it (`plugin:autostart|enable`).
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .invoke_handler(commands)
        .setup(|app| {
            let station = Arc::new(Station::core()?);
            let dir = app.path().document_dir()?.join("FOQA-Sim").join("flights");
            // FOQA_RAW=1 also keeps raw captures, to replay through a changed chain.
            let mut service = Service::new(dir.clone(), Arc::clone(&station))?
                .raw(std::env::var_os("FOQA_RAW").is_some());
            let (stopper, index, sim) =
                (service.stopper(), service.index(), service.sim_requests());
            let (views, view_commands) = (service.views(), service.app_commands());
            let (tx, rx) = mpsc::channel();
            let recorder = thread::Builder::new()
                .name("recorder".into())
                .spawn(move || service.run(tx))?;
            let setup = Setup::new(app.path().app_config_dir()?, app.path().resource_dir().ok());
            // Challenges and attempts live with the app's data, out of synced Documents.
            let data = app.path().app_data_dir()?;
            let library = Library::new(data.join("challenges"))?;
            let handle = app.handle().clone();
            #[cfg(feature = "connect")]
            let connect = connect::Connect::start(
                data.clone(),
                app.path().app_config_dir()?,
                data.join("attempts"),
                handle.clone(),
            )?;
            #[cfg(feature = "connect")]
            let site = Arc::clone(&connect);
            let (changed, strip) = mpsc::channel();
            let attempts = Attempts::new(data.join("attempts"))?.on_change(move |record| {
                if let Err(e) = handle.emit("attempt-changed", record) {
                    eprintln!("event not sent: {e}");
                }
                let _ = changed.send(());
                #[cfg(feature = "connect")]
                site.attempt_changed(record);
            });
            let shared = Shared::new(
                station, dir, stopper, index, recorder, setup, sim, library, attempts, views,
            );
            #[cfg(feature = "connect")]
            let shared = shared.with_hooks(Arc::clone(&connect) as Arc<dyn commands::AttemptHooks>);
            let shared = Arc::new(shared);
            shared.in_sim(strip, view_commands);
            #[cfg(feature = "connect")]
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                let links = app.deep_link();
                // The installer registers the scheme; registering again at
                // start-up repairs an install that missed it (B2).
                if let Err(e) = links.register(::connect::deeplink::SCHEME) {
                    eprintln!("deep links not registered: {e}");
                }
                // A link that started the app.
                links::in_args(std::env::args()).for_each(|link| connect.open_url(&link));
                app.manage(connect);
            }
            #[cfg(not(feature = "connect"))]
            links::arrived(app.handle(), std::env::args());
            app.manage(Arc::clone(&shared));
            let recover = Arc::clone(&shared);
            thread::spawn(move || recover.recover());
            // After an update, the in-sim packages follow the app's version.
            let refresh = Arc::clone(&shared);
            thread::spawn(move || {
                for (name, result) in refresh.setup.refresh() {
                    match result {
                        Ok(()) => eprintln!("{name}: brought up to date"),
                        Err(e) => eprintln!("{name}: could not be brought up to date: {e}"),
                    }
                }
            });
            // Brings the flight list's index up to date before it is asked for.
            let warm = Arc::clone(&shared);
            thread::spawn(move || warm.summaries());

            let handle = app.handle().clone();
            app.notification().on_action(move |action| {
                let extra = |key| action.notification()?.extra().get(key)?.as_str();
                if action.action_id() != "tap" {
                    return;
                }
                show(&handle, extra(FLIGHT_ID));
                if let Some(key) = extra(CHALLENGE_KEY) {
                    let _ = handle.emit("open-challenge", OpenChallenge { key });
                }
            })?;
            let tray = Tray::new(app.handle())?;
            let handle = app.handle().clone();
            thread::spawn(move || relay(&handle, &shared, &tray, rx));
            Ok(())
        })
        // The window only hides: the app keeps recording from the tray.
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running FOQA-Sim");
}

/// Hands what the recorder reports to the tray, the window and the
/// notification centre, until the recorder is gone.
fn relay(app: &AppHandle, shared: &Arc<Shared>, tray: &Tray, happenings: Receiver<Happening>) {
    for happening in happenings {
        let sent = match happening {
            Happening::Status(status) => {
                tray.show(&status);
                *lock(&shared.status) = status.clone();
                app.emit("recorder-status", status)
            }
            Happening::FlightSaved(summary) => {
                shared.flight_closed(&summary.id);
                app.emit("flight-saved", summary)
            }
            Happening::Landing(notice) => {
                shared.landed(&notice);
                notify(app, &notice);
                app.emit("landing", notice)
            }
        };
        if let Err(e) = sent {
            eprintln!("event not sent: {e}");
        }
    }
}

/// `Landing LTBJ 16L — 103 fpm, 1.13 g`, and the most severe event (the
/// first of equals) or that there was none.
fn notify(app: &AppHandle, notice: &LandingNotice) {
    let td = &notice.touchdown;
    let runway = td
        .runway
        .as_ref()
        .map_or(String::new(), |r| format!(" {} {}", r.icao, r.runway));
    let title = format!(
        "Landing{runway} — {:.0} fpm, {:.2} g",
        td.sink_fpm, td.peak_g
    );
    let worst = notice.events.iter().rev().max_by_key(|e| e.level);
    let body = worst.map_or("No events".to_owned(), |e| {
        let more = notice.events.len() - 1;
        let more = if more > 0 {
            format!(" (+{more} more)")
        } else {
            String::new()
        };
        format!("{:?}: {}{more}", e.level, e.title)
    });
    let shown = app
        .notification()
        .builder()
        .title(title)
        .body(body)
        .extra(FLIGHT_ID, &notice.flight_id)
        .show();
    if let Err(e) = shown {
        eprintln!("notification not shown: {e}");
    }
}

/// Brings the debrief window up, on a flight if one is named: the frontend
/// opens it on the `open-flight` event.
pub(crate) fn show(app: &AppHandle, flight_id: Option<&str>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    if let Some(flight_id) = flight_id {
        let _ = app.emit("open-flight", OpenFlight { flight_id });
    }
}

#[derive(Clone, Serialize)]
struct OpenFlight<'a> {
    flight_id: &'a str,
}

/// Payload of `open-challenge`: a click on a site's verdict toast.
#[derive(Clone, Serialize)]
struct OpenChallenge<'a> {
    key: &'a str,
}

/// Stops the recorder, which closes an open flight's files, then exits;
/// gives up waiting after `QUIT_GRACE` (the journal survives a kill).
fn quit(app: &AppHandle) {
    let app = app.clone();
    thread::spawn(move || {
        let shared = app.state::<Arc<Shared>>();
        shared.stopper.stop();
        if let Some(recorder) = lock(&shared.recorder).take() {
            let until = Instant::now() + QUIT_GRACE;
            while !recorder.is_finished() && Instant::now() < until {
                thread::sleep(Duration::from_millis(50));
            }
        }
        app.exit(0);
    });
}

/// The tray icon: the recorder's state in its tooltip and in the first,
/// disabled line of its menu.
struct Tray {
    icon: TrayIcon<Wry>,
    status: MenuItem<Wry>,
}

impl Tray {
    fn new(app: &AppHandle) -> tauri::Result<Self> {
        let status = MenuItem::with_id(app, "status", "", false, None::<&str>)?;
        let item = |id: &str, text: &str| MenuItem::with_id(app, id, text, true, None::<&str>);
        let menu = Menu::with_items(
            app,
            &[
                &status,
                &PredefinedMenuItem::separator(app)?,
                &item("open", "Open debrief")?,
                &item("folder", "Open flights folder")?,
                &PredefinedMenuItem::separator(app)?,
                &item("quit", "Quit")?,
            ],
        )?;
        let mut builder = TrayIconBuilder::with_id("main")
            .menu(&menu)
            .show_menu_on_left_click(false)
            .on_menu_event(|app, event| match event.id().as_ref() {
                "open" => show(app, None),
                "folder" => {
                    if let Err(e) = app.state::<Arc<Shared>>().open_folder() {
                        eprintln!("flights folder: {e}");
                    }
                }
                "quit" => quit(app),
                _ => {}
            })
            .on_tray_icon_event(|tray, event| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
                {
                    show(tray.app_handle(), None);
                }
            });
        if let Some(icon) = app.default_window_icon() {
            builder = builder.icon(icon.clone());
        }
        let tray = Self {
            icon: builder.build(app)?,
            status,
        };
        tray.show(&RecorderStatus::WaitingForSim);
        Ok(tray)
    }

    fn show(&self, status: &RecorderStatus) {
        let text = match status {
            RecorderStatus::WaitingForSim => "Waiting for MSFS".to_owned(),
            RecorderStatus::Connected => "Connected to MSFS".to_owned(),
            RecorderStatus::Recording { elapsed_s, .. } => {
                format!("Recording, {:.0} min", (elapsed_s / 60.0).floor())
            }
            RecorderStatus::Error { message } => format!("Error: {message}"),
        };
        let _ = self.icon.set_tooltip(Some(format!("FOQA-Sim: {text}")));
        let _ = self.status.set_text(text);
    }
}
