//! Links from websites as the command line carries them (B2): Windows
//! starts the app, or a second instance that hands its arguments on, with
//! the link as an argument, exactly as the browser gave it. The `connect`
//! feature parses it from there (so its checks see the raw text); without
//! it the link is recognised, never parsed or acted on, and the window says
//! that sites are disabled in this build.

/// A command-line argument that is a `foqa-sim:` link.
pub fn is_link(arg: &str) -> bool {
    arg.get(..9)
        .is_some_and(|s| s.eq_ignore_ascii_case("foqa-sim:"))
}

/// The links among a process's arguments (the first, the program, skipped).
pub fn in_args(args: impl IntoIterator<Item = String>) -> impl Iterator<Item = String> {
    args.into_iter().skip(1).filter(|a| is_link(a))
}

#[cfg(not(feature = "connect"))]
pub use disabled::*;

#[cfg(not(feature = "connect"))]
mod disabled {
    use std::sync::Mutex;

    use serde_json::{Value, json};
    use tauri::{AppHandle, Emitter, Manager, State};

    use crate::commands::lock;

    /// A link arrived and is not yet told.
    #[derive(Default)]
    pub struct Disabled(Mutex<bool>);

    const TEXT: &str = "Sites are disabled in this build.";

    /// The app was started, or raised by a second start, with `args`.
    pub fn arrived(app: &AppHandle, args: impl IntoIterator<Item = String>) {
        if super::in_args(args).next().is_none() {
            return;
        }
        eprintln!("deep link ignored: sites are disabled in this build");
        *lock(&app.state::<Disabled>().0) = true;
        let _ = app.emit("deep-link", ());
        crate::show(app, None);
    }

    #[tauri::command]
    pub fn deep_link_pending(disabled: State<'_, Disabled>) -> Option<Value> {
        std::mem::take(&mut *lock(&disabled.0))
            .then(|| json!({ "state": "disabled", "signed_in": false, "text": TEXT }))
    }

    #[tauri::command]
    pub fn deep_link_done(seq: Option<u64>) {
        let _ = seq;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_foqa_sim_links_are_recognised() {
        assert!(is_link(
            "foqa-sim://challenge?site=https%3A%2F%2Fa.example&id=abc"
        ));
        assert!(is_link("FOQA-SIM://connect"));
        assert!(!is_link("foqa-sim"));
        assert!(!is_link("--minimized"));
        assert!(!is_link("https://foqa-sim://"));
        assert!(!is_link("fóqa-sim://x"));
    }

    #[test]
    fn links_are_taken_from_the_arguments_as_given() {
        let args = [
            "C:\\Program Files\\FOQA-Sim\\foqa-sim.exe",
            "--minimized",
            "foqa-sim://chal\tlenge?site=https://a.example&id=abç",
        ]
        .map(String::from);
        let links: Vec<_> = in_args(args).collect();
        assert_eq!(
            links,
            ["foqa-sim://chal\tlenge?site=https://a.example&id=abç"],
            "not normalised: a tab and a non-ASCII letter reach the parser"
        );
        assert_eq!(in_args(["foqa-sim://connect".to_owned()]).count(), 0);
    }
}
