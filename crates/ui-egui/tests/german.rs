//! The German UI, in its own test binary: the UI language is process-wide, so it can't share one
//! with the English UI tests. `KEEP_LAUNCHER_SNAPSHOTS=<dir>` also saves screenshots (run with
//! `-- --test-threads=1`).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use keep_launcher_core::protocol::{Backend, Capabilities, Command, Event, JobPhase, LatestRelease, Origin, PausedDownload, Source};
use keep_launcher_core::release::{Asset, Release};
use keep_launcher_core::status::InstallSource;
use keep_launcher_core::{Arch, Installed, Os, Platform, fixtures::TEST_SUITE as SUITE, version};
use keep_launcher_ui_egui::settings::ViewMode;
use keep_launcher_ui_egui::{Host, LauncherUi, Settings};

const NOW: i64 = 1_791_500_000;

struct Fake {
    events: Vec<Event>,
    sent: Rc<RefCell<Vec<Command>>>,
    manage: bool,
    clock: Rc<Cell<i64>>,
}

impl Backend for Fake {
    fn send(&mut self, command: Command) {
        self.sent.borrow_mut().push(command);
    }
    fn poll(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities { manage_apps: self.manage, reveal_label: "Show in Finder", self_update: self.manage }
    }
    fn platform(&self) -> Option<Platform> {
        Some(Platform { os: Os::MacOs, arch: Arch::Arm64 })
    }
    fn now(&self) -> i64 {
        self.clock.get()
    }
}

struct NoHost;
impl Host for NoHost {}

fn release(id: &str, v: &str, at: i64) -> Release {
    let name = format!("{id}-{v}-macos-universal.dmg");
    Release {
        tag: format!("v{v}"),
        version: version::parse(v),
        name: format!("v{v}"),
        published_at: Some(at),
        html_url: format!("https://github.com/moneykeepproject/{id}/releases/tag/v{v}"),
        notes: "## What's Changed\n* Faster things by @a in https://github.com/moneykeepproject/x/pull/1".into(),
        prerelease: false,
        assets: vec![Asset {
            name: name.clone(),
            size: 150 << 20,
            url: format!("https://github.com/moneykeepproject/{id}/releases/download/v{v}/{name}"),
            sha256: None,
        }],
    }
}

/// A realistic mix: updates, up-to-date apps, a download in progress, one app with no release.
fn events() -> Vec<Event> {
    let mut ev = Vec::new();
    let versions = [
        ("moneykeep", "0.5.0"),
        ("budgetkeep", "0.7.0"),
        ("billkeep", "0.4.0"),
        ("savekeep", "0.4.0"),
        ("ledgerkeep", "0.4.0"),
        ("investkeep", "0.6.0"),
        ("invoicekeep", "0.4.0"),
        ("notekeep", "0.3.0"),
        ("reportkeep", "0.3.0"),
        ("calckeep", "0.3.0"),
        ("assetkeep", "0.3.0"),
    ];
    for (i, (id, v)) in versions.iter().enumerate() {
        ev.push(Event::Latest {
            app: (*id).into(),
            latest: LatestRelease::Found(release(id, v, NOW - 86_400 - i as i64 * 600)),
            from: Origin::MoneyKeepApi,
        });
    }
    ev.push(Event::Latest { app: "paykeep".into(), latest: LatestRelease::None, from: Origin::MoneyKeepApi });
    let installed = [("moneykeep", "0.4.1"), ("savekeep", "0.4.0"), ("notekeep", "0.3.0"), ("investkeep", "0.5.0")];
    for app in &SUITE {
        let inst = installed.iter().find(|(id, _)| *id == app.id).map(|(_, v)| Installed {
            version: version::parse(v),
            source: InstallSource::MacApp,
            location: format!("/Applications/{}.app", app.name),
            package: None,
            externally_managed: false,
            origin: None,
            installation: None,
        });
        ev.push(Event::Detected { app: app.id.into(), installed: inst });
    }
    ev.push(Event::Progress { app: "billkeep".into(), phase: JobPhase::Downloading { done: 48 << 20, total: 131 << 20 } });
    ev.push(Event::Paused { app: "reportkeep".into(), download: Some(PausedDownload { done: 30 << 20, total: 120 << 20 }) });
    ev.push(Event::Progress { app: "calckeep".into(), phase: JobPhase::Reconnecting { done: 10 << 20, total: 90 << 20 } });
    ev.push(Event::CheckFinished { at: NOW - 180, source: Source::GitHubApi, rate_limited_until: None });
    ev
}

