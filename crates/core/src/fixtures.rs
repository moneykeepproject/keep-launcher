//! Example catalog entries for tests: apps with the traits the real [`SUITE`](crate::SUITE) has no
//! example of yet (an app that was renamed, a community app with its own publisher and bundle
//! id, a macOS build signed by a known team), and [`TEST_SUITE`], a larger catalog for UI tests
//! that need many apps in different states. They are never shown or installed: nothing outside
//! tests refers to them, and lookups such as [`KeepApp::by_id`](crate::KeepApp::by_id) don't see them.

use crate::catalog::{AppKind, KeepApp, SUITE};

const fn base() -> KeepApp {
    SUITE[1]
}

/// An official app that shipped under another name first (LedgerKeep was BookKeep).
pub const RENAMED: KeepApp = KeepApp {
    id: "ledgerkeep",
    name: "LedgerKeep",
    tagline: "Bookkeeping",
    repo: "moneykeepproject/ledgerkeep",
    former_ids: &["bookkeep"],
    former_names: &["BookKeep"],
    ..base()
};

/// A community app: its own repo, publisher, Windows publisher, bundle ids, site and signing team.
pub const COMMUNITY: KeepApp = KeepApp {
    id: "taxkeep",
    name: "TaxKeep",
    tagline: "Tax returns",
    repo: "example-labs/taxkeep",
    kind: AppKind::Community,
    publisher: "Example Labs",
    msi_publisher: Some("TaxKeep contributors"),
    bundle: Some("org.example.taxkeep"),
    former_bundles: &["com.moneykeep.taxkeep", "com.moneykeep.oldtaxkeep"],
    site: Some("https://taxkeep.example"),
    mac_team: Some("ABCDE12345"),
    ..base()
};

/// The Apple Developer team the [`SIGNED`] example's macOS builds carry.
pub const TEAM_ID: &str = "FGHIJ67890";

/// An official app whose macOS builds carry a Developer ID signature the launcher can check.
pub const SIGNED: KeepApp = KeepApp { mac_team: Some(TEAM_ID), ..SUITE[0] };

/// The byline of [`TEST_SUITE`]'s official apps: not "MoneyKeep", which is also an app's name.
pub const TEST_PUBLISHER: &str = "MoneyKeep Project";

const fn test_app(id: &'static str, name: &'static str, tagline: &'static str, color: [u8; 3]) -> KeepApp {
    KeepApp {
        id,
        name,
        tagline,
        color,
        ink_light: color,
        ink_dark: color,
        repo: "",
        former_ids: &[],
        programs: &[],
        former_names: &[],
        kind: AppKind::Official,
        publisher: TEST_PUBLISHER,
        msi_publisher: None,
        bundle: None,
        former_bundles: &[],
        site: None,
        mac_team: Some(TEAM_ID),
    }
}

/// A seventeen-app catalog for UI tests (`LauncherUi::with_catalog`): the two real apps, signed,
/// plus fictional ones, among them [`RENAMED`] and [`COMMUNITY`]. Each official app's repo is
/// `moneykeepproject/<id>`.
pub const TEST_SUITE: [KeepApp; 17] = [
    KeepApp { mac_team: Some(TEAM_ID), publisher: TEST_PUBLISHER, ..SUITE[0] },
    KeepApp { repo: "moneykeepproject/budgetkeep", ..test_app("budgetkeep", "BudgetKeep", "Budgets", [0xe8, 0x57, 0x3f]) },
    KeepApp { repo: "moneykeepproject/billkeep", ..test_app("billkeep", "BillKeep", "Bills and due dates", [0x8b, 0x5c, 0xf6]) },
    KeepApp { repo: "moneykeepproject/savekeep", ..test_app("savekeep", "SaveKeep", "Savings goals", [0xf2, 0xa5, 0x16]) },
    KeepApp { mac_team: Some(TEAM_ID), publisher: TEST_PUBLISHER, ..RENAMED },
    KeepApp { repo: "moneykeepproject/investkeep", ..test_app("investkeep", "InvestKeep", "Investments", [0xe0, 0x36, 0x8f]) },
    KeepApp { repo: "moneykeepproject/invoicekeep", ..test_app("invoicekeep", "InvoiceKeep", "Invoices", [0x7b, 0xb5, 0x1c]) },
    KeepApp { repo: "moneykeepproject/receiptkeep", ..test_app("receiptkeep", "ReceiptKeep", "Receipts", [0x02, 0xda, 0x96]) },
    KeepApp { repo: "moneykeepproject/notekeep", ..test_app("notekeep", "NoteKeep", "Financial notes", [0x3f, 0x5f, 0xe0]) },
    KeepApp { repo: "moneykeepproject/paykeep", ..test_app("paykeep", "PayKeep", "Payments", [0x1f, 0xa5, 0xc9]) },
    KeepApp { repo: "moneykeepproject/reportkeep", ..test_app("reportkeep", "ReportKeep", "Reports", [0xf0, 0x7a, 0x1e]) },
    KeepApp { repo: "moneykeepproject/assetkeep", ..test_app("assetkeep", "AssetKeep", "Assets", [0x3e, 0xa6, 0xe8]) },
    KeepApp { mac_team: Some(TEAM_ID), publisher: TEST_PUBLISHER, ..SUITE[1] },
    KeepApp { repo: "moneykeepproject/loankeep", ..test_app("loankeep", "LoanKeep", "Loans", [0xc2, 0x67, 0x2b]) },
    KeepApp {
        repo: "moneykeepproject/ratekeep",
        site: Some("https://github.com/moneykeepproject/ratekeep"),
        ..test_app("ratekeep", "RateKeep", "Exchange rates", [0x17, 0x8a, 0x9c])
    },
    KeepApp { repo: "moneykeepproject/keep-anything", ..test_app("keep-anything", "Keep Anything", "Keep track of anything", [0x4e, 0x7b, 0xfb]) },
    COMMUNITY,
];
