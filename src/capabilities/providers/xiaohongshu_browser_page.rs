//! Page driving shared by the Xiaohongshu operations: what passive capture keeps, the page-facts
//! script, scrolling to the end of a list, and the read that waits for the page's own
//! responses and records why it stopped.

use std::time::{Duration, Instant};

use reqwest::Url;
use serde::Deserialize;

use super::{ACCOUNT_PATH, PageFacts, RISK_CONTROL_STATUS, on_site};
use crate::net::AttemptFailure;
use crate::providers::browser::{
    CaptureLevel, Exchange, InputError, InputFailure, Page, RequestMethod, ResponseBody,
};

const POLL: Duration = Duration::from_millis(500);
// A list loads its next page by itself a moment after the last one, so the first scroll waits.
const FIRST_SCROLL_AFTER: Duration = Duration::from_millis(1500);
const SCROLL_EVERY: Duration = Duration::from_secs(4);
// How long no new response must arrive before the page counts as settled.
const SETTLE: Duration = Duration::from_millis(1500);
// A page that is navigating briefly answers no script.
const MAX_UNANSWERED: u32 = 5;

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

/// Turns the outcome of a native action into the reason it did not happen, or a transport
/// failure.
pub(super) fn input_reason(
    result: Result<(), InputError>,
) -> Result<Option<&'static str>, AttemptFailure> {
    match result {
        Ok(()) => Ok(None),
        Err(InputError::Failed(failure)) => Ok(Some(failure.reason())),
        Err(InputError::Transport(failure)) => Err(failure),
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
    page: &'p mut Page,
    facts: PageFacts,
    exchanges: Vec<Exchange>,
    /// The network error of a navigation that never loaded a document.
    navigation_error: Option<String>,
    unanswered: u32,
    timed_out: bool,
    body_missing: bool,
}

impl<'p> Reading<'p> {
    pub(super) fn new(page: &'p mut Page) -> Self {
        Self {
            page,
            facts: PageFacts::default(),
            exchanges: Vec::new(),
            navigation_error: None,
            unanswered: 0,
            timed_out: false,
            body_missing: false,
        }
    }

    pub(super) fn page(&mut self) -> &mut Page {
        self.page
    }

    pub(super) fn exchanges(&self) -> &[Exchange] {
        &self.exchanges
    }

    /// Opens `url` and waits until its document has loaded, at most until the read deadline.
    pub(super) async fn navigate(&mut self, url: &str) -> Result<(), AttemptFailure> {
        let navigation = self.page.navigate(url).await?;
        self.navigation_error = navigation
            .error
            .filter(|_| !navigation.loaded)
            .map(|error| error.trim_start_matches("net::").to_owned());
        Ok(())
    }

    /// Takes what capture holds and reads the page facts; returns whether they end the read.
    ///
    /// # Errors
    ///
    /// Returns a transport failure, or a page script failure that lasted several reads.
    pub(super) async fn poll(&mut self) -> Result<bool, AttemptFailure> {
        let exchanges = self.page.take_exchanges()?;
        self.absorb(&exchanges);
        self.exchanges.extend(exchanges);
        match self
            .page
            .evaluate::<PageState>(&page_script(PAGE_STATE))
            .await
        {
            Ok(state) => {
                self.unanswered = 0;
                let blank = !state.url.starts_with("http");
                self.facts.url = state.url;
                self.facts.title = state.title;
                self.facts.error_code = state.error_code;
                self.facts.notice = state.notice;
                // A navigation that failed before the browser showed its error page leaves the
                // tab blank, where only the navigation result names the error.
                self.facts.load_error = state
                    .load_error
                    .or_else(|| self.navigation_error.clone().filter(|_| blank));
                if state.logged_out {
                    self.facts.guest = true;
                }
            }
            Err(failure) if self.page.closed() || self.unanswered >= MAX_UNANSWERED => {
                return Err(failure);
            }
            Err(_) => self.unanswered += 1,
        }
        Ok(self.ended())
    }

    /// Records what captured exchanges show: risk control and a guest session.
    fn absorb(&mut self, exchanges: &[Exchange]) {
        for exchange in exchanges {
            if exchange.status == Some(RISK_CONTROL_STATUS) {
                self.facts.blocked_status = Some(RISK_CONTROL_STATUS);
            }
            if exchange.url.path() == ACCOUNT_PATH
                && let ResponseBody::Received(body) = &exchange.body
                && serde_json::from_str::<serde_json::Value>(body)
                    .is_ok_and(|account| account["data"]["guest"] == true)
            {
                self.facts.guest = true;
            }
        }
    }

    /// Whether the page facts end the read: a logged-out session, a block page, a notice, or
    /// risk control.
    pub(super) fn ended(&self) -> bool {
        let facts = &self.facts;
        facts.guest
            || facts.error_code.is_some()
            || facts.notice.is_some()
            || facts.blocked_status.is_some()
    }

    /// The responses to `tracked` that finished loading, with or without a body.
    fn completed(&self, tracked: Tracked) -> impl Iterator<Item = &Exchange> {
        self.exchanges.iter().filter(move |exchange| {
            tracked.matches(exchange)
                && matches!(
                    exchange.body,
                    ResponseBody::Received(_) | ResponseBody::Missing
                )
        })
    }

    pub(super) fn completions(&self, tracked: Tracked) -> usize {
        self.completed(tracked).count()
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
        let started = Instant::now();
        let mut scrolled: Option<Instant> = None;
        while self.page.reading() {
            if self.poll().await? {
                return Ok(false);
            }
            if self.completions(tracked) > seen {
                let held = self
                    .completed(tracked)
                    .skip(seen)
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
            self.page.rest(POLL).await;
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
        while self.page.reading() {
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
            self.page.rest(POLL).await;
        }
        self.timed_out = true;
        Ok(false)
    }

    /// Polls until the page facts end the read, a response to `tracked` after `seen` arrives,
    /// or the read-only page `check` holds, for at most `within`; returns whether one did.
    pub(super) async fn confirm(
        &mut self,
        tracked: Tracked,
        seen: usize,
        check: &str,
        within: Duration,
    ) -> Result<bool, AttemptFailure> {
        let started = Instant::now();
        while self.page.reading() {
            if self.poll().await?
                || self.completions(tracked) > seen
                || self.page.evaluate::<bool>(check).await.unwrap_or(false)
            {
                return Ok(true);
            }
            if started.elapsed() >= within {
                return Ok(false);
            }
            self.page.rest(POLL.min(within)).await;
        }
        Ok(false)
    }

    /// Turns the wheel over the list the page shows toward its end. Returns `false` once the
    /// read deadline has passed.
    async fn scroll_to_end(&mut self) -> Result<bool, AttemptFailure> {
        let plan: ScrollPlan = self.page.evaluate(&page_script(SCROLL_PLAN)).await?;
        if plan.distance < 1.0 {
            return Ok(true);
        }
        match self.page.wheel((plan.x, plan.y), plan.distance).await {
            Err(InputError::Failed(InputFailure::DeadlinePassed)) => Ok(false),
            // A wheel turn that found no target leaves the next poll to turn it again.
            Ok(()) | Err(InputError::Failed(_)) => Ok(true),
            Err(InputError::Transport(failure)) => Err(failure),
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
        let exchanges = self.page.take_exchanges()?;
        self.absorb(&exchanges);
        self.exchanges.extend(exchanges);
        Ok(Observed {
            page: self.facts,
            exchanges: self.exchanges,
            timed_out: self.timed_out,
            body_missing: self.body_missing,
        })
    }
}