fn harness_language(
    view: ViewMode,
    theme: egui::Theme,
    manage: bool,
    language: &str,
    extra: Vec<Event>,
) -> (Harness<'static, Option<LauncherUi>>, Rc<RefCell<Vec<Command>>>) {
    keep_launcher_ui_egui::i18n::set_test_lang(keep_launcher_ui_egui::i18n::Lang::EN);
    let sent = Rc::new(RefCell::new(Vec::new()));
    let sent2 = sent.clone();
    let mut events = events();
    events.extend(extra);
    let mut backend = Some(Fake { events, sent: sent2, manage, clock: Rc::new(Cell::new(NOW)) });
    let settings = Settings { view, notify: false, language: language.into(), ..Settings::default() };
    let harness = Harness::builder().with_size(egui::vec2(1180.0, 900.0)).with_theme(theme).build_ui_state(
        move |ui, state: &mut Option<LauncherUi>| {
            if state.is_none()
                && let Some(b) = backend.take()
            {
                let mut s = settings.clone();
                s.theme = if theme == egui::Theme::Dark {
                    keep_launcher_ui_egui::theme::ThemeChoice::Dark
                } else {
                    keep_launcher_ui_egui::theme::ThemeChoice::Light
                };
                *state = Some(ui_new(ui.ctx(), Box::new(b), s));
            }
            if let Some(l) = state.as_mut() {
                l.logic(&ui.ctx().clone(), &mut NoHost);
                l.ui(ui, &mut NoHost);
            }
        },
        None,
    );
    (harness, sent)
}

fn snapshot(h: &mut Harness<'_, Option<LauncherUi>>, name: &str) {
    let Some(dir) = std::env::var_os("KEEP_LAUNCHER_SNAPSHOTS") else { return };
    let img = h.render().unwrap();
    img.save(std::path::Path::new(&dir).join(format!("{name}.png"))).unwrap();
}

