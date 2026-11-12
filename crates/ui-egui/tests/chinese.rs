//! The Simplified Chinese UI, in its own test binary: the UI language is process-wide, so it can't
//! share one with the English UI tests. Chinese is drawn with the system's Chinese font; where
//! there is none (CI, minimal Linux installs), the UI must stay in English and say why.
//! `KEEP_LAUNCHER_SNAPSHOTS=<dir>` also saves screenshots (run with `-- --test-threads=1`).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
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
fn simplified_chinese_switches_persists_and_keeps_install_actions() {
    let (mut h, sent) = harness_language(ViewMode::Grid, egui::Theme::Dark, true, "en");
    h.run_steps(6);
    h.get_by_label("Settings").click();
    h.run_steps(3);
    h.get_by_value("English").click();
    h.run_steps(2);
    // Opening Settings doesn't load the (large) Chinese font: the picker names it in English.
    assert!(!keep_launcher_ui_egui::theme::cjk_font_loaded(keep_launcher_ui_egui::i18n::Lang::ZH_CN));
    // The native-name sort puts Chinese last, below the popup's scroll viewport. Scroll the
    // actual popup with a wheel over its visible first row before the physical pointer click.
    let before_scroll = h.get_by_label("Simplified Chinese").rect();
    let first_row = h.get_by_label("Auto").rect();
    let popup_bottom = first_row.top() + h.ctx.global_style().spacing.combo_height;
    if std::env::var_os("KEEP_LAUNCHER_CHINESE_SCROLL_DIAGNOSTICS").is_some() {
        let node = h.get_by_label("Simplified Chinese");
        eprintln!("ScrollIntoView target {:?}: {node:?}", node.accesskit_node().locate());
        node.scroll_to_me();
        h.step();
        eprintln!("after action: rect {:?}, remaining input {:?}", h.get_by_label("Simplified Chinese").rect(), h.ctx.input(|i| i.events.clone()));
        h.step();
        eprintln!("after following pass: rect {:?}", h.get_by_label("Simplified Chinese").rect());
    }
    h.event(egui::Event::PointerMoved(first_row.center()));
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -600.0),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(60);
    let after_scroll = h.get_by_label("Simplified Chinese").rect();
    assert!(after_scroll.center().y < before_scroll.center().y, "the last option must scroll into view: {before_scroll:?} -> {after_scroll:?}");
    assert!(
        (first_row.top()..=popup_bottom).contains(&after_scroll.center().y),
        "the physical click must be inside the popup viewport: {after_scroll:?}, top {}, bottom {popup_bottom}",
        first_row.top()
    );
    assert_eq!(h.state().as_ref().unwrap().settings.language, "en", "scrolling must not select a language");
    h.get_by_label("Simplified Chinese").click();
    h.run_steps(3);
    if !keep_launcher_ui_egui::theme::cjk_font_available(keep_launcher_ui_egui::i18n::Lang::ZH_CN) {
        // No Chinese font: choosing it keeps the UI in English, and Settings says why.
        assert_eq!(h.state().as_ref().unwrap().settings.language, "zh-CN");
        h.get_by_label_contains("No font for this language was found");
        h.get_by_label("Notify me when an installed app has an update");
        snapshot(&mut h, "settings-zh-CN-no-font");
        return;
    }
    // Choosing it loaded the font: now the picker shows both names.
    h.get_by_value("Simplified Chinese (简体中文)");
    h.get_by_label("已安装应用有更新时通知我");
    snapshot(&mut h, "settings-zh-CN-dark");
    let settings = h.state().as_ref().unwrap().settings.clone();
    assert_eq!(settings.language, "zh-CN");
    let saved = serde_json::to_string(&settings).unwrap();
    let restored: Settings = serde_json::from_str(&saved).unwrap();
    assert_eq!(restored.language, "zh-CN");
    h.get_by_label("完成").click();
    h.run_steps(3);
    h.get_by_label("个人与小型企业记账");
    h.get_by_label_contains("上次检查：3 分钟前 · 来源：MoneyKeep API");
    h.get_by_label("更新至 0.5.0");
    // Product names stay in English.
    assert!(h.query_by_label("Launcher").is_none());
    snapshot(&mut h, "grid-zh-CN-dark");
    h.get_by_label("全部更新（2）").click();
    h.run_steps(2);
    let installs: Vec<_> = sent.borrow().iter().filter_map(|c| if let Command::Install { app } = c { Some(app.clone()) } else { None }).collect();
    assert_eq!(installs, ["moneykeep", "investkeep"]);

    let (mut table, _) = harness_language(ViewMode::Table, egui::Theme::Light, true, &restored.language);
    table.run_steps(6);
    table.get_by_label("版本");
    assert!(table.query_all_by_label_contains("年10月").count() >= 1);
    snapshot(&mut table, "table-zh-CN-light");
    let (mut web, _) = harness_language(ViewMode::Grid, egui::Theme::Dark, false, &restored.language);
    web.run_steps(6);
    assert!(web.query_all_by_label("下载 macOS 版").count() >= 3);
}

/// The UI over the test catalog, which has more apps in more states than the MoneyKeep Apps.
fn ui_new(ctx: &egui::Context, backend: Box<dyn keep_launcher_core::protocol::Backend>, settings: keep_launcher_ui_egui::Settings) -> LauncherUi {
    LauncherUi::with_catalog(ctx, backend, settings, &SUITE)
}
