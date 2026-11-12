//! Where release information comes from.
//!
//! [`GitHubSource`] asks the GitHub REST API for each app's ten newest releases
//! (`releases?per_page=10`, one request per app, revalidated with ETags) and picks the newest
//! published one, pre-releases included when the app has no stable release yet
//! ([`release::parse_github_release_list`]); `releases/latest` would skip them. When the API is
//! out of quota it falls back to github.com's release pages: the `releases/latest` redirect
//! names the newest stable tag and `SHA256SUMS.txt` lists every file with its checksum, which
//! costs no API quota at all. A later [`ReleaseSource`] will ask the MoneyKeep
//! catalog API for every app in one request.

use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};

use keep_launcher_core::protocol::{LatestRelease, Source};
use keep_launcher_core::release::{self, Release};
use serde::{Deserialize, Serialize};

use crate::http::{Fetched, Http};

/// One answer from a source.
#[derive(Clone, Debug)]
pub struct Answer {
    pub latest: LatestRelease,
    pub source: Source,
}

/// Looks up latest releases. Called from worker threads.
pub trait ReleaseSource: Send + Sync {
    fn latest(&self, repo: &str, prereleases: bool) -> Answer;
    /// `repo`'s published releases, newest first, for choosing a version.
    fn versions(&self, _repo: &str) -> Result<Vec<Release>, String> {
        Err("version list isn't available here".into())
    }
    /// The release `track` follows: the latest stable one, or the chosen nightly or exact version
    /// from [`versions`](Self::versions), with no fallback to another.
    fn chosen(&self, repo: &str, track: &keep_launcher_core::protocol::VersionTrack) -> Answer {
        if *track == keep_launcher_core::protocol::VersionTrack::Stable {
            return self.latest(repo, false);
        }
        let latest = match self.versions(repo) {
            Ok(list) => release::choose_version(&list, track).map_or(LatestRelease::None, LatestRelease::Found),
            Err(why) => LatestRelease::Failed(why),
        };
        Answer { latest, source: Source::GitHubApi }
    }
    /// The last answer saved for `repo`, for an instant first paint.
    fn cached(&self, repo: &str, prereleases: bool) -> Option<LatestRelease>;
    /// The tag of `repo`'s newest stable release, by a route that costs no API quota (GitHub's
    /// `releases/latest` redirect), or `None` when it can't be told. Quick: a check pressed by the
    /// user asks this for every app to see whether the feed has fallen behind.
    fn newest_tag(&self, repo: &str) -> Option<String> {
        let _ = repo;
        None
    }
    /// API refused until this time (Unix seconds).
    fn rate_limited_until(&self) -> Option<i64>;
    /// Persist anything worth keeping.
    fn save(&self);
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Entry {
    etag: Option<String>,
    /// `None`: the repo has no releases.
    release: Option<Release>,
    prereleases: bool,
    fetched_at: i64,
}

#[derive(Default, Serialize, Deserialize)]
struct CacheFile {
    entries: BTreeMap<String, Entry>,
}

pub struct GitHubSource {
    http: Http,
    cache: Mutex<CacheFile>,
    versions_cache: Mutex<BTreeMap<String, (i64, Vec<Release>)>>,
    cache_path: std::path::PathBuf,
    limited_until: Mutex<Option<i64>>,
    /// `https://api.github.com`; a loopback fake server in tests.
    api_base: String,
}

impl GitHubSource {
    pub fn new(http: Http, cache_path: std::path::PathBuf) -> Self {
        let cache = std::fs::read(&cache_path).ok().and_then(|b| serde_json::from_slice::<CacheFile>(&b).ok()).unwrap_or_default();
        Self {
            http,
            cache: Mutex::new(cache),
            versions_cache: Mutex::new(BTreeMap::new()),
            cache_path,
            limited_until: Mutex::new(None),
            api_base: "https://api.github.com".into(),
        }
    }

    /// The same source asking a fake API server on this machine.
    #[cfg(test)]
    fn with_api_base(mut self, base: &str) -> Self {
        self.api_base = base.trim_end_matches('/').to_string();
        self
    }

    fn entry(&self, repo: &str, prereleases: bool) -> Option<Entry> {
        let cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        cache.entries.get(repo).filter(|e| e.prereleases == prereleases).cloned()
    }

    fn store(&self, repo: &str, entry: Entry) {
        self.cache.lock().unwrap_or_else(PoisonError::into_inner).entries.insert(repo.to_string(), entry);
    }

    fn limited(&self) -> bool {
        let until = *self.limited_until.lock().unwrap_or_else(PoisonError::into_inner);
        until.is_some_and(|u| u > crate::now())
    }

