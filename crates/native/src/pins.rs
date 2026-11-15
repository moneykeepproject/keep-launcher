//! The shared version-choice file, kept in step with the launcher's settings so an app's own
//! update check can know when the launcher keeps it on another version ([`default_path`]).
//!
//! The file is `{"apps": {"<id>": "Nightly" | {"Pinned": "<tag>"}}}`. Only the launcher writes
//! it; apps read. The format is the one MoneyKeep Launcher inherited from its upstream, so a file
//! from 0.1.0 or 0.1.1 reads the same.
//!
//! Invariants:
//! - A missing, unreadable or damaged file means every app follows stable releases.
//! - The launcher's settings are the source of truth for the apps it lists: an app absent from
//!   them follows stable releases, so a readable entry for it in the file is removed.
//! - An entry this launcher cannot read (written by a newer one) counts as managed by the
//!   launcher, never as stable, and is kept as it is on every save.
//! - Choosing stable removes the app's entry, so the file only lists apps kept on something else.
//! - The file is written only when something changed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use keep_launcher_core::protocol::VersionTrack;
use serde::{Deserialize, Serialize};

/// Which published build an app follows, as the file records it. A pinned version never changes
/// on its own.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SharedTrack {
    #[default]
    Stable,
    Nightly,
    Pinned(String),
}

#[derive(Default, Serialize, Deserialize)]
struct File {
    /// App id to its track, kept as raw values so entries from a newer launcher survive a save.
    #[serde(default)]
    apps: BTreeMap<String, serde_json::Value>,
}

/// The saved version choices.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pins {
    apps: BTreeMap<String, serde_json::Value>,
}

/// What an app's own update check should do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choice {
    /// Follow stable releases: check and offer as usual.
    Stable,
    /// The launcher keeps this app on another track; the app offers nothing itself.
    Managed(SharedTrack),
    /// An entry this version cannot read; also left to the launcher.
    Unknown,
}

impl Pins {
    /// Reads `path`. A missing or damaged file reads as empty: every app follows stable releases.
    pub fn load(path: &Path) -> Self {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            Err(e) => {
                log::warn!("couldn't read {} ({e}); every app follows stable releases", path.display());
                return Self::default();
            }
        };
        match serde_json::from_str::<File>(&text) {
            Ok(file) => Self { apps: file.apps },
            Err(e) => {
                log::warn!("{} is damaged ({e}); every app follows stable releases", path.display());
                Self::default()
            }
        }
    }

    /// Writes `path` in one step, so a reader never sees half a file.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("couldn't create {} ({e})", dir.display()))?;
        }
        let file = File { apps: self.apps.clone() };
        let text = serde_json::to_string_pretty(&file).map_err(|e| format!("couldn't encode the version choices ({e})"))?;
        crate::paths::write_atomic(path, text.as_bytes())
    }

    /// What `app`'s own update check should do.
    pub fn choice(&self, app: &str) -> Choice {
        match self.apps.get(app) {
            None => Choice::Stable,
            Some(raw) => match serde_json::from_value::<SharedTrack>(raw.clone()) {
                Ok(SharedTrack::Stable) => Choice::Stable,
                Ok(track) => Choice::Managed(track),
                Err(_) => Choice::Unknown,
            },
        }
    }

    /// The track `app` follows, when this version can read it.
    pub fn track(&self, app: &str) -> Option<SharedTrack> {
        match self.choice(app) {
            Choice::Stable => Some(SharedTrack::Stable),
            Choice::Managed(track) => Some(track),
            Choice::Unknown => None,
        }
    }

    /// Sets `app`'s track. Stable removes the entry.
    pub fn set(&mut self, app: &str, track: &SharedTrack) {
        match track {
            SharedTrack::Stable => {
                self.apps.remove(app);
            }
            other => {
                if let Ok(raw) = serde_json::to_value(other) {
                    self.apps.insert(app.to_string(), raw);
                }
            }
        }
    }

    /// Every app with a track this version can read, stable ones excluded.
    pub fn tracks(&self) -> impl Iterator<Item = (&str, SharedTrack)> {
        self.apps
            .iter()
            .filter_map(|(id, raw)| serde_json::from_value(raw.clone()).ok().map(|t| (id.as_str(), t)))
            .filter(|(_, track)| *track != SharedTrack::Stable)
    }
}

/// Names a file to use in place of the default (absolute paths only). The launcher passes it to
/// every app it starts, so an app whose home is redirected (tarball installs on Linux) still finds
/// the real file.
pub const PATH_ENV: &str = "KEEP_VERSIONS_FILE";

/// The folder, under the host's per-user data folder, that holds the file. The Flatpak manifests
/// grant exactly this folder (`--filesystem=xdg-data/MoneyKeep/versions:create`); the two change
/// together.
const FOLDER: [&str; 2] = ["MoneyKeep", "versions"];

