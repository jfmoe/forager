//! Page driving shared by the Xiaohongshu operations: what passive capture keeps, the page-facts
//! script, scrolling to the end of a list, and the read that waits for the page's own
//! responses and records why it stopped. Navigating, taking capture, and tolerating a page that
//! briefly answers no script are the shared reading session's.

use std::time::{Duration, Instant};

use reqwest::Url;
use serde::Deserialize;

use super::{ACCOUNT_PATH, ApiError, LOGIN_CODES, PageFacts, RISK_CONTROL_STATUSES, on_site};
use crate::net::AttemptFailure;
use crate::providers::browser::{
    CaptureLevel, Exchange, InputError, InputFailure, Missed, Page, RequestMethod, ResponseBody,
};
use crate::providers::browser_route::PageReading;
use crate::types::Deadline;

const POLL: Duration = Duration::from_millis(500);
// A list loads its next page by itself a moment after the last one, so the first scroll waits.
const FIRST_SCROLL_AFTER: Duration = Duration::from_millis(1500);
const SCROLL_EVERY: Duration = Duration::from_secs(4);
// How long no new response must arrive before the page counts as settled.
const SETTLE: Duration = Duration::from_millis(1500);

/// What capture keeps of a request: the body of the `bodies` paths and of the account request,
/// and the status of every other request to the site, which shows risk control.
pub(super) fn site_capture(url: &Url, bodies: &[&str]) -> CaptureLevel {
    if !on_site(url) {
        CaptureLevel::Ignore
    } else if url.path() == ACCOUNT_PATH || bodies.contains(&url.path()) {
        CaptureLevel::Body
    } else {
        CaptureLevel::Status
    }
}

/// A request the page logic waits for.
#[derive(Clone, Copy, Debug)]
pub(super) struct Tracked {
    pub(super) path: &'static str,
    pub(super) method: RequestMethod,
}

impl Tracked {
    pub(super) fn matches(self, exchange: &Exchange) -> bool {
        exchange.method == self.method && exchange.url.path() == self.path
    }

    /// The responses to this request among `exchanges` that finished loading, with or without
    /// a body.
    pub(super) fn completed(self, exchanges: &[Exchange]) -> impl Iterator<Item = &Exchange> {
        exchanges.iter().filter(move |exchange| {
            self.matches(exchange)
                && matches!(
                    exchange.body,
                    ResponseBody::Received(_) | ResponseBody::Missing
                )
        })
    }
}

/// Which completed responses to a tracked request a read waits for.
#[derive(Clone, Copy, Debug)]
enum Awaited {
    /// Any after the first this many.
    After(usize),
    /// Those to requests the page sent once this many requests had gone out.
    SentFrom(u64),
}

impl Awaited {
    fn among(self, tracked: Tracked, exchanges: &[Exchange]) -> Vec<&Exchange> {
        let completed = tracked.completed(exchanges);
        match self {
            Self::After(seen) => completed.skip(seen).collect(),
            Self::SentFrom(sent) => completed
                .filter(|exchange| exchange.sequence >= sent)
                .collect(),
        }
    }
}

// The helpers every page script can use. The site keeps its state in Vue refs, which hold their
// value in `value` or `_value`.
const PAGE_HELPERS: &str = r"
  const pick = (object, keys) => {
    const picked = {};
    for (const key of keys) if (object && object[key] !== undefined) picked[key] = object[key];
    return picked;
  };
  const unwrap = (value) => (value && typeof value === 'object' ? ('value' in value ? value.value : value._value) : value);
  const pageState = () => window.__INITIAL_STATE__;
  const noteDetailMap = () => {
    const state = pageState();
    return state && state.note && state.note.noteDetailMap;
  };";

/// Wraps a read-only script body to evaluate in the page, with the shared page helpers in
/// scope.
pub(super) fn page_script(body: &str) -> String {
    format!("(() => {{{PAGE_HELPERS}\n{body}\n}})()")
}