    fn ask_api(&self, repo: &str, prereleases: bool) -> Result<Option<Answer>, String> {
        let url = format!("{}/repos/{repo}/releases?per_page={RELEASES_PER_LOOKUP}", self.api_base);
        let saved = self.entry(repo, prereleases);
        let found =
            |release: Option<Release>| Answer { latest: release.map_or(LatestRelease::None, LatestRelease::Found), source: Source::GitHubApi };
        match self.http.get_api(&url, saved.as_ref().and_then(|e| e.etag.as_deref()))? {
            Fetched::Body { text, etag } => {
                let release = release::parse_github_release_list(&text, repo, prereleases)?;
                self.store(repo, Entry { etag, release: release.clone(), prereleases, fetched_at: crate::now() });
                Ok(Some(found(release)))
            }
            Fetched::NotModified => match saved {
                Some(e) => {
                    self.store(repo, Entry { fetched_at: crate::now(), ..e.clone() });
                    Ok(Some(found(e.release)))
                }
                None => Err("GitHub said nothing changed, but there's no saved answer".into()),
            },
            Fetched::NotFound => {
                self.store(repo, Entry { etag: None, release: None, prereleases, fetched_at: crate::now() });
                Ok(Some(found(None)))
            }
            Fetched::RateLimited { reset } => {
                *self.limited_until.lock().unwrap_or_else(PoisonError::into_inner) = Some(retry_at(reset, crate::now()));
                Ok(None)
            }
        }
    }

    /// The quota-free route through github.com (stable releases only).
    fn ask_pages(&self, repo: &str) -> Result<Answer, String> {
        let location = self.http.redirect_target(&format!("https://github.com/{repo}/releases/latest"))?;
        let Some(tag) = location.as_deref().and_then(|l| release::tag_from_redirect(l, repo)) else {
            // No stable release: github.com sends us to the release list instead. The app may
            // still have pre-releases, which this route can't see, so it says nothing and the
            // last answer from the API stands.
            return Err("no stable release on the release pages".into());
        };
        if let Some(e) = self.entry(repo, false)
            && let Some(r) = e.release.as_ref().filter(|r| r.tag == tag)
        {
            return Ok(Answer { latest: LatestRelease::Found(r.clone()), source: Source::GitHubPages });
        }
        let sums_url = format!("{}{tag}/SHA256SUMS.txt", release::download_prefix(repo));
        let sums = self.http.get_text(&sums_url)?.map(|t| release::parse_sha256sums(&t)).unwrap_or_default();
        let r = release::release_from_sums(repo, &tag, &sums);
        self.store(repo, Entry { etag: None, release: Some(r.clone()), prereleases: false, fetched_at: crate::now() });
        Ok(Answer { latest: LatestRelease::Found(r), source: Source::GitHubPages })
    }
}

/// How many of the newest releases one lookup reads: enough to step over a few drafts and
/// nightlies, small enough to stay a quick answer.
const RELEASES_PER_LOOKUP: u32 = 10;

/// GitHub's quota refills within the hour, so a later reset is a bad answer.
const MAX_LIMIT_WAIT: i64 = 60 * 60;

/// When to ask the API again after a refusal.
fn retry_at(reset: Option<i64>, now: i64) -> i64 {
    reset.unwrap_or(now + 15 * 60).min(now + MAX_LIMIT_WAIT)
}

impl ReleaseSource for GitHubSource {
    fn versions(&self, repo: &str) -> Result<Vec<Release>, String> {
        if let Some((at, list)) = self.versions_cache.lock().unwrap_or_else(PoisonError::into_inner).get(repo)
            && crate::now().saturating_sub(*at) < 15 * 60
        {
            return Ok(list.clone());
        }
        if self.limited() {
            return Err("GitHub's request limit was reached. Try again later.".into());
        }
        let url = format!("{}/repos/{repo}/releases?per_page=100", self.api_base);
        let list = match self.http.get_api(&url, None)? {
            Fetched::Body { text, .. } => release::parse_github_versions(&text, repo)?,
            Fetched::NotFound => Vec::new(),
            Fetched::RateLimited { reset } => {
                *self.limited_until.lock().unwrap_or_else(PoisonError::into_inner) = Some(retry_at(reset, crate::now()));
                return Err("GitHub's request limit was reached. Try again later.".into());
            }
            Fetched::NotModified => return Err("GitHub returned no version list. Try again.".into()),
        };
        self.versions_cache.lock().unwrap_or_else(PoisonError::into_inner).insert(repo.to_string(), (crate::now(), list.clone()));
        Ok(list)
    }

