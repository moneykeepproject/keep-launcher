//! `cargo xtask workflows`: the self-hosted runner check (MoneyKeep's
//! `docs/specs/macos-release.md`, W1, W2, W9 and O3; tests MAC-1, MAC-2 and MAC-5).
//!
//! The project's Mac is a self-hosted GitHub Actions runner. Code from a pull request must never
//! run on it, so:
//!
//! - only `release.yml` (and MoneyKeep's `mac-runner-check.yml`) may name `self-hosted`;
//! - a workflow that does is triggered only by a pushed tag (`push` with `tags` and nothing else)
//!   and by `workflow_dispatch`;
//! - every job in it that names `self-hosted` waits for the owner in the `mac-release`
//!   environment;
//! - `release.yml` has a `macos` job on `[self-hosted, macOS, ARM64, mk-mac]` in that
//!   environment.
//!
//! There is no YAML parser among the dependencies, so this reads the files line by line. It only
//! understands the block style the workflows here are written in, and anything it does not
//! understand in a self-hosted workflow's triggers is an error (it fails closed).

use std::path::Path;

pub const WORKFLOWS_DIR: &str = ".github/workflows";
/// The only workflows that may name a self-hosted runner.
pub const SELF_HOSTED_ALLOWED: [&str; 2] = ["release.yml", "mac-runner-check.yml"];
/// The environment, with the owner as required reviewer, that self-hosted jobs wait in (O3).
pub const MAC_ENVIRONMENT: &str = "mac-release";
/// `release.yml`'s Mac job and its runner labels (W1).
pub const RELEASE_WORKFLOW: &str = "release.yml";
pub const MAC_JOB: &str = "macos";
pub const MAC_LABELS: [&str; 4] = ["self-hosted", "macOS", "ARM64", "mk-mac"];
/// The triggers a self-hosted workflow may have (W2).
const ALLOWED_TRIGGERS: [&str; 2] = ["push", "workflow_dispatch"];

pub fn run(root: &Path) -> Result<(), String> {
    let files = read_dir(&root.join(WORKFLOWS_DIR))?;
    let errors = check_files(&files);
    if errors.is_empty() {
        println!("workflows: ok ({} files; self-hosted only in {})", files.len(), RELEASE_WORKFLOW);
        Ok(())
    } else {
        Err(format!("{} problem(s) in {WORKFLOWS_DIR}:\n  {}", errors.len(), errors.join("\n  ")))
    }
}

/// Every `.yml` and `.yaml` file in `dir`, as (file name, text), sorted by name.
pub fn read_dir(dir: &Path) -> Result<Vec<(String, String)>, String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    let mut files = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| format!("cannot read {}: {e}", dir.display()))?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()).map(str::to_owned) else {
            continue;
        };
        if !(name.ends_with(".yml") || name.ends_with(".yaml")) {
            continue;
        }
        let text = std::fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        files.push((name, text));
    }
    files.sort();
    Ok(files)
}

/// All problems in a set of workflows (file name, text).
pub fn check_files(files: &[(String, String)]) -> Vec<String> {
    let mut errors = Vec::new();
    for (name, text) in files {
        errors.extend(workflow_errors(name, text));
    }
    match files.iter().find(|(n, _)| n == RELEASE_WORKFLOW) {
        Some((_, text)) => errors.extend(release_errors(text)),
        None => errors.push(format!("{RELEASE_WORKFLOW} is missing")),
    }
    errors
}

/// One workflow's problems under W9 (none for a workflow that doesn't name `self-hosted`).
pub fn workflow_errors(name: &str, text: &str) -> Vec<String> {
    let lines = code_lines(text);
    if !lines.iter().any(|l| l.text.contains("self-hosted")) {
        return Vec::new();
    }
    let mut errors = Vec::new();
    if !SELF_HOSTED_ALLOWED.contains(&name) {
        errors.push(format!("{name} names a self-hosted runner; only {} may (W1, W9)", SELF_HOSTED_ALLOWED.join(" and ")));
    }
    match triggers(&lines) {
        Ok(found) => {
            for t in found {
                if let Some(problem) = t.problem() {
                    errors.push(format!("{name} names a self-hosted runner but {problem} (W2, W9)"));
                }
            }
        }
        Err(e) => errors.push(format!("{name}: {e}")),
    }
    for job in jobs(&lines) {
        if job.names_self_hosted() && job.environment().as_deref() != Some(MAC_ENVIRONMENT) {
            errors.push(format!("{name}: job `{}` runs on a self-hosted runner without `environment: {MAC_ENVIRONMENT}` (O3)", job.id));
        }
    }
    errors
}