// The page state that ends any read: login walls, block pages, and their notices. A notice is
// read only while no note card and no note is shown, so text in a note title, a note, or its
// comments never counts as one. The URL never carries the access token.
const PAGE_STATE: &str = r"
  // The browser's own error page for a navigation that failed, such as ERR_CONNECTION_CLOSED.
  // Its location is chrome-error://chromewebdata/; the navigation entry keeps the URL it tried.
  const loadError = location.protocol === 'chrome-error:'
    ? ((document.querySelector('.error-code') || {}).textContent || '').trim() || 'unknown network error'
    : null;
  const navigation = performance.getEntriesByType('navigation')[0];
  let url;
  try { url = new URL(loadError && navigation ? navigation.name : location.href); } catch { url = new URL(location.href); }
  url.searchParams.delete('xsec_token');
  const errorPage = /^\/(404|website-login\/error)/.test(url.pathname);
  const cards = document.querySelectorAll('section.note-item').length;
  const notes = noteDetailMap();
  const noteShown = Boolean(notes) && Object.values(notes).some((entry) => entry && entry.note && entry.note.noteId);
  const rendered = cards > 0 || noteShown;
  const text = document.body && !loadError ? document.body.innerText : '';
  const notice = rendered
    ? null
    : (text.match(/[^\n]*(安全限制|访问链接异常|登录后查看|请求太频繁|访问频次异常)[^\n]*/) || [null])[0];
  const state = pageState();
  const loggedIn = unwrap(state && state.user ? state.user.loggedIn : undefined);
  return {
    url: url.href,
    title: document.title,
    error_code: errorPage ? url.searchParams.get('error_code') : null,
    notice: notice ? notice.trim().slice(0, 120) : null,
    load_error: loadError,
    logged_out: loggedIn === false && !rendered,
  };";

#[derive(Deserialize)]
struct PageState {
    url: String,
    title: String,
    error_code: Option<String>,
    notice: Option<String>,
    load_error: Option<String>,
    logged_out: bool,
}

// Where the wheel turns to reach the end of the list the page shows, and how far that is. A
// note page lists its comments in its own scrolling panel, and so does the results page of the
// `/search_result_ai` layout; the older results page scrolls as a whole.
const SCROLL_PLAN: &str = r"
  const width = document.documentElement.clientWidth;
  const height = document.documentElement.clientHeight;
  const visible = (element) => {
    const rect = element.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0;
  };
  const panel = [...document.querySelectorAll('.note-scroller, .search-layout-wrapper')]
    .find((element) => visible(element) && element.scrollHeight > element.clientHeight);
  if (panel) {
    const rect = panel.getBoundingClientRect();
    const left = Math.max(rect.left, 0);
    const right = Math.min(rect.right, width);
    const top = Math.max(rect.top, 0);
    const bottom = Math.min(rect.bottom, height);
    if (right > left && bottom > top) {
      return { x: (left + right) / 2, y: (top + bottom) / 2, distance: panel.scrollHeight - panel.clientHeight - panel.scrollTop };
    }
  }
  const root = document.scrollingElement || document.documentElement;
  return { x: width / 2, y: height / 2, distance: root.scrollHeight - root.clientHeight - root.scrollTop };";

#[derive(Deserialize)]
struct ScrollPlan {
    x: f64,
    y: f64,
    distance: f64,
}

/// Turns the outcome of a native action into why it did not happen, or a transport failure.
pub(super) fn input_failure(
    result: Result<(), InputError>,
) -> Result<Option<Missed>, AttemptFailure> {
    match result {
        Ok(()) => Ok(None),
        Err(InputError::Failed(failure)) => Ok(Some(failure)),
        Err(InputError::Transport(failure)) => Err(failure),
    }
}

/// Records what captured exchanges show: a risk control status on any request, a logged-out
/// session in the account answer or a login code in any answer, and the first other failure
/// `code` in an awaited API answer. Capture keeps the body only of the account request and the
/// awaited ones. A `code` of 0 is no failure even with `success: false`, which `search/onebox`
/// answers on a working search.
fn absorb(facts: &mut PageFacts, exchanges: &[Exchange]) {
    for exchange in exchanges {
        if let Some(status) = exchange
            .status
            .filter(|status| RISK_CONTROL_STATUSES.contains(status))
        {
            facts.blocked_status.get_or_insert(status);
        }
        let ResponseBody::Received(body) = &exchange.body else {
            continue;
        };
        let Ok(answer) = serde_json::from_str::<serde_json::Value>(body) else {
            continue;
        };
        let account = exchange.url.path() == ACCOUNT_PATH;
        let code = answer["code"].as_i64();
        if code.is_some_and(|code| LOGIN_CODES.contains(&code))
            || (account && answer["data"]["guest"] == true)
        {
            facts.guest = true;
        } else if let Some(code) = code.filter(|code| *code != 0 && !account)
            && facts.api_error.is_none()
        {
            facts.api_error = Some(ApiError {
                path: exchange.url.path().to_owned(),
                code,
                msg: answer["msg"]
                    .as_str()
                    .map(|msg| msg.chars().take(120).collect()),
            });
        }
    }
}