    fn latest(&self, repo: &str, prereleases: bool) -> Answer {
        let api_error = if self.limited() {
            None
        } else {
            match self.ask_api(repo, prereleases) {
                Ok(Some(answer)) => return answer,
                Ok(None) => None,
                Err(e) => Some(e),
            }
        };
        if !prereleases && api_error.is_none() {
            match self.ask_pages(repo) {
                Ok(answer) => return answer,
                Err(e) => log::warn!("{repo}: release pages: {e}"),
            }
        }
        let why = api_error.unwrap_or_else(|| "GitHub's request limit was reached".into());
        match self.cached(repo, prereleases) {
            Some(latest) => Answer { latest, source: Source::Cache },
            None => Answer { latest: LatestRelease::Failed(why), source: Source::Cache },
        }
    }

    fn cached(&self, repo: &str, prereleases: bool) -> Option<LatestRelease> {
        self.entry(repo, prereleases).map(|e| e.release.map_or(LatestRelease::None, LatestRelease::Found))
    }

    fn newest_tag(&self, repo: &str) -> Option<String> {
        match self.http.redirect_target(&format!("https://github.com/{repo}/releases/latest")) {
            Ok(location) => location.as_deref().and_then(|l| release::tag_from_redirect(l, repo)),
            Err(e) => {
                log::debug!("{repo}: couldn't read the latest-release redirect: {e}");
                None
            }
        }
    }

    fn rate_limited_until(&self) -> Option<i64> {
        let until = *self.limited_until.lock().unwrap_or_else(PoisonError::into_inner);
        until.filter(|u| *u > crate::now())
    }

