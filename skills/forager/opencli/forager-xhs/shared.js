// Shared deadlines, network capture reads, page facts, and output envelopes for the
// forager Xiaohongshu commands. The commands only operate the page as a user would and report
// what the page itself requested and received; forager judges the facts.

export const CONTRACT = 'forager-xhs/1';
export const SITE = 'forager-xhs';
export const SITE_DOMAIN = 'www.xiaohongshu.com';

// OpenCLI closes the tab after the command returns, so the page reads stop this long before
// the timeout that forager passes.
const CLOSE_RESERVE_MS = 3000;
const POLL_MS = 500;
// The extension stores a response body shortly after the request completes; reading the
// capture drains it, so a read must not overtake that store.
const BODY_GRACE_MS = 1000;
const RISK_CONTROL_STATUS = 461;
const USER_ME_PATH = '/api/sns/web/v2/user/me';

export function envelope(data) {
  return { contract: CONTRACT, status: 'ok', data };
}

/** Returns the time at which page reads must stop, from the `--timeout` seconds. */
export function readDeadline(timeoutSeconds) {
  return Date.now() + Math.max(1000, Number(timeoutSeconds) * 1000 - CLOSE_RESERVE_MS);
}

export function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

// The page state that ends any read: login walls, block pages, and their notices. A notice is
// read only while no note card is shown, so a note title never counts as one.
//
// Each read also makes sure the current document counts the completed requests to the tracked
// paths on any xiaohongshu.com host. A document keeps its own count, and the page the command
// opened is not always the one that answers: when the first load fails, Chrome shows its own
// error page and reloads it into a new document. Resource Timing records a request only once its
// response has ended, which is the signal that the capture holds its body.
function pageStateScript(tracked) {
  return `(() => {
  const tracked = ${JSON.stringify(tracked)};
  if (!window.__foragerXhs && tracked.length > 0) {
    const counts = {};
    for (const path of tracked) counts[path] = 0;
    window.__foragerXhs = counts;
    new PerformanceObserver((list) => {
      for (const entry of list.getEntries()) {
        let url;
        try { url = new URL(entry.name); } catch { continue; }
        if (url.hostname.endsWith('xiaohongshu.com') && url.pathname in counts && entry.responseEnd > 0) {
          counts[url.pathname] += 1;
        }
      }
    }).observe({ type: 'resource', buffered: true });
  }
  // Chrome's own error page for a navigation that failed, such as ERR_CONNECTION_CLOSED. Its
  // location is chrome-error://chromewebdata/; the navigation entry keeps the URL it tried.
  const loadError = location.protocol === 'chrome-error:'
    ? ((document.querySelector('.error-code') || {}).textContent || '').trim() || 'unknown network error'
    : null;
  const navigation = performance.getEntriesByType('navigation')[0];
  const url = new URL(loadError && navigation ? navigation.name : location.href);
  if (url.searchParams.has('xsec_token')) url.searchParams.delete('xsec_token');
  const errorPage = /^\\/(404|website-login\\/error)/.test(url.pathname);
  const cards = document.querySelectorAll('section.note-item').length;
  const text = document.body && !loadError ? document.body.innerText : '';
  const notice = cards === 0
    ? (text.match(/[^\\n]*(安全限制|访问链接异常|登录后查看|请求太频繁|访问频次异常)[^\\n]*/) || [null])[0]
    : null;
  let loggedIn = window.__INITIAL_STATE__ && window.__INITIAL_STATE__.user
    ? window.__INITIAL_STATE__.user.loggedIn : undefined;
  if (loggedIn && typeof loggedIn === 'object') loggedIn = 'value' in loggedIn ? loggedIn.value : loggedIn._value;
  return {
    url: url.href,
    title: document.title,
    error_code: errorPage ? url.searchParams.get('error_code') : null,
    notice: notice ? notice.trim().slice(0, 120) : null,
    load_error: loadError,
    logged_out: loggedIn === false && cards === 0,
    cards,
    counts: window.__foragerXhs || {},
  };
})()`;
}