// One test: the UI language is process-wide.
#[test]
fn german_switches_persists_and_keeps_install_actions() {
    let (mut h, sent) = harness_language(ViewMode::Grid, egui::Theme::Dark, true, "en", Vec::new());
    h.run_steps(6);
    h.get_by_label("Settings").click();
    h.run_steps(3);
    h.get_by_value("English").click();
    h.run_steps(2);
    h.get_by_label("Deutsch").click();
    h.run_steps(3);
    h.get_by_value("Deutsch");
    h.get_by_label("Benachrichtigen, wenn es für eine installierte App ein Update gibt");
    snapshot(&mut h, "settings-de-dark");
    let settings = h.state().as_ref().unwrap().settings.clone();
    assert_eq!(settings.language, "de");
    let saved = serde_json::to_string(&settings).unwrap();
    let restored: Settings = serde_json::from_str(&saved).unwrap();
    assert_eq!(restored.language, "de");
    h.get_by_label("Fertig").click();
    h.run_steps(3);
    h.get_by_label("Buchhaltung für Haushalte und kleine Unternehmen");
    h.get_by_label_contains("Zuletzt geprüft: vor 3 Min. · über MoneyKeep API");
    h.get_by_label("Auf 0.5.0 aktualisieren");
    // Product names stay in English.
    assert!(h.query_by_label("Launcher").is_none());
    snapshot(&mut h, "grid-de-dark");
    h.get_by_label("Alle aktualisieren (2)").click();
    h.run_steps(2);
    let installs: Vec<_> = sent.borrow().iter().filter_map(|c| if let Command::Install { app } = c { Some(app.clone()) } else { None }).collect();
    assert_eq!(installs, ["moneykeep", "investkeep"]);

    let (mut table, _) = harness_language(ViewMode::Table, egui::Theme::Light, true, &restored.language, Vec::new());
    table.run_steps(6);
    table.get_by_label("Datum");
    assert!(table.query_all_by_label_contains(". Okt. 2026").count() >= 1);
    snapshot(&mut table, "table-de-light");
    // Kept downloads: German retry labels are long, but a button never outgrows its slot (#956).
    let kept = ["moneykeep", "budgetkeep"].map(|app| Event::ReadyToInstall { app: app.into(), ready: true });
    let (mut retry, _) = harness_language(ViewMode::Table, egui::Theme::Light, true, &restored.language, kept.into());
    retry.run_steps(6);
    let slot = retry.query_all_by_label("Installieren").next().unwrap().rect().width();
    for app in ["MoneyKeep", "BudgetKeep"] {
        let menu = retry.get_by_label(&format!("Weitere Aktionen für {app}")).rect();
        // The main button is the one ending just left of the menu (or overlapping it).
        let main = retry
            .query_all_by_role(egui::accesskit::Role::Button)
            .filter(|n| (n.rect().center().y - menu.center().y).abs() < 1.0 && n.rect().left() < menu.left() && n.rect().right() > menu.left() - 20.0)
            .find(|n| n.rect().height() < 40.0)
            .unwrap();
        assert!(main.rect().width() <= slot + 0.5, "{app}: {:?} wider than Installieren ({slot})", main.rect());
    }
    let (mut web, _) = harness_language(ViewMode::Grid, egui::Theme::Dark, false, &restored.language, Vec::new());
    web.run_steps(6);
    assert!(web.query_all_by_label("Für macOS herunterladen").count() >= 3);
    for width in [720.0, 800.0, 1180.0] {
        web.set_size(egui::vec2(width, 900.0));
        web.run_steps(6);
        for label in ["Show apps as cards", "Show apps as a table", "Settings", "Check for updates"] {
            let rect = web.get_by_label(keep_launcher_ui_egui::i18n::t(label)).rect();
            assert!(rect.left() >= 0.0 && rect.right() <= width, "{width}: {label} {rect:?}");
            if let Some(text) = web.query_by_label("MoneyKeep Apps") {
                assert!(!rect.intersects(text.rect()), "{width}: MoneyKeep Apps overlaps {label} {rect:?}");
            }
        }
    }
    for language in &keep_launcher_ui_egui::i18n::LANGUAGES {
        main_buttons_fit_the_narrowest_window(language.code);
    }
}

/// At the launcher's narrowest, no main button outgrows its slot or crowds out the `⋯` menu (#956):
/// a kept update, a kept install and a plain update, in both views, whichever label `language` has.
fn main_buttons_fit_the_narrowest_window(language: &str) {
    use keep_launcher_ui_egui::i18n::{fmt, t};
    for view in [ViewMode::Table, ViewMode::Grid] {
        let kept = ["moneykeep", "budgetkeep"].map(|app| Event::ReadyToInstall { app: app.into(), ready: true });
        let (mut h, _) = harness_language(view, egui::Theme::Dark, true, language, kept.into());
        h.set_size(egui::vec2(720.0, 1600.0));
        h.run_steps(8);
        let slot = h.query_all_by_label(t("Install")).next().unwrap().rect().width();
        for app in ["MoneyKeep", "BudgetKeep", "InvestKeep"] {
            let menu = h.get_by_label(&fmt(t("More actions for {name}"), &[("name", app)])).rect();
            let main = h
                .query_all_by_role(egui::accesskit::Role::Button)
                .filter(|n| (n.rect().center().y - menu.center().y).abs() < 1.0 && n.rect().left() < menu.left())
                .max_by(|a, b| a.rect().right().total_cmp(&b.rect().right()))
                .unwrap()
                .rect();
            assert!(main.width() <= slot + 0.5, "{language} {view:?} {app}: {main:?} wider than Install ({slot})");
            assert!(main.right() <= menu.left(), "{language} {view:?} {app}: {main:?} overlaps the menu {menu:?}");
            // The table scrolls sideways, with a fixed slot; cards have to fit the window.
            assert!(view == ViewMode::Table || menu.right() <= 720.0, "{language} {view:?} {app}: the menu left the window: {menu:?}");
        }
    }
}

/// The UI over the test catalog, which has more apps in more states than the MoneyKeep Apps.
fn ui_new(ctx: &egui::Context, backend: Box<dyn keep_launcher_core::protocol::Backend>, settings: keep_launcher_ui_egui::Settings) -> LauncherUi {
    LauncherUi::with_catalog(ctx, backend, settings, &SUITE)
}