    fn save(&self) {
        let json = {
            let cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
            serde_json::to_vec(&*cache)
        };
        match json {
            Ok(bytes) => {
                if let Err(e) = crate::paths::write_atomic(&self.cache_path, &bytes) {
                    log::warn!("release cache: {e}");
                }
            }
            Err(e) => log::warn!("release cache: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::Arc;

    const REPO: &str = "moneykeepproject/calckeep";

    /// One request the fake API saw: its path and the ETag it revalidated with.
    #[derive(Clone, Debug, PartialEq)]
    struct Seen {
        path: String,
        etag: Option<String>,
    }

    /// A fake `api.github.com` on this machine answering one connection per entry of `replies`
    /// (each a whole raw HTTP response), in order.
    fn fake_api(replies: Vec<String>) -> (String, Arc<Mutex<Vec<Seen>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        std::thread::spawn(move || {
            for reply in replies {
                let Ok((mut stream, _)) = listener.accept() else { return };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request = String::new();
                reader.read_line(&mut request).unwrap();
                let mut etag = None;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some((name, value)) = line.split_once(':')
                        && name.eq_ignore_ascii_case("if-none-match")
                    {
                        etag = Some(value.trim().to_string());
                    }
                }
                let path = request.split_whitespace().nth(1).unwrap_or_default().to_string();
                log.lock().unwrap().push(Seen { path, etag });
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        (base, seen)
    }

    fn ok(body: &str, etag: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nETag: {etag}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    fn not_modified() -> String {
        "HTTP/1.1 304 Not Modified\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into()
    }

    fn release_json(tag: &str, prerelease: bool, draft: bool) -> String {
        format!(
            r#"{{"tag_name":"{tag}","name":"CalcKeep {tag}","prerelease":{prerelease},"draft":{draft},"html_url":"https://github.com/{REPO}/releases/tag/{tag}","assets":[{{"name":"SHA256SUMS.txt","size":10,"browser_download_url":"https://github.com/{REPO}/releases/download/{tag}/SHA256SUMS.txt"}}]}}"#
        )
    }

    /// An alpha-only repo, newest first as GitHub lists it, with a draft on top.
    fn alpha_list() -> String {
        format!(
            "[{},{},{}]",
            release_json("v0.1.0", false, true),
            release_json("v0.1.0-alpha.2", true, false),
            release_json("v0.1.0-alpha.1", true, false)
        )
    }

    fn source(base: &str, dir: &tempfile::TempDir) -> GitHubSource {
        GitHubSource::new(Http::new().unwrap(), dir.path().join("releases.json")).with_api_base(base)
    }

    fn tag(answer: &Answer) -> Option<&str> {
        match &answer.latest {
            LatestRelease::Found(r) => Some(r.tag.as_str()),
            _ => None,
        }
    }

    #[test]
    fn a_lookup_lists_the_newest_releases_and_finds_a_pre_release() {
        let (base, seen) = fake_api(vec![ok(&alpha_list(), "\"a1\"")]);
        let dir = tempfile::tempdir().unwrap();
        let answer = source(&base, &dir).latest(REPO, false);
        assert_eq!(tag(&answer), Some("v0.1.0-alpha.2"), "{:?}", answer.latest);
        assert_eq!(answer.source, Source::GitHubApi);
        assert_eq!(*seen.lock().unwrap(), [Seen { path: format!("/repos/{REPO}/releases?per_page=10"), etag: None }]);
    }

    #[test]
    fn with_pre_releases_on_the_newest_release_wins_over_a_stable_one() {
        let list = format!("[{},{}]", release_json("v0.2.0-beta.1", true, false), release_json("v0.1.0", false, false));
        let (base, _) = fake_api(vec![ok(&list, "\"b\""), ok(&list, "\"b\"")]);
        let dir = tempfile::tempdir().unwrap();
        let github = source(&base, &dir);
        assert_eq!(tag(&github.latest(REPO, false)), Some("v0.1.0"));
        assert_eq!(tag(&github.latest(REPO, true)), Some("v0.2.0-beta.1"));
    }

    #[test]
    fn the_answer_is_revalidated_with_its_etag_and_kept_between_runs() {
        let (base, seen) = fake_api(vec![ok(&alpha_list(), "\"a1\""), not_modified(), not_modified()]);
        let dir = tempfile::tempdir().unwrap();
        let github = source(&base, &dir);
        assert_eq!(tag(&github.latest(REPO, false)), Some("v0.1.0-alpha.2"));
        let again = github.latest(REPO, false);
        assert_eq!((tag(&again), again.source), (Some("v0.1.0-alpha.2"), Source::GitHubApi));
        github.save();
        // The next run shows the saved answer at once and revalidates with the saved ETag.
        let next_run = source(&base, &dir);
        assert!(matches!(next_run.cached(REPO, false), Some(LatestRelease::Found(r)) if r.tag == "v0.1.0-alpha.2"));
        assert_eq!(next_run.cached(REPO, true), None, "answers are kept per pre-release setting");
        assert_eq!(tag(&next_run.latest(REPO, false)), Some("v0.1.0-alpha.2"));
        let seen = seen.lock().unwrap();
        assert_eq!(seen.iter().map(|s| s.etag.as_deref()).collect::<Vec<_>>(), [None, Some("\"a1\""), Some("\"a1\"")]);
    }

    #[test]
    fn a_rate_limit_falls_back_to_the_saved_answer_and_stops_asking() {
        let reset = crate::now() + 30 * 60;
        let refused = format!(
            "HTTP/1.1 403 Forbidden\r\nx-ratelimit-remaining: 0\r\nx-ratelimit-reset: {reset}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        let (base, seen) = fake_api(vec![ok(&alpha_list(), "\"a1\""), refused]);
        let dir = tempfile::tempdir().unwrap();
        let github = source(&base, &dir);
        // Pre-releases on, so the quota-free github.com route (stable only) stays out of it.
        assert_eq!(tag(&github.latest(REPO, true)), Some("v0.1.0-alpha.2"));
        let limited = github.latest(REPO, true);
        assert_eq!((tag(&limited), limited.source), (Some("v0.1.0-alpha.2"), Source::Cache));
        assert_eq!(github.rate_limited_until(), Some(reset));
        // While refused, no request goes out at all.
        let quiet = github.latest(REPO, true);
        assert_eq!((tag(&quiet), quiet.source), (Some("v0.1.0-alpha.2"), Source::Cache));
        assert_eq!(seen.lock().unwrap().len(), 2);
    }

    #[test]
    fn a_repo_without_releases_has_none() {
        let (base, _) = fake_api(vec![ok("[]", "\"e\""), "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into()]);
        let dir = tempfile::tempdir().unwrap();
        let github = source(&base, &dir);
        assert!(matches!(github.latest(REPO, false).latest, LatestRelease::None));
        assert!(matches!(github.latest("moneykeepproject/missing", false).latest, LatestRelease::None));
    }

    #[test]
    fn a_refusal_waits_for_the_reset_github_names() {
        assert_eq!(retry_at(Some(1_000 + 600), 1_000), 1_600);
        assert_eq!(retry_at(None, 1_000), 1_000 + 15 * 60);
    }

    #[test]
    fn a_reset_past_the_quota_window_cannot_stop_the_api_for_good() {
        assert_eq!(retry_at(Some(i64::MAX), 1_000), 1_000 + 60 * 60);
        assert_eq!(retry_at(Some(99_999_999_999), 1_000), 1_000 + 60 * 60);
    }
}