/// `release.yml`'s own requirements: the `macos` job (W1, MAC-5).
pub fn release_errors(text: &str) -> Vec<String> {
    let lines = code_lines(text);
    let Some(job) = jobs(&lines).into_iter().find(|j| j.id == MAC_JOB) else {
        return vec![format!("{RELEASE_WORKFLOW} has no `{MAC_JOB}` job")];
    };
    let mut errors = Vec::new();
    let runs_on = job.value("runs-on").unwrap_or_default();
    for label in MAC_LABELS {
        if !runs_on.contains(label) {
            errors.push(format!("{RELEASE_WORKFLOW}: job `{MAC_JOB}`'s runs-on lacks the label `{label}` (W1)"));
        }
    }
    if job.environment().as_deref() != Some(MAC_ENVIRONMENT) {
        errors.push(format!("{RELEASE_WORKFLOW}: job `{MAC_JOB}` must use `environment: {MAC_ENVIRONMENT}` (O3, MAC-5)"));
    }
    errors
}

/// A line with its comment removed, and its indentation.
#[derive(Debug, Clone)]
pub struct Line {
    pub indent: usize,
    pub text: String,
}

impl Line {
    /// `key` and the rest after `key:`, for a `key: value` (or `key:`) line.
    fn key_value(&self) -> Option<(String, String)> {
        let t = self.text.trim_start_matches("- ");
        let (key, rest) = t.split_once(':')?;
        let key = key.trim().trim_matches(|c| c == '"' || c == '\'');
        if key.is_empty() || key.contains(' ') || !(rest.is_empty() || rest.starts_with(' ')) {
            return None;
        }
        Some((key.to_owned(), rest.trim().to_owned()))
    }
}

/// The non-blank lines of a YAML file without comments: a `#` at the start or after a space,
/// outside quotes, starts one.
pub fn code_lines(text: &str) -> Vec<Line> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let mut quote: Option<char> = None;
        let mut prev = ' ';
        let mut end = raw.len();
        for (i, c) in raw.char_indices() {
            match quote {
                Some(q) if c == q => quote = None,
                Some(_) => {}
                None if c == '"' || c == '\'' => quote = Some(c),
                None if c == '#' && prev.is_whitespace() => {
                    end = i;
                    break;
                }
                None => {}
            }
            prev = c;
        }
        let code = raw.get(..end).unwrap_or(raw).trim_end();
        let text = code.trim_start();
        if text.is_empty() {
            continue;
        }
        out.push(Line { indent: code.len() - text.len(), text: text.to_owned() });
    }
    out
}

/// The lines below `lines[at]` that are indented further than it.
fn children(lines: &[Line], at: usize) -> &[Line] {
    let Some(parent) = lines.get(at) else {
        return &[];
    };
    let rest = lines.get(at.saturating_add(1)..).unwrap_or(&[]);
    let n = rest.iter().take_while(|l| l.indent > parent.indent).count();
    rest.get(..n).unwrap_or(&[])
}

/// The direct keys among `block` (its least-indented lines), as (index in block, key, value).
fn direct_keys(block: &[Line]) -> Vec<(usize, String, String)> {
    let Some(indent) = block.iter().map(|l| l.indent).min() else {
        return Vec::new();
    };
    block.iter().enumerate().filter(|(_, l)| l.indent == indent).filter_map(|(i, l)| l.key_value().map(|(k, v)| (i, k, v))).collect()
}

/// A top-level key's line index.
fn top_level(lines: &[Line], keys: &[&str]) -> Option<usize> {
    lines.iter().position(|l| l.indent == 0 && l.key_value().is_some_and(|(k, _)| keys.contains(&k.as_str())))
}

/// One trigger under `on:` and what of it matters here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trigger {
    pub event: String,
    /// For `push`: its filters (`tags`, `branches`, `paths`, …).
    pub filters: Vec<String>,
}

impl Trigger {
    /// Why this trigger isn't allowed on a self-hosted workflow, if it isn't.
    fn problem(&self) -> Option<String> {
        if !ALLOWED_TRIGGERS.contains(&self.event.as_str()) {
            return Some(format!("is triggered by `{}`; only a pushed tag and workflow_dispatch may trigger it", self.event));
        }
        if self.event == "push" {
            if !self.filters.iter().any(|f| f == "tags") {
                return Some("`push` has no `tags` filter, so a branch push triggers it".to_owned());
            }
            if let Some(other) = self.filters.iter().find(|f| *f != "tags") {
                return Some(format!("`push` has `{other}` besides `tags`; only tags may trigger it"));
            }
        }
        None
    }
}

