//! Installation status, like a BITE page: where MSFS 2024 keeps its Community
//! folder, whether the in-sim packages there match the ones the app ships,
//! and one fix per failure. Contract: `docs/debrief-api.md#installation`.

use std::fs;
use std::io;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use ::debrief::RecorderStatus;
use serde::{Deserialize, Serialize};

use crate::commands::lock;

/// The in-sim packages the app ships as resources, `packages/<name>`, with their titles.
pub const PACKAGES: [(&str, &str); 2] = [
    ("foqa-sim-panel", "Toolbar panel"),
    ("foqa-sim-efb", "EFB app"),
];
/// Debug builds: the packages `msfs-panel\assemble.ps1` writes (build.rs runs
/// it), fresher than the resource copy beside the exe after a manual rebuild.
#[cfg(debug_assertions)]
const DEV_PACKAGES: Option<&str> = Some(concat!(
    env!("CARGO_MANIFEST_DIR"),
    r"\..\..\msfs-panel\out\Packages"
));
#[cfg(not(debug_assertions))]
const DEV_PACKAGES: Option<&str> = None;
const MS_STORE_CFG: &str = r"Packages\Microsoft.Limitless_8wekyb3d8bbwe\LocalCache\UserCfg.opt";
const STEAM_CFG: &str = r"Microsoft Flight Simulator 2024\UserCfg.opt";
const SIM_EXE: &str = "FlightSimulator2024.exe";
/// `manufacturer` in our packages' manifest.json (msfs-panel `layout.ps1`,
/// `PackageDefinitions`); the uninstaller checks it too (windows/hooks.nsh).
const MANUFACTURER: &str = "FOQA-Sim";
/// In the app config dir: the Community folder the user chose, if any.
const CHOSEN: &str = "community.txt";
/// In the app config dir: every package folder the app installed; the uninstaller removes them.
const INSTALLED: &str = "community-packages.txt";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const BOM: char = '\u{feff}';

#[derive(Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Store {
    MsStore,
    Steam,
}

impl Store {
    /// Its serialized name, kept in `community.txt` when the user chooses it.
    fn key(self) -> &'static str {
        match self {
            Self::MsStore => "ms_store",
            Self::Steam => "steam",
        }
    }
}

/// How the Community folder was chosen.
#[derive(Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Choice {
    /// The first install found; the user has not chosen.
    Found,
    /// The install the user chose.
    Install,
    /// A folder the user picked.
    Folder,
}

/// An MSFS 2024 install, found by its `UserCfg.opt`.
#[derive(Serialize)]
pub struct Sim {
    pub store: Store,
    pub user_cfg: PathBuf,
    pub community: PathBuf,
}

#[derive(Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Pass,
    Warn,
    Fail,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Install,
    Remove,
    PickCommunity,
}

#[derive(Serialize)]
pub struct Fix {
    action: Action,
    #[serde(skip_serializing_if = "Option::is_none")]
    package: Option<&'static str>,
    label: String,
}

#[derive(Serialize)]
pub struct Check {
    id: String,
    title: String,
    state: State,
    detail: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    paths: Vec<PathBuf>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    files: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    fixes: Vec<Fix>,
}

impl Check {
    fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        state: State,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            state,
            detail: detail.into(),
            paths: Vec::new(),
            files: Vec::new(),
            fixes: Vec::new(),
        }
    }

    fn path(mut self, path: &Path) -> Self {
        self.paths.push(path.to_owned());
        self
    }

    fn fix(
        mut self,
        action: Action,
        package: Option<&'static str>,
        label: impl Into<String>,
    ) -> Self {
        self.fixes.push(Fix {
            action,
            package,
            label: label.into(),
        });
        self
    }
}

#[derive(Serialize)]
pub struct SetupStatus {
    checks: Vec<Check>,
    sims: Vec<Sim>,
    community: Option<PathBuf>,
    choice: Choice,
}

