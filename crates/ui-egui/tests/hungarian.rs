//! The Hungarian UI, in its own test binary: the UI language is process-wide, so it can't share one
//! with the English UI tests. `KEEP_LAUNCHER_SNAPSHOTS=<dir>` also saves screenshots (run with
//! `-- --test-threads=1`, see `ui.rs`).

use std::cell::Cell;
use std::rc::Rc;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use keep_launcher_core::protocol::{Backend, Capabilities, Command, Event, JobPhase, LatestRelease, Origin, Source};
use keep_launcher_core::release::{Asset, Release};
use keep_launcher_core::status::InstallSource;
use keep_launcher_core::{Arch, Installed, Os, Platform, fixtures::TEST_SUITE as SUITE, version};
use keep_launcher_ui_egui::settings::ViewMode;
use keep_launcher_ui_egui::{Host, LauncherUi, Settings};

const NOW: i64 = 1_791_500_000;

struct Fake {
    events: Vec<Event>,
    clock: Rc<Cell<i64>>,
    os: Os,
}

impl Backend for Fake {
    fn send(&mut self, _command: Command) {}
    fn poll(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities { manage_apps: true, reveal_label: "Show in Finder", self_update: true }
    }
    fn platform(&self) -> Option<Platform> {
        Some(Platform { os: self.os, arch: Arch::Arm64 })
    }
    fn now(&self) -> i64 {
        self.clock.get()
    }
}

struct DesktopHost;
impl Host for DesktopHost {
    fn has_tray(&self) -> bool {
        true
    }
    fn login_item(&self) -> Option<bool> {
        Some(false)
    }
}

fn release(id: &str, v: &str, at: i64) -> Release {
    let name = format!("{id}-{v}-macos-universal.dmg");
    Release {
        tag: format!("v{v}"),
        version: version::parse(v),
        name: format!("v{v}"),
        published_at: Some(at),
        html_url: format!("https://github.com/moneykeepproject/{id}/releases/tag/v{v}"),
        notes: String::new(),
        prerelease: false,
        assets: vec![Asset {
            name: name.clone(),
            size: 150 << 20,
            url: format!("https://github.com/moneykeepproject/{id}/releases/download/v{v}/{name}"),
            sha256: None,
        }],
    }
}

fn events() -> Vec<Event> {
    let mut ev = Vec::new();
    for (i, app) in SUITE.iter().enumerate() {
        let latest = LatestRelease::Found(release(app.id, "0.5.0", NOW - 40 * 86_400 - i as i64 * 600));
        ev.push(Event::Latest { app: app.id.into(), latest, from: Origin::MoneyKeepApi });
        let installed = matches!(app.id, "moneykeep" | "savekeep").then(|| Installed {
            version: version::parse(if app.id == "moneykeep" { "0.4.1" } else { "0.5.0" }),
            source: InstallSource::MacApp,
            location: format!("/Applications/{}.app", app.name),
            package: None,
            externally_managed: false,
            origin: None,
            installation: None,
        });
        ev.push(Event::Detected { app: app.id.into(), installed });
    }
    ev.push(Event::Progress { app: "billkeep".into(), phase: JobPhase::Verifying });
    ev.push(Event::CheckFinished { at: NOW - 180, source: Source::Feed, rate_limited_until: None });
    // A backend message, translated through its `@msg` template where it's shown.
    ev.push(Event::JobFinished { app: "savekeep".into(), result: Ok("SaveKeep updated to 0.5.0".into()) });
    ev
}

fn harness(view: ViewMode) -> Harness<'static, Option<LauncherUi>> {
    harness_on(view, Os::MacOs)
}

fn harness_on(view: ViewMode, os: Os) -> Harness<'static, Option<LauncherUi>> {
    keep_launcher_ui_egui::i18n::set_test_lang(keep_launcher_ui_egui::i18n::Lang::EN);
    let mut backend = Some(Fake { events: events(), clock: Rc::new(Cell::new(NOW)), os });
    let settings =
        Settings { view, notify: false, language: "hu".into(), theme: keep_launcher_ui_egui::theme::ThemeChoice::Dark, ..Settings::default() };
    Harness::builder().with_size(egui::vec2(1180.0, 900.0)).with_theme(egui::Theme::Dark).build_ui_state(
        move |ui, state: &mut Option<LauncherUi>| {
            if state.is_none()
                && let Some(b) = backend.take()
            {
                *state = Some(ui_new(ui.ctx(), Box::new(b), settings.clone()));
            }
            if let Some(l) = state.as_mut() {
                l.logic(&ui.ctx().clone(), &mut DesktopHost);
                l.ui(ui, &mut DesktopHost);
            }
        },
        None,
    )
}