/// The triggers under `on:`. Fails on forms this reader doesn't follow (flow mappings).
pub fn triggers(lines: &[Line]) -> Result<Vec<Trigger>, String> {
    let at = top_level(lines, &["on", "true"]).ok_or("no `on:` triggers")?;
    let (_, inline) = lines.get(at).and_then(Line::key_value).ok_or("no `on:` triggers")?;
    let plain = |event: &str| Trigger { event: event.to_owned(), filters: Vec::new() };
    if !inline.is_empty() {
        // `on: push` or `on: [push, workflow_dispatch]`: no filters.
        if let Some(list) = inline.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            return Ok(list.split(',').map(|e| plain(e.trim().trim_matches(|c| c == '"' || c == '\''))).collect());
        }
        if inline.starts_with('{') {
            return Err("write `on:` in block style so its triggers can be checked".to_owned());
        }
        return Ok(vec![plain(inline.trim_matches(|c| c == '"' || c == '\''))]);
    }
    let block = children(lines, at);
    let mut out = Vec::new();
    for (i, event, value) in direct_keys(block) {
        let mut t = plain(&event);
        if event == "push" {
            if !value.is_empty() {
                return Err("write `push:` in block style so its filters can be checked".to_owned());
            }
            let filters = children(block, i);
            t.filters = direct_keys(filters).into_iter().map(|(_, k, _)| k).collect();
        }
        out.push(t);
    }
    if out.is_empty() {
        return Err("no triggers found under `on:`".to_owned());
    }
    Ok(out)
}

/// One job under `jobs:`.
#[derive(Debug, Clone)]
pub struct Job {
    pub id: String,
    pub body: Vec<Line>,
}

impl Job {
    pub fn names_self_hosted(&self) -> bool {
        self.body.iter().any(|l| l.text.contains("self-hosted"))
    }

    /// The value of one of the job's own keys (`runs-on`, `if`, …), with a block value's lines
    /// (a list, or a `|` or `>` scalar) joined by spaces.
    pub fn value(&self, key: &str) -> Option<String> {
        let (i, _, v) = direct_keys(&self.body).into_iter().find(|(_, k, _)| k == key)?;
        if !(v.is_empty() || v.starts_with('|') || v.starts_with('>')) {
            return Some(v);
        }
        let parts: Vec<&str> = children(&self.body, i).iter().map(|l| l.text.as_str()).collect();
        Some(parts.join(" "))
    }

    /// `environment: <name>` or `environment:` with `name: <name>` below it.
    pub fn environment(&self) -> Option<String> {
        let (i, _, v) = direct_keys(&self.body).into_iter().find(|(_, k, _)| k == "environment")?;
        let name = if v.is_empty() { direct_keys(children(&self.body, i)).into_iter().find(|(_, k, _)| k == "name").map(|(_, _, v)| v)? } else { v };
        Some(name.trim_matches(|c| c == '"' || c == '\'').to_owned())
    }
}