/// `InstalledPackagesPath "D:\MSFS 2024"` → `D:\MSFS 2024`; the quotes are optional.
pub fn installed_packages_path(user_cfg: &str) -> Option<PathBuf> {
    user_cfg.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("InstalledPackagesPath")?;
        let path = rest
            .starts_with(char::is_whitespace)
            .then(|| rest.trim().trim_matches('"'))?;
        (!path.is_empty()).then(|| PathBuf::from(path))
    })
}

/// Every install with a readable `UserCfg.opt`, the MS Store build first. Its
/// packages go to `<InstalledPackagesPath>\Community`, which the sim reads for
/// all community packages; `Community2024` beside it is optional and may be empty.
pub fn locate(local: Option<&Path>, roaming: Option<&Path>) -> Vec<Sim> {
    [
        (Store::MsStore, local.map(|d| d.join(MS_STORE_CFG))),
        (Store::Steam, roaming.map(|d| d.join(STEAM_CFG))),
    ]
    .into_iter()
    .filter_map(|(store, user_cfg)| {
        let user_cfg = user_cfg?;
        let root = installed_packages_path(&String::from_utf8_lossy(&fs::read(&user_cfg).ok()?))?;
        Some(Sim {
            store,
            community: root.join("Community"),
            user_cfg,
        })
    })
    .collect()
}

#[derive(Deserialize)]
struct Layout {
    content: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    path: String,
    size: u64,
}

/// The files of a `layout.json` that `dir` lacks or has with another size,
/// as `"<path> (missing)"` or `"<path> (changed)"`.
pub fn verify(dir: &Path, layout: &str) -> Result<Vec<String>, String> {
    let layout: Layout = serde_json::from_str(layout).map_err(|e| format!("layout.json: {e}"))?;
    Ok(layout
        .content
        .into_iter()
        .filter_map(|e| {
            let what = match fs::metadata(dir.join(&e.path)) {
                Ok(m) if m.len() == e.size => return None,
                Ok(_) => "changed",
                Err(_) => "missing",
            };
            Some(format!("{} ({what})", e.path))
        })
        .collect())
}

#[derive(Deserialize)]
struct Manifest {
    manufacturer: String,
    package_version: String,
}

/// The version of the FOQA-Sim package in `dir`; `None` when its manifest.json
/// is missing or names another manufacturer, e.g. a hand-made copy.
fn ours(dir: &Path) -> Option<String> {
    let manifest: Manifest =
        serde_json::from_slice(&fs::read(dir.join("manifest.json")).ok()?).ok()?;
    (manifest.manufacturer == MANUFACTURER).then_some(manifest.package_version)
}

fn not_ours(dir: &Path) -> String {
    format!(
        "{}: not a FOQA-Sim package (no manifest.json naming {MANUFACTURER}), so it is left alone.",
        dir.display()
    )
}

/// `1.10.0` is newer than `1.9.2`: dotted versions, part by part.
fn older(have: &str, want: &str) -> bool {
    let parts = |v: &str| {
        v.split('.')
            .map(|p| p.parse().unwrap_or(0))
            .collect::<Vec<u64>>()
    };
    parts(have) < parts(want)
}

/// An I/O failure on `path`, in words.
fn fail(path: &Path, e: io::Error) -> String {
    match e.kind() {
        io::ErrorKind::PermissionDenied | io::ErrorKind::ResourceBusy => format!(
            "{}: {e}. The sim may be using its files: close the flight or MSFS 2024 and try again.",
            path.display()
        ),
        _ => format!("{}: {e}", path.display()),
    }
}