/// What one read observed.
pub(super) struct Observed {
    pub(super) page: PageFacts,
    /// Every exchange capture kept, in the order the responses ended.
    pub(super) exchanges: Vec<Exchange>,
    /// The read deadline passed before the awaited responses arrived.
    pub(super) timed_out: bool,
    /// An awaited response finished loading, but the browser could not return its body.
    pub(super) body_missing: bool,
}

/// One operation's read of the page: it keeps every exchange capture holds, follows the page
/// facts, and waits for the responses to the tracked requests.
pub(super) struct Reading<'p> {
    session: PageReading<'p>,
    facts: PageFacts,
    timed_out: bool,
    body_missing: bool,
}

impl<'p> Reading<'p> {
    pub(super) fn new(page: &'p mut Page) -> Self {
        Self {
            session: PageReading::new(page),
            facts: PageFacts::default(),
            timed_out: false,
            body_missing: false,
        }
    }

    pub(super) fn page(&mut self) -> &mut Page {
        self.session.page()
    }

    pub(super) fn exchanges(&self) -> &[Exchange] {
        self.session.exchanges()
    }

    /// Opens `url` and waits until its document has loaded, at most until the read deadline.
    pub(super) async fn navigate(&mut self, url: &str) -> Result<(), AttemptFailure> {
        self.session.navigate(url).await
    }

    /// Takes what capture holds and reads the page facts; returns whether they end the read.
    ///
    /// # Errors
    ///
    /// Returns a transport failure, or a page script failure that lasted several reads.
    pub(super) async fn poll(&mut self) -> Result<bool, AttemptFailure> {
        absorb(&mut self.facts, self.session.capture()?);
        if let Some(state) = self
            .session
            .evaluate::<PageState>(&page_script(PAGE_STATE))
            .await?
        {
            self.facts.load_error = state
                .load_error
                .or_else(|| self.session.blank_page_error(&state.url));
            self.facts.url = state.url;
            self.facts.title = state.title;
            self.facts.error_code = state.error_code;
            self.facts.notice = state.notice;
            if state.logged_out {
                self.facts.guest = true;
            }
        }
        Ok(self.ended())
    }

    /// Whether the page facts end the read: a logged-out session, a block page, a notice, risk
    /// control, or a failed API answer.
    pub(super) fn ended(&self) -> bool {
        let facts = &self.facts;
        facts.guest
            || facts.error_code.is_some()
            || facts.notice.is_some()
            || facts.blocked_status.is_some()
            || facts.api_error.is_some()
    }

    fn completions(&self, tracked: Tracked) -> usize {
        tracked.completed(self.exchanges()).count()
    }

    /// Waits while the browser shows its own error page for the last navigation, which it
    /// reloads by itself; returns whether the read can go on: the page loaded before the read
    /// deadline and its facts do not end the read.
    pub(super) async fn loaded(&mut self) -> Result<bool, AttemptFailure> {
        loop {
            if self.poll().await? {
                return Ok(false);
            }
            if !self.page().reading() {
                self.timed_out = true;
                return Ok(false);
            }
            if self.facts.load_error.is_none() {
                return Ok(true);
            }
            self.page().rest(POLL).await;
        }
    }

    /// Waits for a response to `tracked` after the `seen` completed ones, scrolling to the end
    /// of the list every few seconds when `scroll` is set; returns whether the read can go on.
    /// A response without a body, the page facts, or the read deadline end the read.
    pub(super) async fn next(
        &mut self,
        tracked: Tracked,
        seen: usize,
        scroll: bool,
    ) -> Result<bool, AttemptFailure> {
        self.wait(tracked, Awaited::After(seen), scroll).await
    }

    /// Waits for the answer to an action: a response to `tracked` whose request the page sent
    /// once `sent` requests had gone out (see [`Page::requests_sent`]); returns whether the read
    /// can go on, as [`Reading::next`] does. An older request that ends after the action is no
    /// answer to it.
    pub(super) async fn answer(
        &mut self,
        tracked: Tracked,
        sent: u64,
    ) -> Result<bool, AttemptFailure> {
        self.wait(tracked, Awaited::SentFrom(sent), false).await
    }