/// The jobs under `jobs:`.
pub fn jobs(lines: &[Line]) -> Vec<Job> {
    let Some(at) = top_level(lines, &["jobs"]) else {
        return Vec::new();
    };
    let block = children(lines, at);
    direct_keys(block).into_iter().map(|(i, id, _)| Job { id, body: children(block, i).to_vec() }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn real() -> Vec<(String, String)> {
        match read_dir(&crate::root().join(WORKFLOWS_DIR)) {
            Ok(f) => f,
            Err(e) => panic!("{e}"),
        }
    }

    fn release_text() -> String {
        real().into_iter().find(|(n, _)| n == RELEASE_WORKFLOW).map(|(_, t)| t).unwrap_or_default()
    }

    fn with(files: &[(&str, &str)]) -> Vec<(String, String)> {
        let mut all: Vec<(String, String)> = real().into_iter().filter(|(n, _)| !files.iter().any(|(f, _)| f == n)).collect();
        all.extend(files.iter().map(|(n, t)| ((*n).to_owned(), (*t).to_owned())));
        all
    }

    const PR_SELF_HOSTED: &str = "\
name: CI
on:
  pull_request:
  push:
    branches: [main]
jobs:
  test:
    runs-on: self-hosted
    steps:
      - run: cargo test
";

    // MAC-1: the repository passes …
    #[test]
    fn the_repository_passes() {
        let errors = check_files(&real());
        assert!(errors.is_empty(), "{errors:#?}");
    }

    // … and a pull-request workflow on a self-hosted runner fails, whatever it is called.
    #[test]
    fn a_pull_request_workflow_on_the_mac_fails() {
        let errors = check_files(&with(&[("ci.yml", PR_SELF_HOSTED)]));
        assert!(errors.iter().any(|e| e.contains("ci.yml names a self-hosted runner; only")), "{errors:#?}");
        assert!(errors.iter().any(|e| e.contains("`pull_request`")), "{errors:#?}");
        assert!(errors.iter().any(|e| e.contains("job `test`") && e.contains(MAC_ENVIRONMENT)), "{errors:#?}");

        let as_release = workflow_errors(RELEASE_WORKFLOW, PR_SELF_HOSTED);
        assert!(as_release.iter().any(|e| e.contains("`pull_request`")), "{as_release:#?}");
        assert!(as_release.iter().any(|e| e.contains("`push` has no `tags` filter")), "{as_release:#?}");
    }

    #[test]
    fn other_triggers_on_a_self_hosted_workflow_fail() {
        let wf = |on: &str| format!("on:\n{on}\njobs:\n  macos:\n    runs-on: [self-hosted, macOS]\n    environment: mac-release\n");
        for (on, want) in [
            ("  push:\n    branches: [release]", "no `tags` filter"),
            ("  push:\n    tags: [\"v*\"]\n    branches: [main]", "`branches` besides `tags`"),
            ("  push:", "no `tags` filter"),
            ("  pull_request_target:", "`pull_request_target`"),
            ("  issue_comment:", "`issue_comment`"),
            ("  workflow_run:\n    workflows: [CI]", "`workflow_run`"),
            ("  schedule:\n    - cron: '0 0 * * *'", "`schedule`"),
        ] {
            let errors = workflow_errors("release.yml", &wf(on));
            assert!(errors.iter().any(|e| e.contains(want)), "{on}: {errors:#?}");
        }
        for on in ["on: [push, workflow_dispatch]", "on: pull_request", "on: {push: {tags: [v*]}}"] {
            let text = format!("{on}\njobs:\n  a:\n    runs-on: self-hosted\n    environment: mac-release\n");
            assert!(!workflow_errors("release.yml", &text).is_empty(), "{on}");
        }
        let ok = wf("  push:\n    tags: [\"v*\"]\n  workflow_dispatch:\n    inputs:\n      macos:\n        type: choice");
        assert_eq!(workflow_errors("release.yml", &ok), Vec::<String>::new());
        assert_eq!(
            workflow_errors(
                "mac-runner-check.yml",
                "on:\n  workflow_dispatch:\njobs:\n  check:\n    runs-on: [self-hosted, mk-mac]\n    environment:\n      name: mac-release\n"
            ),
            Vec::<String>::new()
        );
    }

    #[test]
    fn comments_and_hosted_workflows_are_ignored() {
        let text = "# never on self-hosted runners\non:\n  pull_request:\njobs:\n  a:\n    runs-on: macos-latest # not self-hosted\n";
        assert_eq!(workflow_errors("ci.yml", text), Vec::<String>::new());
    }

    // MAC-5: the Mac job waits in mac-release; without it the check fails.
    #[test]
    fn the_mac_job_needs_the_approval_environment() {
        let text = release_text();
        assert!(text.contains("    environment: mac-release\n"));
        let without = text.replace("    environment: mac-release\n", "");
        let errors = check_files(&with(&[(RELEASE_WORKFLOW, &without)]));
        assert!(errors.iter().any(|e| e.contains("job `macos`") && e.contains("MAC-5")), "{errors:#?}");
        let other = text.replace("    environment: mac-release\n", "    environment: release\n");
        assert!(!check_files(&with(&[(RELEASE_WORKFLOW, &other)])).is_empty());
    }

    /// Evaluates the only expression shapes the Mac job uses, for one value of the `macos` input:
    /// `${{ fromJSON(<cond> && '<json>' || '<json>') }}` and `<x> != '<v>'`.
    fn eval_runs_on(expr: &str, mode: &str) -> serde_json::Value {
        let inner = expr.strip_prefix("${{ fromJSON(").and_then(|s| s.strip_suffix(") }}")).unwrap_or_else(|| panic!("unexpected runs-on {expr}"));
        let (cond, rest) = inner.split_once(" && '").unwrap_or_else(|| panic!("no && in {inner}"));
        let (then, other) = rest.split_once("' || '").unwrap_or_else(|| panic!("no || in {rest}"));
        let other = other.strip_suffix('\'').unwrap_or_else(|| panic!("unquoted {other}"));
        let want = cond.strip_prefix("needs.version.outputs.macos == '").and_then(|s| s.strip_suffix('\''));
        let pick = if want == Some(mode) { then } else { other };
        serde_json::from_str(pick).unwrap_or_else(|e| panic!("{pick}: {e}"))
    }

    // MAC-2: release.yml reads as a workflow; the Mac job runs on the project's Mac for `runner`,
    // on macos-15 for `github` and not at all for `skip`, and the draft release still follows.
    #[test]
    fn the_mac_job_follows_the_macos_input() {
        let text = release_text();
        assert!(!text.contains('\t'), "YAML indents with spaces");
        let lines = code_lines(&text);
        let found = match triggers(&lines) {
            Ok(t) => t,
            Err(e) => panic!("{e}"),
        };
        let events: Vec<&str> = found.iter().map(|t| t.event.as_str()).collect();
        assert_eq!(events, ["push", "workflow_dispatch"]);
        let all = jobs(&lines);
        let ids: Vec<&str> = all.iter().map(|j| j.id.as_str()).collect();
        assert_eq!(ids, ["version", "windows", "macos", "linux", "flatpak", "linux-riscv64", "freebsd", "web", "release"]);

        // The input: runner (default), github, skip; a tag push always uses runner.
        assert!(text.contains("      macos:\n") && text.contains("        options: [runner, github, skip]\n        default: runner\n"));
        assert!(text.contains(
            "          macos=\"${INPUT_MACOS:-runner}\"\n          if [ \"$GITHUB_EVENT_NAME\" = push ]; then\n            macos=runner\n"
        ));

        let Some(mac) = all.iter().find(|j| j.id == MAC_JOB) else { panic!("no macos job") };
        let runs_on = mac.value("runs-on").unwrap_or_default();
        assert_eq!(eval_runs_on(&runs_on, "runner"), serde_json::json!(MAC_LABELS));
        assert_eq!(eval_runs_on(&runs_on, "github"), serde_json::json!("macos-15"));
        assert_eq!(mac.value("if").as_deref(), Some("needs.version.outputs.macos != 'skip'"));
        assert_eq!(mac.value("timeout-minutes").as_deref(), Some("120"));

        // W3: clean checkout without credentials, untracked files removed, kept build folder.
        assert!(text.contains("          persist-credentials: false\n          clean: true\n"));
        assert!(text.contains("run: git clean -ffdx"));
        assert!(text.contains("dir=\"$HOME/mk-build/keep-launcher\""));
        // W4: secrets reach only GitHub's Mac; the kept keychain is never deleted.
        assert!(text.contains("if: always() && env.MACOS_MODE == 'github'"));
        for line in text.lines().filter(|l| l.contains("secrets.APPLE_")) {
            let in_import = line.contains("APPLE_CERTIFICATE");
            assert!(in_import || line.contains("needs.version.outputs.macos == 'github' && secrets."), "{line}");
        }

        // The draft release tolerates a skipped Mac job only when it was skipped on purpose.
        let Some(publish) = all.iter().find(|j| j.id == "release") else { panic!("no release job") };
        let cond = publish.value("if").unwrap_or_default();
        assert!(
            cond.contains("(needs.macos.result == 'success' || (needs.macos.result == 'skipped' && needs.version.outputs.macos == 'skip'))"),
            "{cond}"
        );
    }

    // Every action is GitHub's own or one already allowed, pinned by full commit SHA (W8, M9).
    #[test]
    fn the_release_actions_are_pinned() {
        for line in release_text().lines() {
            let Some(action) = line.trim().trim_start_matches("- ").strip_prefix("uses:").map(str::trim) else {
                continue;
            };
            let (name, rest) = action.split_once('@').unwrap_or((action, ""));
            let sha = rest.split_whitespace().next().unwrap_or("");
            assert!(sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()), "{action}");
            assert!(name.starts_with("actions/") || name == "vmactions/freebsd-vm", "{action}");
        }
    }
}