fn copy_dir(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// `.<name>.new` and `.<name>.old` beside `target`: an install's copy and the
/// folder it replaces, left behind only when they could not be deleted.
fn staging(target: &Path) -> [PathBuf; 2] {
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    ["new", "old"].map(|suffix| target.with_file_name(format!(".{name}.{suffix}")))
}

/// Deletes what an earlier install left in `staging`, unless it holds someone
/// else's manifest.json; one without any is ours (see `replace`).
fn clear_staging(target: &Path) -> Result<(), String> {
    for dir in staging(target).iter().filter(|d| d.exists()) {
        if dir.join("manifest.json").exists() && ours(dir).is_none() {
            return Err(not_ours(dir));
        }
        fs::remove_dir_all(dir).map_err(|e| fail(dir, e))?;
    }
    Ok(())
}

/// Replaces `target`, if it is ours, with a copy of `source`: copies to
/// `.<name>.new`, moves the old folder aside to `.<name>.old`, renames the copy
/// into place, then deletes the old one. A failure leaves the old folder as it
/// was; nothing else is touched.
pub fn replace(source: &Path, target: &Path) -> Result<(), String> {
    let had = target.exists();
    if had && ours(target).is_none() {
        return Err(not_ours(target));
    }
    clear_staging(target)?;
    let [new, old] = staging(target);
    if let Err(e) = copy_dir(source, &new) {
        let _ = fs::remove_dir_all(&new);
        return Err(fail(target, e));
    }
    if had && let Err(e) = fs::rename(target, &old) {
        let _ = fs::remove_dir_all(&new);
        return Err(fail(target, e));
    }
    if let Err(e) = fs::rename(&new, target) {
        if had {
            let _ = fs::rename(&old, target);
        }
        let _ = fs::remove_dir_all(&new);
        return Err(fail(target, e));
    }
    // The sim or an antivirus can hold old files; without its manifest.json MSFS
    // cannot load what is left as a second package, and the package check names it.
    if had && fs::remove_dir_all(&old).is_err() {
        let _ = fs::remove_file(old.join("manifest.json"));
    }
    Ok(())
}

fn utf16(le: &[u8]) -> String {
    let units: Vec<u16> = le
        .as_chunks()
        .0
        .iter()
        .map(|&c| u16::from_le_bytes(c))
        .collect();
    String::from_utf16_lossy(&units)
}

/// Adds `package` to the list file, or drops it; one absolute path per line, no
/// duplicates. UTF-16LE with a BOM, which the uninstaller reads (windows/hooks.nsh).
pub fn record(list: &Path, package: &Path, installed: bool) -> io::Result<()> {
    let text = match fs::read(list) {
        Ok(bytes) => utf16(&bytes),
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    let package = package.to_string_lossy();
    let mut lines: Vec<&str> = text
        .trim_start_matches(BOM)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.eq_ignore_ascii_case(&package))
        .collect();
    if installed {
        lines.push(&package);
    }
    if let Some(dir) = list.parent() {
        fs::create_dir_all(dir)?;
    }
    let text = lines.iter().fold(BOM.to_string(), |t, l| t + l + "\r\n");
    fs::write(
        list,
        text.encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
}

/// Whether `dir` takes a new file.
fn writable(dir: &Path) -> io::Result<()> {
    let probe = dir.join(".foqa-sim-write-test");
    fs::write(&probe, b"")?;
    fs::remove_file(probe)
}

fn sim_running() -> bool {
    Command::new("tasklist")
        .args(["/FI", &format!("IMAGENAME eq {SIM_EXE}"), "/NH"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(SIM_EXE))
}

fn package_name(name: &str) -> Result<(&'static str, &'static str), String> {
    PACKAGES
        .into_iter()
        .find(|(n, _)| *n == name)
        .ok_or_else(|| format!("no package {name:?}"))
}

/// The installation checks and their fixes.
pub struct Setup {
    /// `%APPDATA%\com.egebilge.foqa-sim`.
    config: PathBuf,
    resources: Option<PathBuf>,
    /// One fix at a time.
    busy: Mutex<()>,
}

impl Setup {
    pub fn new(config: PathBuf, resources: Option<PathBuf>) -> Self {
        Self {
            config,
            resources,
            busy: Mutex::new(()),
        }
    }

    fn sims() -> Vec<Sim> {
        let dir = |var| std::env::var_os(var).map(PathBuf::from);
        locate(dir("LOCALAPPDATA").as_deref(), dir("APPDATA").as_deref())
    }

    /// The install or folder the user chose, else the first install's.
    fn community(&self, sims: &[Sim]) -> Option<(PathBuf, Choice)> {
        let chosen = fs::read_to_string(self.config.join(CHOSEN)).unwrap_or_default();
        let chosen = chosen.trim();
        if let Some(sim) = sims.iter().find(|s| s.store.key() == chosen) {
            Some((sim.community.clone(), Choice::Install))
        } else if Path::new(chosen).is_absolute() {
            Some((chosen.into(), Choice::Folder))
        } else {
            sims.first().map(|s| (s.community.clone(), Choice::Found))
        }
    }

    /// The package the app ships.
    fn bundled(&self, name: &str) -> Result<PathBuf, String> {
        let resource = self.resources.as_ref().map(|d| d.join("packages"));
        [DEV_PACKAGES.map(PathBuf::from), resource]
            .into_iter()
            .flatten()
            .map(|d| d.join(name))
            .find(|d| d.join("layout.json").is_file())
            .ok_or_else(|| match DEV_PACKAGES {
                Some(_) => format!("No built {name}: run msfs-panel\\assemble.ps1."),
                None => {
                    format!("This copy of FOQA-Sim has no {name} to install; reinstall the app.")
                }
            })
    }

    pub fn status(&self, recorder: &RecorderStatus, flights: &Path) -> SetupStatus {
        let sims = Self::sims();
        let community = self.community(&sims);
        let mut checks = vec![Self::sim_check(&sims, community.as_ref())];
        if let Some((dir, _)) = &community {
            let writable = writable(dir);
            let check = Check::new("community", "Community folder", State::Pass, "Exists and is writable. Packages go here, not to Community2024: MSFS 2024 loads both.").path(dir);
            let named = dir.file_name().is_some_and(|n| {
                ["Community", "Community2024"]
                    .iter()
                    .any(|c| n.eq_ignore_ascii_case(c))
            });
            checks.push(match &writable {
                Ok(()) if named => check,
                Ok(()) => Check {
                    state: State::Warn,
                    detail: "Writable, but not named Community: MSFS 2024 loads packages only from its Community folder.".into(),
                    ..check
                }
                .fix(Action::PickCommunity, None, "Choose folder"),
                Err(e) => Check {
                    state: State::Fail,
                    detail: match e.kind() {
                        io::ErrorKind::NotFound => {
                            "The folder does not exist. Start MSFS 2024 once, or choose the folder."
                                .into()
                        }
                        _ => format!("Not writable: {e}"),
                    },
                    ..check
                }
                .fix(Action::PickCommunity, None, "Choose folder"),
            });
            if writable.is_ok() {
                checks.extend(PACKAGES.map(|(name, title)| self.package_check(dir, name, title)));
            }
        }
        checks.push(
            match writable(flights) {
                Ok(()) => {
                    let n = ::debrief::storage_usage(flights).flights;
                    Check::new(
                        "flights",
                        "Flights folder",
                        State::Pass,
                        format!(
                            "Writable, {n} flight{} recorded.",
                            if n == 1 { "" } else { "s" }
                        ),
                    )
                }
                Err(e) => Check::new(
                    "flights",
                    "Flights folder",
                    State::Fail,
                    format!("Not writable: {e}"),
                ),
            }
            .path(flights),
        );
        let running = sim_running();
        checks.push(recorder_check(recorder, running));
        checks.push(if running {
            Check::new(
                "sim_running",
                "MSFS 2024 running",
                State::Pass,
                "Installing can fail while it uses the package files; then close the flight or the sim and try again. Changes surely load the next time it starts.",
            )
        } else {
            Check::new(
                "sim_running",
                "MSFS 2024 not running",
                State::Pass,
                "Installed packages load the next time it starts.",
            )
        });
        SetupStatus {
            checks,
            choice: community.as_ref().map_or(Choice::Found, |(_, c)| *c),
            community: community.map(|(dir, _)| dir),
            sims,
        }
    }

    fn sim_check(sims: &[Sim], community: Option<&(PathBuf, Choice)>) -> Check {
        let store = |s: &Sim| match s.store {
            Store::MsStore => "MS Store build",
            Store::Steam => "Steam build",
        };
        let mut check = match (sims, community.map(|(_, c)| *c)) {
            (_, Some(Choice::Folder)) => Check::new(
                "sim",
                "MSFS 2024",
                State::Pass,
                "Using the Community folder you chose.",
            ),
            ([], _) => {
                let check = Check::new("sim", "MSFS 2024 not found", State::Fail, "No UserCfg.opt where the MS Store or Steam build keeps it. Start MSFS 2024 once, or choose its Community folder.")
                    .fix(Action::PickCommunity, None, "Choose folder");
                let dir = |var, cfg| std::env::var_os(var).map(|d| Path::new(&d).join(cfg));
                return [dir("LOCALAPPDATA", MS_STORE_CFG), dir("APPDATA", STEAM_CFG)]
                    .into_iter()
                    .flatten()
                    .fold(check, |c, p| c.path(&p));
            }
            ([sim], _) => Check::new(
                "sim",
                "MSFS 2024 found",
                State::Pass,
                format!(
                    "{}; Community folder from InstalledPackagesPath in UserCfg.opt.",
                    store(sim)
                ),
            ),
            (_, Some(Choice::Install)) => Check::new(
                "sim",
                "MSFS 2024 found",
                State::Pass,
                "Both the MS Store and the Steam build are installed; the packages go to the one you chose.",
            ),
            ([first, ..], _) => Check::new(
                "sim",
                "MSFS 2024 found",
                State::Warn,
                format!(
                    "Both the MS Store and the Steam build are installed. Choose which one gets the packages; until then the {}'s.",
                    store(first)
                ),
            ),
        };
        for sim in sims {
            check = check.path(&sim.user_cfg);
        }
        check
    }

    fn package_check(&self, community: &Path, name: &'static str, title: &str) -> Check {
        let target = community.join(name);
        let bundled = self.bundled(name);
        let installed = target.exists();
        let have = ours(&target);
        let (mut state, mut detail, files, mut fix) = match &have {
            None if installed => (
                State::Fail,
                format!(
                    "Not a FOQA-Sim package: no manifest.json naming {MANUFACTURER}. It is left alone; move it out of the Community folder to install this one."
                ),
                Vec::new(),
                None,
            ),
            None => (
                State::Fail,
                "Not installed.".into(),
                Vec::new(),
                Some("Install".to_owned()),
            ),
            Some(have) => {
                let want = bundled.as_ref().ok().and_then(|b| ours(b));
                // Against the shipped layout, so a changed build shows; else the package's own.
                let layout = bundled.as_ref().unwrap_or(&target).join("layout.json");
                let files = fs::read_to_string(layout)
                    .map_err(|e| format!("layout.json: {e}"))
                    .and_then(|l| verify(&target, &l));
                match (files, want) {
                    (_, Some(want)) if older(have, &want) => (
                        State::Warn,
                        format!("Version {have} installed; the app ships {want}."),
                        Vec::new(),
                        Some(format!("Update to {want}")),
                    ),
                    (_, Some(want)) if want != *have => (
                        State::Warn,
                        format!(
                            "Version {have} installed, newer than this app's {want}: update FOQA-Sim."
                        ),
                        Vec::new(),
                        None,
                    ),
                    (Err(e), _) => (State::Fail, e, Vec::new(), Some("Repair".into())),
                    (Ok(files), _) if !files.is_empty() => {
                        let n = files.len();
                        (
                            State::Fail,
                            format!(
                                "{n} file{} missing or changed.",
                                if n == 1 { "" } else { "s" }
                            ),
                            files,
                            Some("Repair".into()),
                        )
                    }
                    (Ok(_), _) => (
                        State::Pass,
                        format!("Version {have}, every file matches."),
                        Vec::new(),
                        None,
                    ),
                }
            }
        };
        let left: Vec<PathBuf> = staging(&target)
            .into_iter()
            .filter(|d| d.exists())
            .collect();
        if !left.is_empty() {
            detail += " A copy an earlier install could not delete (files in use) is left beside it; MSFS skips it. Close MSFS 2024, then Repair to delete it.";
            if state == State::Pass {
                state = State::Warn;
                fix = Some("Repair".into());
            }
        }
        let mut check = Check::new(
            format!("package:{name}"),
            format!("{title} ({name})"),
            state,
            detail,
        )
        .path(&target);
        check.paths.extend(left);
        check.files = files;
        if let Some(label) = fix {
            match &bundled {
                Ok(_) => check = check.fix(Action::Install, Some(name), label),
                Err(e) => check.detail = format!("{} {e}", check.detail),
            }
        }
        if have.is_some() {
            check = check.fix(Action::Remove, Some(name), "Remove");
        }
        check
    }

    /// Installs, updates or repairs a package in the Community folder.
    pub fn install(&self, name: &str) -> Result<(), String> {
        let (name, _) = package_name(name)?;
        let _busy = lock(&self.busy);
        let source = self.bundled(name)?;
        let target = self.target(name)?;
        replace(&source, &target)?;
        record(&self.config.join(INSTALLED), &target, true).map_err(|e| format!("{INSTALLED}: {e}"))
    }

    /// Removes a package, and what an earlier install left beside it; only ours.
    pub fn remove(&self, name: &str) -> Result<(), String> {
        let (name, _) = package_name(name)?;
        let _busy = lock(&self.busy);
        let target = self.target(name)?;
        if target.exists() {
            if ours(&target).is_none() {
                return Err(not_ours(&target));
            }
            fs::remove_dir_all(&target).map_err(|e| fail(&target, e))?;
        }
        clear_staging(&target)?;
        record(&self.config.join(INSTALLED), &target, false)
            .map_err(|e| format!("{INSTALLED}: {e}"))
    }

    fn target(&self, name: &str) -> Result<PathBuf, String> {
        let (dir, _) = self
            .community(&Self::sims())
            .ok_or("No Community folder: choose one first.")?;
        Ok(dir.join(name))
    }

    /// Uses `dir` as the Community folder from now on: a found install's is kept
    /// as that install, so it follows its InstalledPackagesPath. `None` goes back
    /// to the first install found.
    pub fn use_community(&self, dir: Option<&Path>) -> Result<(), String> {
        let file = self.config.join(CHOSEN);
        let done = match dir {
            Some(dir) if dir.parent().is_none() => {
                return Err(format!(
                    "{} is a drive root, not a Community folder.",
                    dir.display()
                ));
            }
            Some(dir) => {
                let chosen: String = Self::sims()
                    .into_iter()
                    .find(|s| same(&s.community, dir))
                    .map_or_else(|| dir.to_string_lossy().into(), |s| s.store.key().into());
                fs::create_dir_all(&self.config).and_then(|()| fs::write(&file, chosen))
            }
            None => fs::remove_file(&file).or_else(|e| {
                if e.kind() == io::ErrorKind::NotFound {
                    Ok(())
                } else {
                    Err(e)
                }
            }),
        };
        done.map_err(|e| format!("{CHOSEN}: {e}"))
    }
}

/// The same folder, ignoring case as Windows does.
fn same(a: &Path, b: &Path) -> bool {
    let key = |p: &Path| {
        p.components()
            .map(|c| c.as_os_str().to_ascii_lowercase())
            .collect::<Vec<_>>()
    };
    key(a) == key(b)
}

fn recorder_check(recorder: &RecorderStatus, running: bool) -> Check {
    let (state, detail) = match recorder {
        RecorderStatus::Connected => (State::Pass, "Connected to MSFS 2024.".to_owned()),
        RecorderStatus::Recording { .. } => (State::Pass, "Recording this flight.".to_owned()),
        RecorderStatus::WaitingForSim if running => (State::Warn, "MSFS 2024 is running but has not accepted the connection yet; it does once the sim reaches the main menu.".to_owned()),
        RecorderStatus::WaitingForSim => (State::Pass, "Waiting for MSFS 2024; connects on its own when it starts.".to_owned()),
        RecorderStatus::Error { message } => (State::Fail, message.clone()),
    };
    Check::new("recorder", "Recorder", state, detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh, empty folder under the system temp dir.
    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("foqa-sim-setup-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn reads_installed_packages_path() {
        let cfg =
            |line: &str| installed_packages_path(&format!("Version 1\r\n{line}\r\n[Graphics]\n"));
        assert_eq!(
            cfg(
                r#"InstalledPackagesPath "C:\Users\me\AppData\Local\Packages\x\LocalCache\Packages""#
            ),
            Some(r"C:\Users\me\AppData\Local\Packages\x\LocalCache\Packages".into())
        );
        assert_eq!(
            cfg(r#"  InstalledPackagesPath   "D:\My Games\MSFS 2024"  "#),
            Some(r"D:\My Games\MSFS 2024".into())
        );
        assert_eq!(
            cfg(r"InstalledPackagesPath E:\MSFS"),
            Some(r"E:\MSFS".into())
        );
        assert_eq!(cfg(r#"InstalledPackagesPathX "C:\x""#), None);
        assert_eq!(cfg(r#"InstalledPackagesPath """#), None);
        assert_eq!(installed_packages_path("Version 1\n[Graphics]\n"), None);
    }

    #[test]
    fn locates_both_stores() {
        let root = temp("locate");
        let (local, roaming) = (root.join("local"), root.join("roaming"));
        for (cfg, packages) in [
            (local.join(MS_STORE_CFG), r"C:\MS"),
            (roaming.join(STEAM_CFG), r"D:\Steam Packages"),
        ] {
            fs::create_dir_all(cfg.parent().unwrap()).unwrap();
            fs::write(&cfg, format!("InstalledPackagesPath \"{packages}\"\n")).unwrap();
        }
        let sims = locate(Some(&local), Some(&roaming));
        let found: Vec<_> = sims
            .iter()
            .map(|s| (s.store, s.community.clone()))
            .collect();
        assert!(
            found
                == [
                    (Store::MsStore, PathBuf::from(r"C:\MS\Community")),
                    (Store::Steam, PathBuf::from(r"D:\Steam Packages\Community"))
                ]
        );
        assert!(locate(Some(&root.join("none")), None).is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn verifies_layout_sizes() {
        let dir = temp("verify");
        fs::create_dir_all(dir.join("html_ui/x")).unwrap();
        fs::write(dir.join("html_ui/x/a.js"), "12345").unwrap();
        fs::write(dir.join("b.css"), "1").unwrap();
        let layout = r#"{"content":[{"path":"html_ui/x/a.js","size":5,"date":1},{"path":"b.css","size":2,"date":1},{"path":"c.spb","size":3,"date":1}]}"#;
        assert_eq!(
            verify(&dir, layout).unwrap(),
            ["b.css (changed)", "c.spb (missing)"]
        );
        assert!(verify(&dir, "{}").is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    fn manifest(dir: &Path, manufacturer: &str) {
        fs::create_dir_all(dir).unwrap();
        let json = format!(r#"{{"manufacturer":  "{manufacturer}", "package_version": "0.1.0"}}"#);
        fs::write(dir.join("manifest.json"), json).unwrap();
    }

    #[test]
    fn replaces_only_its_folder() {
        let root = temp("replace");
        let source = root.join("bundled");
        manifest(&source, MANUFACTURER);
        fs::create_dir_all(source.join("html_ui")).unwrap();
        fs::write(source.join("html_ui/new.js"), "new").unwrap();
        let community = root.join("Community");
        let (target, other) = (
            community.join("foqa-sim-panel"),
            community.join("someone-else"),
        );
        manifest(&target, MANUFACTURER);
        fs::create_dir_all(&other).unwrap();
        fs::write(target.join("stale.js"), "old").unwrap();
        fs::write(other.join("keep.js"), "keep").unwrap();
        // An old copy `replace` could not delete, stripped of its manifest.
        let [_, old] = staging(&target);
        fs::create_dir_all(&old).unwrap();

        replace(&source, &target).unwrap();
        assert_eq!(
            fs::read_to_string(target.join("html_ui/new.js")).unwrap(),
            "new"
        );
        assert!(!target.join("stale.js").exists());
        assert!(other.join("keep.js").exists());
        let mut left: Vec<_> = fs::read_dir(&community)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        left.sort();
        assert_eq!(left, ["foqa-sim-panel", "someone-else"]);

        // A failed copy leaves the installed folder as it was.
        assert!(replace(&root.join("nowhere"), &target).is_err());
        assert!(target.join("html_ui/new.js").exists());
        assert!(!community.join(".foqa-sim-panel.new").exists());

        // Someone else's folder or leftover is refused and kept.
        manifest(&old, "Someone");
        assert!(
            replace(&source, &target)
                .unwrap_err()
                .contains("not a FOQA-Sim package")
        );
        assert!(old.join("manifest.json").exists());
        fs::remove_dir_all(&old).unwrap();
        manifest(&target, "Someone");
        assert!(replace(&source, &target).is_err());
        assert!(
            fs::read_to_string(target.join("manifest.json"))
                .unwrap()
                .contains("Someone")
        );
        fs::remove_file(target.join("manifest.json")).unwrap();
        assert!(replace(&source, &target).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn compares_versions() {
        assert!(older("0.1.0", "0.2.0"));
        assert!(older("1.9.2", "1.10.0"));
        assert!(!older("1.10.0", "1.9.2"));
        assert!(!older("0.1.0", "0.1.0"));
    }

    #[test]
    fn keeps_the_community_choice() {
        let root = temp("choice");
        let setup = Setup::new(root.clone(), None);
        let sims = [Store::MsStore, Store::Steam].map(|store| Sim {
            store,
            user_cfg: PathBuf::new(),
            community: root.join(store.key()).join("Community"),
        });
        let is =
            |sims: &[Sim], dir: &Path, choice| setup.community(sims) == Some((dir.into(), choice));
        assert!(is(&sims, &sims[0].community, Choice::Found));
        fs::write(root.join(CHOSEN), "steam").unwrap();
        assert!(is(&sims, &sims[1].community, Choice::Install));
        assert!(is(&sims[..1], &sims[0].community, Choice::Found));
        fs::write(root.join(CHOSEN), r"D:\Mods\Community").unwrap();
        assert!(is(&sims, Path::new(r"D:\Mods\Community"), Choice::Folder));
        assert!(setup.use_community(Some(Path::new(r"D:\"))).is_err());
        assert!(same(
            Path::new(r"C:\MSFS\Community\"),
            Path::new(r"c:\msfs\COMMUNITY")
        ));
        assert!(!same(
            Path::new(r"C:\MSFS\Community"),
            Path::new(r"C:\MSFS\Community2024")
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn records_installed_folders() {
        let root = temp("record");
        let list = root.join("config").join(INSTALLED);
        let read = || {
            let bytes = fs::read(&list).unwrap();
            assert_eq!(bytes[..2], [0xFF, 0xFE]);
            utf16(&bytes[2..])
        };
        let (panel, efb) = (
            Path::new(r"C:\C\Community\foqa-sim-panel"),
            Path::new(r"D:\Uçuş\Community\foqa-sim-efb"),
        );
        record(&list, panel, true).unwrap();
        record(&list, efb, true).unwrap();
        record(&list, Path::new(r"c:\c\community\FOQA-SIM-PANEL"), true).unwrap();
        assert_eq!(
            read(),
            "D:\\Uçuş\\Community\\foqa-sim-efb\r\nc:\\c\\community\\FOQA-SIM-PANEL\r\n"
        );
        record(&list, efb, false).unwrap();
        assert_eq!(read(), "c:\\c\\community\\FOQA-SIM-PANEL\r\n");
        fs::remove_dir_all(root).unwrap();
    }
}
