//! The Brazilian Portuguese (pt-BR) UI test and snapshot generator.

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
}

impl Backend for Fake {
    fn send(&mut self, _command: Command) {}
    fn poll(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities { manage_apps: true, reveal_label: "Show in Explorer", self_update: true }
    }
    fn platform(&self) -> Option<Platform> {
        Some(Platform { os: Os::Windows, arch: Arch::X64 })
    }
    fn now(&self) -> i64 {
        self.clock.get()
    }
}

struct NoHost;
impl Host for NoHost {}

fn release(id: &str, v: &str, at: i64) -> Release {
    let name = format!("{id}-{v}-windows-x64.msi");
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
            source: InstallSource::Msi,
            location: format!("C:\\Program Files\\{}\\{}.exe", app.name, app.id),
            package: None,
            externally_managed: false,
            origin: None,
            installation: None,
        });
        ev.push(Event::Detected { app: app.id.into(), installed });
    }
    ev.push(Event::Progress { app: "billkeep".into(), phase: JobPhase::Verifying });
    ev.push(Event::CheckFinished { at: NOW - 180, source: Source::Feed, rate_limited_until: None });
    ev.push(Event::JobFinished { app: "savekeep".into(), result: Ok("SaveKeep updated to 0.5.0".into()) });
    ev
}

fn harness(view: ViewMode) -> Harness<'static, Option<LauncherUi>> {
    keep_launcher_ui_egui::i18n::set_test_lang(keep_launcher_ui_egui::i18n::Lang::EN);
    let mut backend = Some(Fake { events: events(), clock: Rc::new(Cell::new(NOW)) });
    let settings =
        Settings { view, notify: false, language: "pt-BR".into(), theme: keep_launcher_ui_egui::theme::ThemeChoice::Dark, ..Settings::default() };
    Harness::builder().with_size(egui::vec2(1180.0, 900.0)).with_theme(egui::Theme::Dark).build_ui_state(
        move |ui, state: &mut Option<LauncherUi>| {
            if state.is_none()
                && let Some(b) = backend.take()
            {
                *state = Some(ui_new(ui.ctx(), Box::new(b), settings.clone()));
            }
            if let Some(l) = state.as_mut() {
                l.logic(&ui.ctx().clone(), &mut NoHost);
                l.ui(ui, &mut NoHost);
            }
        },
        None,
    )
}

#[test]
fn portuguese_ui_translates_labels_dates_and_messages() {
    let mut h = harness(ViewMode::Grid);
    h.run_steps(8);
    assert!(h.query_by_label("Launcher").is_none());
    h.get_by_label("MoneyKeep Apps");
    h.get_by_label("Contabilidade pessoal e para pequenas empresas");
    h.get_by_label("Atualizar para 0.5.0");
    h.get_by_label_contains("Conferindo a soma de verificação…");
    h.get_by_label_contains("Verificado há 3 min · via MoneyKeep API");
    h.get_by_label("SaveKeep atualizado para 0.5.0");

    let mut table = harness(ViewMode::Table);
    table.run_steps(8);
    table.get_by_label("Versão");
    assert!(table.query_all_by_label_contains("de ago. de 2026").count() >= 1);
}

/// Brazilian Portuguese writes the launcher's longest dates ("29 de set. de 2026"): the table's
/// released column must show one whole, not cut to "29 de set...".
#[test]
fn the_table_shows_the_longest_dates_whole() {
    let mut h = harness(ViewMode::Table);
    h.run_steps(4);
    let text = "29 de ago. de 2026";
    let font = egui::TextStyle::Body.resolve(&h.ctx.global_style());
    let whole = h.ctx.fonts_mut(|f| f.layout_no_wrap(text.into(), font, egui::Color32::WHITE).size().x);
    let date = h.get_all_by_label_contains(text).next().expect("a release date in the table");
    // A label cut with an ellipsis keeps its full text, so only its width tells.
    assert!(date.rect().width() >= whole - 0.5, "{:?} of {whole}", date.rect());
}

/// The UI over the test catalog, which has more apps in more states than the MoneyKeep Apps.
fn ui_new(ctx: &egui::Context, backend: Box<dyn keep_launcher_core::protocol::Backend>, settings: keep_launcher_ui_egui::Settings) -> LauncherUi {
    LauncherUi::with_catalog(ctx, backend, settings, &SUITE)
}