/// Where MoneyKeep Launcher 0.1.0 and 0.1.1 wrote the file: the upstream launcher's folder, which
/// a copy of that launcher on the same machine also uses. See [`retire_former_file`].
const FORMER_FOLDER: [&str; 2] = ["ArtCraft", "versions"];

const FILE: &str = "versions.json";

/// The shared file: `MoneyKeep/versions/versions.json` in the host's per-user data folder
/// (`%LOCALAPPDATA%`, `~/Library/Application Support`, `$XDG_DATA_HOME` or `~/.local/share`), or
/// the one [`PATH_ENV`] names.
pub fn default_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(PATH_ENV).map(PathBuf::from).filter(|p| p.is_absolute()) {
        return Some(path);
    }
    host_data_local_dir().map(|dir| in_folder(&dir, FOLDER))
}

fn in_folder(data: &Path, folder: [&str; 2]) -> PathBuf {
    data.join(folder[0]).join(folder[1]).join(FILE)
}

/// The host's per-user data folder, also from inside a Flatpak, where `XDG_DATA_HOME` points into
/// the sandbox (`~/.var/app/<id>/data`).
fn host_data_local_dir() -> Option<PathBuf> {
    let absolute = |name: &str| std::env::var_os(name).map(PathBuf::from).filter(|p| p.is_absolute());
    pick_host_data_dir(Path::new("/.flatpak-info").exists(), absolute("HOST_XDG_DATA_HOME"), dirs::home_dir(), dirs::data_local_dir())
}

/// [`host_data_local_dir`] with its inputs passed in, so every CI OS can test the Flatpak case.
fn pick_host_data_dir(in_flatpak: bool, host_xdg: Option<PathBuf>, home: Option<PathBuf>, local: Option<PathBuf>) -> Option<PathBuf> {
    if in_flatpak {
        return host_xdg.or_else(|| home.map(|h| h.join(".local").join("share")));
    }
    local
}

/// Takes MoneyKeep's entries (`ids`) out of the file 0.1.0 and 0.1.1 wrote in the upstream
/// launcher's folder. That file may also belong to a copy of the upstream launcher, so its other
/// entries stay; the file goes only when nothing else is left in it, and its folders only when
/// they are empty. Best effort: a failure is logged and changes nothing else.
pub fn retire_former_file(ids: &[&str]) {
    if let Some(dir) = host_data_local_dir()
        && let Err(e) = retire_in(&dir, ids)
    {
        log::warn!("couldn't tidy the old version-choice file ({e})");
    }
}

fn retire_in(data: &Path, ids: &[&str]) -> Result<(), String> {
    let path = in_folder(data, FORMER_FOLDER);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    // A file this launcher can't read is left alone: it isn't ours to repair.
    let Ok(mut file) = serde_json::from_str::<serde_json::Value>(&text) else { return Ok(()) };
    let Some(apps) = file.get_mut("apps").and_then(serde_json::Value::as_object_mut) else { return Ok(()) };
    let before = apps.len();
    apps.retain(|id, _| !ids.contains(&id.as_str()));
    if apps.len() == before {
        return Ok(());
    }
    if apps.is_empty() && file.as_object().is_some_and(|o| o.len() == 1) {
        std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        // Only empty folders go: `remove_dir` refuses any other.
        let versions = data.join(FORMER_FOLDER[0]).join(FORMER_FOLDER[1]);
        let _ = std::fs::remove_dir(&versions);
        let _ = std::fs::remove_dir(data.join(FORMER_FOLDER[0]));
        log::info!("removed the old version-choice file {}", path.display());
        return Ok(());
    }
    let text = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
    crate::paths::write_atomic(&path, text.as_bytes())?;
    log::info!("took MoneyKeep's entries out of {}", path.display());
    Ok(())
}

fn shared(track: &VersionTrack) -> SharedTrack {
    match track {
        VersionTrack::Stable => SharedTrack::Stable,
        VersionTrack::Nightly => SharedTrack::Nightly,
        VersionTrack::Pinned(tag) => SharedTrack::Pinned(tag.clone()),
    }
}

/// `pins` changed so it says what `versions` says, keeping entries this launcher cannot read.
pub fn merged(mut pins: Pins, versions: &BTreeMap<String, VersionTrack>) -> Pins {
    let stale: Vec<String> = pins.tracks().map(|(id, _)| id.to_string()).filter(|id| !versions.contains_key(id)).collect();
    for id in stale {
        pins.set(&id, &SharedTrack::Stable);
    }
    for (id, track) in versions {
        pins.set(id, &shared(track));
    }
    pins
}