    async fn wait(
        &mut self,
        tracked: Tracked,
        awaited: Awaited,
        scroll: bool,
    ) -> Result<bool, AttemptFailure> {
        let started = Instant::now();
        let mut scrolled: Option<Instant> = None;
        while self.page().reading() {
            if self.poll().await? {
                return Ok(false);
            }
            let arrived = awaited.among(tracked, self.exchanges());
            if !arrived.is_empty() {
                let held = arrived
                    .iter()
                    .any(|exchange| matches!(exchange.body, ResponseBody::Received(_)));
                self.body_missing |= !held;
                return Ok(held);
            }
            let due = scrolled.map_or(started.elapsed() >= FIRST_SCROLL_AFTER, |at| {
                at.elapsed() >= SCROLL_EVERY
            });
            if scroll && due {
                scrolled = Some(Instant::now());
                if !self.scroll_to_end().await? {
                    break;
                }
                continue;
            }
            self.page().rest(POLL).await;
        }
        self.poll().await?;
        self.timed_out = true;
        Ok(false)
    }

    /// Scrolls until one more response to `tracked` arrives; returns whether the read can go
    /// on.
    pub(super) async fn more(&mut self, tracked: Tracked) -> Result<bool, AttemptFailure> {
        let seen = self.completions(tracked);
        self.next(tracked, seen, true).await
    }

    /// Waits until no response to `tracked` has arrived for a moment, as after the first screen
    /// of results, which requests its second page by itself; returns whether the read can go on.
    pub(super) async fn settle(&mut self, tracked: Tracked) -> Result<bool, AttemptFailure> {
        let mut seen = self.completions(tracked);
        let mut quiet = Instant::now();
        while self.page().reading() {
            if self.poll().await? {
                return Ok(false);
            }
            let completions = self.completions(tracked);
            if completions > seen {
                seen = completions;
                quiet = Instant::now();
            } else if quiet.elapsed() >= SETTLE {
                return Ok(true);
            }
            self.page().rest(POLL).await;
        }
        self.timed_out = true;
        Ok(false)
    }

    /// Polls until the page facts end the read, a response to `tracked` whose request was sent
    /// once `sent` requests had gone out arrives, or the read-only page `check` holds, for at
    /// most `within`; returns whether one did.
    pub(super) async fn confirm(
        &mut self,
        tracked: Tracked,
        sent: u64,
        check: &str,
        within: Duration,
    ) -> Result<bool, AttemptFailure> {
        let until = Deadline::new(within);
        loop {
            if self.poll().await?
                || !Awaited::SentFrom(sent)
                    .among(tracked, self.exchanges())
                    .is_empty()
            {
                return Ok(true);
            }
            let slice = POLL.min(until.remaining().unwrap_or_default());
            match self.page().confirm(check, slice).await {
                Ok(()) => return Ok(true),
                Err(InputError::Transport(failure)) => return Err(failure),
                Err(InputError::Failed(_)) => {}
            }
            if until.remaining().is_none_or(|left| left.is_zero()) || !self.page().reading() {
                return Ok(false);
            }
        }
    }

    /// Turns the wheel over the list the page shows toward its end. Returns `false` once the
    /// read deadline has passed.
    async fn scroll_to_end(&mut self) -> Result<bool, AttemptFailure> {
        let plan: ScrollPlan = self.page().evaluate(&page_script(SCROLL_PLAN)).await?;
        if plan.distance < 1.0 {
            return Ok(true);
        }
        match input_failure(self.page().wheel((plan.x, plan.y), plan.distance).await)? {
            Some(missed) if missed.failure == InputFailure::DeadlinePassed => Ok(false),
            // A wheel turn that found no target leaves the next poll to turn it again.
            _ => Ok(true),
        }
    }

    /// A rendered note is the expected page: text in the note or its comments is no notice.
    pub(super) fn clear_notice(&mut self) {
        self.facts.notice = None;
    }

    pub(super) fn mark_timed_out(&mut self) {
        self.timed_out = true;
    }

    /// Takes the exchanges still in the capture and returns what the read observed.
    ///
    /// # Errors
    ///
    /// Returns a transport failure.
    pub(super) fn finish(mut self) -> Result<Observed, AttemptFailure> {
        absorb(&mut self.facts, self.session.capture()?);
        Ok(Observed {
            page: self.facts,
            exchanges: self.session.into_exchanges(),
            timed_out: self.timed_out,
            body_missing: self.body_missing,
        })
    }
}