/** Collects the facts about where the page is and what the site said there. */
export class PageFacts {
  /** `tracked` lists the request paths whose completions the page counts. */
  constructor(tracked = []) {
    this.script = pageStateScript(tracked);
    this.facts = {
      url: '', title: '', guest: false, error_code: null, notice: null, blocked_status: null, load_error: null,
    };
    this.counts = {};
    this.cards = 0;
  }

  /** Reads the page state; returns whether it ends the command. */
  async observe(page) {
    const state = await page.evaluate(this.script);
    this.counts = state.counts || {};
    this.cards = state.cards;
    Object.assign(this.facts, {
      url: state.url,
      title: state.title,
      error_code: state.error_code,
      notice: state.notice,
      load_error: state.load_error,
    });
    if (state.logged_out) this.facts.guest = true;
    return this.ended();
  }

  /** Records the facts that captured exchanges show: a guest session and risk control. */
  absorb(exchanges) {
    for (const exchange of exchanges) {
      if (exchange.status === RISK_CONTROL_STATUS) this.facts.blocked_status = RISK_CONTROL_STATUS;
      if (exchange.path === USER_ME_PATH && exchange.body && exchange.body.data
        && exchange.body.data.guest === true) {
        this.facts.guest = true;
      }
    }
  }

  ended() {
    const { guest, error_code: errorCode, notice, blocked_status: blocked } = this.facts;
    return guest || Boolean(errorCode) || Boolean(notice) || blocked !== null;
  }

  completions(path) {
    return Number(this.counts[path] || 0);
  }
}

function decodeBody(text) {
  if (typeof text !== 'string' || text === '') return null;
  const raw = text.startsWith('base64:') ? Buffer.from(text.slice(7), 'base64').toString('utf8') : text;
  try {
    return JSON.parse(raw);
  } catch {
    return null;
  }
}

/**
 * Drains the network capture into exchanges `{path, method, status, request, body, has_body}`.
 * OPTIONS preflights and other methods are dropped.
 */
export async function readExchanges(page) {
  const entries = await page.readNetworkCapture();
  return entries
    .filter((entry) => entry.method === 'GET' || entry.method === 'POST')
    .map((entry) => {
      let path = '';
      try { path = new URL(entry.url).pathname; } catch { /* keep empty */ }
      return {
        path,
        method: entry.method,
        status: entry.responseStatus || null,
        request: decodeBody(entry.requestBodyPreview),
        body: decodeBody(entry.responsePreview),
        has_body: typeof entry.responsePreview === 'string' && entry.responsePreview !== '',
      };
    });
}

/**
 * Waits until `path` has completed more than `seen` times, the page reaches a final state, or
 * the deadline passes. `nudge` runs before each wait round, for example to scroll. With
 * `orRendered`, note cards on the page also count as the completion: the first response of a
 * page usually completes before the navigation returns and the count starts, and the page clears
 * its Resource Timing buffer at its load event. Returns the exchanges read after the completion,
 * `ended`, or `timed_out`.
 */
export async function awaitCompletion(page, facts, { path, seen, deadline, nudge, nudgeEveryMs, orRendered }) {
  let nudgedAt = 0;
  while (Date.now() < deadline) {
    if (await facts.observe(page)) return { state: 'ended', exchanges: [] };
    if (facts.completions(path) > seen || (orRendered && facts.cards > 0)) {
      await sleep(BODY_GRACE_MS);
      const exchanges = await readExchanges(page);
      facts.absorb(exchanges);
      return { state: 'completed', exchanges };
    }
    if (nudge && Date.now() - nudgedAt >= nudgeEveryMs) {
      nudgedAt = Date.now();
      await nudge();
    }
    await sleep(POLL_MS);
  }
  const exchanges = await readExchanges(page);
  facts.absorb(exchanges);
  await facts.observe(page);
  return { state: 'timed_out', exchanges };
}