fn snapshot(h: &mut Harness<'_, Option<LauncherUi>>, name: &str) {
    let Some(dir) = std::env::var_os("KEEP_LAUNCHER_SNAPSHOTS") else { return };
    let img = h.render().unwrap();
    img.save(std::path::Path::new(&dir).join(format!("{name}.png"))).unwrap();
}

// One test: the UI language is process-wide.
#[test]
fn hungarian_ui_translates_labels_dates_and_messages() {
    let mut h = harness(ViewMode::Grid);
    h.run_steps(8);
    snapshot(&mut h, "grid-hu-dark");
    // Product names stay in English.
    assert!(h.query_by_label("Launcher").is_none());
    h.get_by_label("MoneyKeep Apps");
    h.get_by_label("Könyvelés háztartásoknak és kisvállalkozásoknak");
    h.get_by_label("Frissítés: 0.5.0");
    h.get_by_label_contains("Ellenőrzőösszeg ellenőrzése…");
    h.get_by_label_contains("Ellenőrizve: 3 perce · MoneyKeep API");
    h.get_by_label("SaveKeep frissítve. Verzió: 0.5.0");

    let mut table = harness(ViewMode::Table);
    table.run_steps(8);
    snapshot(&mut table, "table-hu-dark");
    table.get_by_label("Verzió");
    assert!(table.query_all_by_label_contains("2026. aug.").count() >= 1);
    check_settings_layout();
}

/// Checks the actual modal after resizing it while open, including Linux's extra controls.
fn check_settings_layout() {
    let mut h = harness_on(ViewMode::Grid, Os::Linux);
    h.run_steps(8);
    h.get_by_label("Beállítások").click();
    h.run_steps(8);
    for (width, height) in [(1180.0, 900.0), (720.0, 500.0), (480.0, 500.0), (1180.0, 900.0)] {
        h.set_size(egui::vec2(width, height));
        h.run_steps(8);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height));
        let done = h.get_by_label("Kész").rect();
        let quit = h.get_by_label("Kilépés az MoneyKeep Launcherből").rect();
        assert!(screen.contains_rect(done), "{width}x{height}: Done is clipped: {done:?}");
        assert!(screen.contains_rect(quit), "{width}x{height}: Quit is clipped: {quit:?}");
        assert!(!done.intersects(quit));
        let title = h.get_all_by_label("Beállítások").last().unwrap().rect();
        assert!(screen.contains_rect(title), "{width}x{height}: title is clipped: {title:?}");
        // The community links and the appearance choices sit in different columns (stacked, one
        // under the other), so each is scrolled to in turn.
        h.get_by_label("Rendszerbeállítás szerint").scroll_to_me();
        h.run_steps(8);
        let theme = h.get_by_label("Rendszerbeállítás szerint").rect();
        assert!(theme.bottom() < done.top(), "appearance controls must scroll above the fixed actions");
        // The community section's GitHub button (34px tall; the footer's GitHub icon is 24px).
        h.get_all_by_label("GitHub").find(|n| n.rect().height() > 30.0).unwrap().scroll_to_me();
        h.run_steps(8);
        let github = h.get_all_by_label("GitHub").find(|n| n.rect().height() > 30.0).unwrap().rect();
        assert!(
            screen.contains_rect(github) && github.bottom() <= done.top(),
            "{width}x{height}: the community link must be reachable by scrolling: {github:?}"
        );
        snapshot(&mut h, &format!("settings-hu-{width}x{height}-bottom"));
        h.get_by_label("Nyelv").scroll_to_me();
        h.run_steps(8);
        let language = h.get_by_label("Nyelv").rect();
        assert!(screen.contains_rect(language), "the top of settings must remain reachable");
        snapshot(&mut h, &format!("settings-hu-{width}x{height}-top"));
    }
    h.get_by_label("Kész").click();
    h.run_steps(4);
    assert!(h.query_by_label("Kész").is_none(), "Done must still close the resized dialog");
}

/// The UI over the test catalog, which has more apps in more states than the MoneyKeep Apps.
fn ui_new(ctx: &egui::Context, backend: Box<dyn keep_launcher_core::protocol::Backend>, settings: keep_launcher_ui_egui::Settings) -> LauncherUi {
    LauncherUi::with_catalog(ctx, backend, settings, &SUITE)
}
