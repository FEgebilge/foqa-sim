//! Saving a flight for other tools, the flight list as a logbook and the
//! landing card image (docs/exports.md): each asks where in a save dialog
//! and writes there, nothing else. `None` when the dialog is cancelled.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use ::debrief::export::{self, Format, Options};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

use crate::commands::{Shared, blocking};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";

/// The file the user picks, `name` suggested, with one filter.
fn save_as(
    app: &AppHandle,
    title: &str,
    name: &str,
    kind: &str,
    ext: &str,
) -> Result<Option<PathBuf>, String> {
    let picked = app
        .dialog()
        .file()
        .set_title(title)
        .set_file_name(name)
        .add_filter(kind, &[ext])
        .blocking_save_file();
    picked
        .map(|p| p.into_path().map_err(|e| e.to_string()))
        .transpose()
}

fn kind(format: Format) -> &'static str {
    match format {
        Format::Parquet => "Parquet",
        Format::Csv => "CSV",
        Format::Kml => "KML (Google Earth)",
        Format::Gpx => "GPX track",
    }
}

/// Saves flight `id` as `format` (`parquet`, `csv`, `kml`, `gpx`); the
/// first file written.
#[tauri::command]
pub async fn export_flight(
    app: AppHandle,
    id: String,
    format: String,
    identity: bool,
    shared: State<'_, Arc<Shared>>,
) -> Result<Option<String>, String> {
    let shared = Arc::clone(&shared);
    blocking(move || {
        let format =
            Format::parse(&format).ok_or_else(|| format!("no export format {format:?}"))?;
        let path = shared.flight(&id)?;
        let name = format!("{id}.{}", format.ext());
        let Some(out) = save_as(&app, "Export the flight", &name, kind(format), format.ext())?
        else {
            return Ok(None);
        };
        // The dialog confirmed `out` only; Parquet writes a second file.
        if let Some(extra) = export::existing_extra(format, &out) {
            let replace = app
                .dialog()
                .message(format!(
                    "{} already exists. Replace it too?",
                    extra.display()
                ))
                .title("Export the flight")
                .kind(MessageDialogKind::Warning)
                .buttons(MessageDialogButtons::OkCancelCustom(
                    "Replace".into(),
                    "Cancel".into(),
                ))
                .blocking_show();
            if !replace {
                return Ok(None);
            }
        }
        let options = Options { identity };
        let fail = |e: ::debrief::Error| format!("{id}: {e}");
        match format {
            Format::Parquet => export::parquet(
                &::debrief::load(&path).map_err(|e| fail(e.into()))?,
                options,
                &out,
            ),
            _ => shared
                .station
                .open(&path)
                .map_err(fail)?
                .export(format, options, &out),
        }
        .map_err(fail)?;
        Ok(Some(out.display().to_string()))
    })
    .await
}

/// Saves the flight list as a Little Navmap logbook CSV.
#[tauri::command]
pub async fn export_logbook(
    app: AppHandle,
    identity: bool,
    shared: State<'_, Arc<Shared>>,
) -> Result<Option<String>, String> {
    let shared = Arc::clone(&shared);
    blocking(move || {
        let Some(out) = save_as(
            &app,
            "Export a Little Navmap logbook",
            "foqa-sim-logbook.csv",
            "CSV",
            "csv",
        )?
        else {
            return Ok(None);
        };
        export::logbook_file(&shared.summaries(), Options { identity }, &out)
            .map_err(|e| e.to_string())?;
        Ok(Some(out.display().to_string()))
    })
    .await
}

/// Saves a PNG the window drew (the landing card) as `name`.
#[tauri::command]
pub async fn save_image(
    app: AppHandle,
    name: String,
    png: Vec<u8>,
) -> Result<Option<String>, String> {
    if !png.starts_with(PNG) {
        return Err("not a PNG image".into());
    }
    blocking(move || {
        let Some(out) = save_as(&app, "Save the landing card", &name, "PNG image", "png")? else {
            return Ok(None);
        };
        fs::write(&out, png).map_err(|e| e.to_string())?;
        Ok(Some(out.display().to_string()))
    })
    .await
}