#[cfg(test)]
mod tests {
    use reqwest::Url;
    use serde_json::{Value, json};

    use super::{ACCOUNT_PATH, PageFacts, absorb};
    use crate::providers::browser::{Exchange, RequestMethod, ResponseBody};
    use crate::samples::sample_json;

    const SEARCH_PATH: &str = "/api/sns/web/v1/search/notes";
    const ONEBOX_PATH: &str = "/api/sns/web/v1/search/onebox";

    /// The stored `search/notes` failure answer with `code` and `msg`.
    fn failure(code: i64, msg: &str) -> Value {
        let mut answer = sample_json("xiaohongshu_browser/search-error.json");
        answer["code"] = json!(code);
        answer["msg"] = json!(msg);
        answer
    }

    fn exchange(path: &str, status: u16, body: Option<&Value>) -> Exchange {
        Exchange {
            sequence: 0,
            method: RequestMethod::Get,
            url: Url::parse(&format!(
                "https://edith.xiaohongshu.com{path}?xsec_token=ABsecret="
            ))
            .expect("site URL"),
            status: Some(status),
            request_body: None,
            body: body.map_or(ResponseBody::Missing, |body| {
                ResponseBody::Received(body.to_string())
            }),
        }
    }

    fn absorbed(exchanges: &[Exchange]) -> PageFacts {
        let mut facts = PageFacts::default();
        absorb(&mut facts, exchanges);
        facts
    }

    #[test]
    fn an_account_answer_with_a_login_code_or_a_guest_is_a_logged_out_session() {
        let mut expired = sample_json("xiaohongshu_browser/account-logged-out.json");
        expired["code"] = json!(-100);
        let mut guest = sample_json("xiaohongshu_browser/account.json");
        guest["data"]["guest"] = json!(true);
        let answers = [
            sample_json("xiaohongshu_browser/account-logged-out.json"),
            expired,
            guest,
        ];

        let guests =
            answers.map(|answer| absorbed(&[exchange(ACCOUNT_PATH, 200, Some(&answer))]).guest);

        assert_eq!(guests, [true, true, true]);
    }

    #[test]
    fn a_logged_in_account_answer_is_no_page_fact() {
        let facts = absorbed(&[exchange(
            ACCOUNT_PATH,
            200,
            Some(&sample_json("xiaohongshu_browser/account.json")),
        )]);

        assert_eq!(
            (facts.guest, facts.blocked_status, facts.api_error.is_none()),
            (false, None, true)
        );
    }

    #[test]
    fn an_api_answer_with_a_login_code_is_a_logged_out_session() {
        let facts = absorbed(&[exchange(
            SEARCH_PATH,
            200,
            Some(&sample_json("xiaohongshu_browser/search-error.json")),
        )]);

        assert_eq!((facts.guest, facts.api_error.is_none()), (true, true));
    }

    #[test]
    fn the_first_failure_code_of_an_api_answer_is_kept_with_its_path_and_message() {
        let first = failure(300_031, "当前笔记暂时无法浏览");
        let second = failure(999_999, "未知错误");

        let facts = absorbed(&[
            exchange(SEARCH_PATH, 200, Some(&first)),
            exchange(SEARCH_PATH, 200, Some(&second)),
        ]);

        let api = facts.api_error.expect("a failed API answer");
        assert_eq!(
            (api.path.as_str(), api.code, api.msg.as_deref()),
            (SEARCH_PATH, 300_031, Some("当前笔记暂时无法浏览"))
        );
    }

    #[test]
    fn a_zero_code_is_no_failure_even_without_success() {
        let facts = absorbed(&[exchange(
            ONEBOX_PATH,
            200,
            Some(&sample_json("xiaohongshu_browser/onebox.json")),
        )]);

        assert!(facts.api_error.is_none(), "{:?}", facts.api_error);
    }

    #[test]
    fn a_verification_status_on_any_site_request_is_risk_control() {
        let statuses = [461, 471, 403].map(|status| {
            absorbed(&[exchange("/api/sns/web/v1/feed", status, None)]).blocked_status
        });

        assert_eq!(statuses, [Some(461), Some(471), None]);
    }
}