/// Makes the file's folder when the launcher starts, before any app does. Flatpak mounts an app's
/// grant for that folder only if it exists when the app starts, so an app started before the
/// first pin would otherwise never see it until restarted.
pub fn prepare_folder(path: &Path) {
    if let Some(dir) = path.parent()
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        log::warn!("couldn't create {} ({e}); Flatpak apps won't see version choices", dir.display());
    }
}

/// Writes `path` so it matches `versions`. Returns whether the file changed.
pub fn sync(path: &Path, versions: &BTreeMap<String, VersionTrack>) -> Result<bool, String> {
    let before = Pins::load(path);
    let after = merged(before.clone(), versions);
    if after == before {
        return Ok(false);
    }
    after.save(path)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pinned(tag: &str) -> SharedTrack {
        SharedTrack::Pinned(tag.into())
    }

    #[test]
    fn a_missing_or_damaged_file_means_every_app_follows_stable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("versions.json");
        assert_eq!(Pins::load(&path).choice("moneykeep"), Choice::Stable);
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(Pins::load(&path).choice("moneykeep"), Choice::Stable);
        std::fs::write(&path, r#"{"apps":"wrong shape"}"#).unwrap();
        assert_eq!(Pins::load(&path).choice("moneykeep"), Choice::Stable);
    }

    #[test]
    fn choices_survive_a_save_and_only_the_named_app_is_held() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("versions.json");
        let mut pins = Pins::default();
        pins.set("moneykeep", &pinned("v0.1.0"));
        pins.set("calckeep", &SharedTrack::Nightly);
        pins.save(&path).unwrap();
        let read = Pins::load(&path);
        assert_eq!(read, pins);
        assert_eq!(read.choice("moneykeep"), Choice::Managed(pinned("v0.1.0")));
        assert_eq!(read.choice("calckeep"), Choice::Managed(SharedTrack::Nightly));
        assert_eq!(read.choice("groupkeep"), Choice::Stable);
        assert_eq!(read.tracks().count(), 2);
    }

    #[test]
    fn a_file_from_0_1_1_reads_the_same() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("versions.json");
        std::fs::write(&path, r#"{"apps":{"moneykeep":{"Pinned":"v0.1.0"},"calckeep":"Nightly","groupkeep":"Stable"}}"#).unwrap();
        let pins = Pins::load(&path);
        assert_eq!(pins.track("moneykeep"), Some(pinned("v0.1.0")));
        assert_eq!(pins.track("calckeep"), Some(SharedTrack::Nightly));
        assert_eq!(pins.choice("groupkeep"), Choice::Stable);
        let listed: Vec<&str> = pins.tracks().map(|(id, _)| id).collect();
        assert_eq!(listed, ["calckeep", "moneykeep"], "an explicit Stable entry is not listed");
        let mut saved = pins.clone();
        saved.set("form1099keep", &SharedTrack::Nightly);
        saved.save(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains(r#""Pinned": "v0.1.0""#) && text.contains(r#""form1099keep": "Nightly""#), "{text}");
    }

    fn versions(entries: &[(&str, VersionTrack)]) -> BTreeMap<String, VersionTrack> {
        entries.iter().map(|(id, t)| (id.to_string(), t.clone())).collect()
    }

    #[test]
    fn the_file_lives_in_moneykeeps_own_folder() {
        let data = Path::new("/data");
        assert_eq!(in_folder(data, FOLDER), Path::new("/data/MoneyKeep/versions/versions.json"));
        let local = Some(PathBuf::from("/home/a/.local/share"));
        assert_eq!(pick_host_data_dir(false, None, None, local.clone()), local);
        let host = Some(PathBuf::from("/home/a/custom-data"));
        assert_eq!(pick_host_data_dir(true, host.clone(), Some(PathBuf::from("/home/a")), None), host);
        assert_eq!(pick_host_data_dir(true, None, Some(PathBuf::from("/home/a")), None), Some(PathBuf::from("/home/a/.local/share")));
        if std::env::var_os(PATH_ENV).is_none() {
            assert!(default_path().is_none_or(|p| p.ends_with("MoneyKeep/versions/versions.json")));
        }
    }

    #[test]
    fn the_flatpak_manifests_grant_the_folder() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/linux/flatpak");
        let grant = format!("--filesystem=xdg-data/{}/{}:create", FOLDER[0], FOLDER[1]);
        for name in ["com.moneykeep.keep-launcher.yml", "com.moneykeep.keep-launcher.bundle.yml"] {
            let text = std::fs::read_to_string(root.join(name)).unwrap();
            assert!(text.contains(&grant), "{name} doesn't grant {grant}");
        }
    }

    #[test]
    fn the_old_file_loses_only_moneykeeps_entries() {
        let dir = tempfile::tempdir().unwrap();
        let path = in_folder(dir.path(), FORMER_FOLDER);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"apps": {"moneykeep": "Nightly", "photocraft": {"Pinned": "v1.0.0"}}}"#).unwrap();
        retire_in(dir.path(), &["moneykeep", "calckeep"]).unwrap();
        let pins = Pins::load(&path);
        assert_eq!(pins.choice("moneykeep"), Choice::Stable);
        assert_eq!(pins.choice("photocraft"), Choice::Managed(SharedTrack::Pinned("v1.0.0".into())));
        // Nothing of ours left: nothing changes.
        let text = std::fs::read_to_string(&path).unwrap();
        retire_in(dir.path(), &["moneykeep"]).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    }

    #[test]
    fn the_old_file_and_its_empty_folders_go_when_only_moneykeep_used_them() {
        let dir = tempfile::tempdir().unwrap();
        let path = in_folder(dir.path(), FORMER_FOLDER);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"apps": {"moneykeep": "Nightly", "groupkeep": {"Pinned": "v0.1.0"}}}"#).unwrap();
        retire_in(dir.path(), &["moneykeep", "groupkeep"]).unwrap();
        assert!(!path.exists());
        assert!(!dir.path().join(FORMER_FOLDER[0]).exists());

        // Another program's files keep the folder.
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(dir.path().join(FORMER_FOLDER[0]).join("settings.json"), "{}").unwrap();
        std::fs::write(&path, r#"{"apps": {"moneykeep": "Nightly"}}"#).unwrap();
        retire_in(dir.path(), &["moneykeep"]).unwrap();
        assert!(!path.exists());
        assert!(dir.path().join(FORMER_FOLDER[0]).join("settings.json").exists());
    }

    #[test]
    fn an_unreadable_or_missing_old_file_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        retire_in(dir.path(), &["moneykeep"]).unwrap();
        let path = in_folder(dir.path(), FORMER_FOLDER);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "not json").unwrap();
        retire_in(dir.path(), &["moneykeep"]).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "not json");
    }

    #[test]
    fn the_folder_exists_before_any_choice_is_saved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("MoneyKeep").join("versions").join("versions.json");
        prepare_folder(&path);
        assert!(path.parent().unwrap().is_dir());
        assert!(!path.exists(), "no file until there is a choice");
        prepare_folder(&path);
    }

    #[test]
    fn the_file_follows_the_launchers_choices() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("versions.json");
        let chosen = versions(&[("ledgerkeep", VersionTrack::Pinned("v0.13.0".into())), ("billkeep", VersionTrack::Nightly)]);
        assert_eq!(sync(&path, &chosen), Ok(true));
        let pins = Pins::load(&path);
        assert_eq!(pins.choice("ledgerkeep"), Choice::Managed(SharedTrack::Pinned("v0.13.0".into())));
        assert_eq!(pins.choice("billkeep"), Choice::Managed(SharedTrack::Nightly));
        assert_eq!(pins.choice("calckeep"), Choice::Stable);
        assert_eq!(sync(&path, &chosen), Ok(false), "nothing changed, nothing written");
    }

    #[test]
    fn going_back_to_stable_removes_the_entry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("versions.json");
        sync(&path, &versions(&[("ledgerkeep", VersionTrack::Pinned("v0.13.0".into()))])).unwrap();
        // The UI may drop the entry or keep it as Stable; both mean stable.
        assert_eq!(sync(&path, &versions(&[("ledgerkeep", VersionTrack::Stable)])), Ok(true));
        assert_eq!(Pins::load(&path).choice("ledgerkeep"), Choice::Stable);
        sync(&path, &versions(&[("ledgerkeep", VersionTrack::Nightly)])).unwrap();
        assert_eq!(sync(&path, &BTreeMap::new()), Ok(true));
        assert_eq!(Pins::load(&path).choice("ledgerkeep"), Choice::Stable);
    }

    #[test]
    fn an_entry_from_a_newer_launcher_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("versions.json");
        std::fs::write(&path, r#"{"apps":{"paykeep":{"Beta":1}}}"#).unwrap();
        sync(&path, &versions(&[("ledgerkeep", VersionTrack::Nightly)])).unwrap();
        let pins = Pins::load(&path);
        assert_eq!(pins.choice("paykeep"), Choice::Unknown);
        assert_eq!(pins.choice("ledgerkeep"), Choice::Managed(SharedTrack::Nightly));
    }

    #[test]
    fn a_damaged_file_is_replaced_by_the_launchers_choices() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("versions.json");
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(sync(&path, &versions(&[("ledgerkeep", VersionTrack::Nightly)])), Ok(true));
        assert_eq!(Pins::load(&path).choice("ledgerkeep"), Choice::Managed(SharedTrack::Nightly));
    }
}
