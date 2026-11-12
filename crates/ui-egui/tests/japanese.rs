//! The Japanese UI, in its own test binary: the UI language is process-wide, so it can't share one
//! with the English UI tests. Japanese is drawn with the system's Japanese font; where there is
//! none (CI, minimal Linux installs), the UI stays in English and says why.
//! `KEEP_LAUNCHER_SNAPSHOTS=<dir>` also saves screenshots (run with `-- --test-threads=1`).

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
) -> (Harness<'static, Option<LauncherUi>>, Rc<RefCell<Vec<Command>>>) {
    keep_launcher_ui_egui::i18n::set_test_lang(keep_launcher_ui_egui::i18n::Lang::EN);
    let sent = Rc::new(RefCell::new(Vec::new()));
    let sent2 = sent.clone();
    let mut backend = Some(Fake { events: events(), sent: sent2, manage, clock: Rc::new(Cell::new(NOW)) });
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
fn japanese_switches_persists_and_keeps_install_actions() {
    use keep_launcher_ui_egui::i18n::Lang;
    let (mut h, sent) = harness_language(ViewMode::Grid, egui::Theme::Dark, true, "en");
    h.run_steps(6);
    h.get_by_label("Settings").click();
    h.run_steps(3);
    h.get_by_value("English").click();
    h.run_steps(2);
    // Opening Settings doesn't load the (large) Japanese font: the picker names it in English.
    assert!(!keep_launcher_ui_egui::theme::cjk_font_loaded(Lang::JA));
    // The list is long enough to scroll, so the last option may be out of view: click it through
    // accesskit, which reaches widgets that aren't visible.
    h.get_by_label("Japanese").click_accesskit();
    h.run_steps(3);
    if !keep_launcher_ui_egui::theme::cjk_font_available(Lang::JA) {
        // No font for Japanese: choosing it keeps the UI in English, and Settings says why.
        assert_eq!(h.state().as_ref().unwrap().settings.language, "ja");
        h.get_by_label_contains("No font for this language was found");
        h.get_by_label("Notify me when an installed app has an update");
        snapshot(&mut h, "settings-ja-no-font");
        return;
    }
    // Choosing it loaded the font: now the picker shows both names.
    h.get_by_value("Japanese (日本語)");
    h.get_by_label("インストール済みアプリに更新があるときに通知する");
    snapshot(&mut h, "settings-ja-dark");
    let settings = h.state().as_ref().unwrap().settings.clone();
    assert_eq!(settings.language, "ja");
    let saved = serde_json::to_string(&settings).unwrap();
    let restored: Settings = serde_json::from_str(&saved).unwrap();
    assert_eq!(restored.language, "ja");
    h.get_by_label("完了").click();
    h.run_steps(3);
    h.get_by_label("個人と小規模事業の会計");
    h.get_by_label_contains("最終確認: 3 分前 · 取得元: MoneyKeep API");
    h.get_by_label("0.5.0 に更新");
    // Product names stay in English.
    assert!(h.query_by_label("Launcher").is_none());
    snapshot(&mut h, "grid-ja-dark");
    h.get_by_label("すべて更新（2）").click();
    h.run_steps(2);
    let installs: Vec<_> = sent.borrow().iter().filter_map(|c| if let Command::Install { app } = c { Some(app.clone()) } else { None }).collect();
    assert_eq!(installs, ["moneykeep", "investkeep"]);

    let (mut table, _) = harness_language(ViewMode::Table, egui::Theme::Light, true, &restored.language);
    table.run_steps(6);
    table.get_by_label("バージョン");
    assert!(table.query_all_by_label_contains("年10月").count() >= 1);
    snapshot(&mut table, "table-ja-light");
    let (mut web, _) = harness_language(ViewMode::Grid, egui::Theme::Dark, false, &restored.language);
    web.run_steps(6);
    assert!(web.query_all_by_label("macOS 版をダウンロード").count() >= 3);
}

/// The UI over the test catalog, which has more apps in more states than the MoneyKeep Apps.
fn ui_new(ctx: &egui::Context, backend: Box<dyn keep_launcher_core::protocol::Backend>, settings: keep_launcher_ui_egui::Settings) -> LauncherUi {
    LauncherUi::with_catalog(ctx, backend, settings, &SUITE)
}
