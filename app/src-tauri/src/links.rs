//! Links from websites in a build without the `connect` feature. The
//! installer registers the `foqa-sim` scheme all the same, so a link starts
//! or raises the app: it is recognised, never parsed or acted on, and the
//! window says that sites are disabled in this build.

use std::sync::Mutex;

use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::commands::lock;

/// A link arrived and is not yet told.
#[derive(Default)]
pub struct Disabled(Mutex<bool>);

const TEXT: &str = "Sites are disabled in this build.";

/// A command-line argument that is a `foqa-sim:` link.
pub fn is_link(arg: &str) -> bool {
    arg.get(..9)
        .is_some_and(|s| s.eq_ignore_ascii_case("foqa-sim:"))
}

/// The app was started, or raised by a second start, with `args`.
pub fn arrived(app: &AppHandle, args: impl IntoIterator<Item = String>) {
    if !args.into_iter().skip(1).any(|a| is_link(&a)) {
        return;
    }
    eprintln!("deep link ignored: sites are disabled in this build");
    *lock(&app.state::<Disabled>().0) = true;
    let _ = app.emit("deep-link", view());
    crate::show(app, None);
}

fn view() -> Value {
    json!({ "state": "disabled", "signed_in": false, "text": TEXT })
}

#[tauri::command]
pub fn deep_link_pending(disabled: State<'_, Disabled>) -> Option<Value> {
    std::mem::take(&mut *lock(&disabled.0)).then(view)
}

#[tauri::command]
pub fn deep_link_done(disabled: State<'_, Disabled>) {
    *lock(&disabled.0) = false;
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
}
