// Shared deadlines, page helpers, network capture reads, read sessions, page facts, and output
// envelopes for the forager Xiaohongshu commands. The commands only operate the page as a user
// would and report what the page itself requested and received; forager judges the facts.

export const CONTRACT = 'forager-xhs/1';
export const SITE = 'forager-xhs';
export const SITE_DOMAIN = 'www.xiaohongshu.com';

// OpenCLI closes the tab after the command returns, so the page reads stop this long before
// the timeout that forager passes.
const CLOSE_RESERVE_MS = 3000;
export const POLL_MS = 500;
const SCROLL_EVERY_MS = 4000;
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

/** Copies the listed keys that `object` defines. Also injected into the page scripts. */
export function pick(object, keys) {
  const picked = {};
  for (const key of keys) if (object && object[key] !== undefined) picked[key] = object[key];
  return picked;
}

// The helpers every page script can use. The site keeps its state in Vue refs, which hold their
// value in `value` or `_value`.
const PAGE_HELPERS = `
  ${pick}
  const unwrap = (value) => (value && typeof value === 'object' ? ('value' in value ? value.value : value._value) : value);
  const pageState = () => window.__INITIAL_STATE__;
  const noteDetailMap = () => {
    const state = pageState();
    return state && state.note && state.note.noteDetailMap;
  };`;

/** Wraps a script body to evaluate in the page, with the shared page helpers in scope. */
export function pageScript(body) {
  return `(() => {${PAGE_HELPERS}\n${body}\n})()`;
}

/** Opens a note page the way a search result link does, with the note's access token. */
export async function openNote(page, id, token) {
  const params = new URLSearchParams({ xsec_token: String(token), xsec_source: 'pc_search' });
  await page.goto(`https://${SITE_DOMAIN}/explore/${encodeURIComponent(id)}?${params}`, {
    waitUntil: 'load',
    settleMs: 1000,
  });
}

// The page state that ends any read: login walls, block pages, and their notices. A notice is
// read only while no note card and no note is shown, so text in a note title, a note, or its
// comments never counts as one.
//
// Each read also makes sure the current document counts the completed requests to the tracked
// paths on any xiaohongshu.com host. A document keeps its own count, and the page the command
// opened is not always the one that answers: when the first load fails, Chrome shows its own
// error page and reloads it into a new document. Resource Timing records a request only once its
// response has ended.
function pageStateScript(tracked) {
  return pageScript(`
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
  const notes = noteDetailMap();
  const noteShown = Boolean(notes) && Object.values(notes).some((entry) => entry && entry.note && entry.note.noteId);
  const rendered = cards > 0 || noteShown;
  const text = document.body && !loadError ? document.body.innerText : '';
  const notice = rendered
    ? null
    : (text.match(/[^\\n]*(安全限制|访问链接异常|登录后查看|请求太频繁|访问频次异常)[^\\n]*/) || [null])[0];
  const state = pageState();
  const loggedIn = unwrap(state && state.user ? state.user.loggedIn : undefined);
  return {
    url: url.href,
    title: document.title,
    error_code: errorPage ? url.searchParams.get('error_code') : null,
    notice: notice ? notice.trim().slice(0, 120) : null,
    load_error: loadError,
    logged_out: loggedIn === false && !rendered,
    cards,
    counts: window.__foragerXhs || {},
  };`);
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
 * Drains the network capture into exchanges `{path, query, method, status, request, body,
 * has_body}`; `query` never holds an access token. OPTIONS preflights and other methods are
 * dropped.
 */
export async function readExchanges(page) {
  const entries = await page.readNetworkCapture();
  return entries
    .filter((entry) => entry.method === 'GET' || entry.method === 'POST')
    .map((entry) => {
      let path = '';
      const query = {};
      try {
        const url = new URL(entry.url);
        path = url.pathname;
        for (const [name, value] of url.searchParams) if (name !== 'xsec_token') query[name] = value;
      } catch { /* keep empty */ }
      return {
        path,
        query,
        method: entry.method,
        status: entry.responseStatus || null,
        request: decodeBody(entry.requestBodyPreview),
        body: decodeBody(entry.responsePreview),
        has_body: typeof entry.responsePreview === 'string' && entry.responsePreview !== '',
      };
    });
}

// A hidden background window renders no frame, so scrolling alone never loads the next page;
// a screenshot forces one. A note page lists its comments in its own scrolling panel.
const SCROLL_TO_END = `(() => {
  const scroller = document.querySelector('.note-scroller');
  if (scroller) scroller.scrollTop = scroller.scrollHeight;
  window.scrollTo(0, document.documentElement.scrollHeight);
})()`;

async function scrollToEnd(page) {
  await page.evaluate(SCROLL_TO_END);
  await page.screenshot();
  await page.screenshot();
}

/**
 * Waits until `path` has completed more than `seen` times, the page reaches a final state, or
 * the deadline passes, scrolling to the end every few seconds when `scroll` is set. `ready`, when
 * given, is a page state that also counts as the completion, such as rendered note cards: the
 * first response of a page usually completes before the navigation returns and the count
 * starts, and the page clears its Resource Timing buffer at its load event. Returns the exchanges
 * read after the completion with `completed`, or with `body_missing` when none of them holds a
 * body for `path`; `ended`; or `timed_out`. Exchanges for `path` without a body are other
 * requests still in flight or aborted, not the completed one.
 */
async function awaitCompletion(page, facts, { path, seen, deadline, scroll, ready }) {
  let scrolledAt = 0;
  while (Date.now() < deadline) {
    if (await facts.observe(page)) return { state: 'ended', exchanges: [] };
    if (facts.completions(path) > seen || (ready && await ready())) {
      // Reading the capture drains it, and an entry drained before the extension stores its
      // body never gets one. The extension asks Chrome for the body on the request's
      // loadingFinished event, which Chrome sends before the page can see the completion, and
      // a tab answers DevTools commands in order. One more page round trip therefore returns
      // only after the body request has been answered.
      await facts.observe(page);
      const exchanges = await readExchanges(page);
      facts.absorb(exchanges);
      const held = exchanges.some((exchange) => exchange.path === path && exchange.has_body);
      return { state: held ? 'completed' : 'body_missing', exchanges };
    }
    if (scroll && Date.now() - scrolledAt >= SCROLL_EVERY_MS) {
      scrolledAt = Date.now();
      await scrollToEnd(page);
    }
    await sleep(POLL_MS);
  }
  const exchanges = await readExchanges(page);
  facts.absorb(exchanges);
  await facts.observe(page);
  return { state: 'timed_out', exchanges };
}

/**
 * One command's read of the page: it waits for responses to the tracked `paths`, hands every
 * exchange it reads to `keep`, and reports what ended the read. `fields` returns the command's
 * own envelope fields when the read finishes.
 */
export class ReadSession {
  constructor(page, { paths, deadline, keep, fields }) {
    this.page = page;
    this.deadline = deadline;
    this.keep = keep;
    this.fields = fields;
    this.facts = new PageFacts(paths);
    this.timedOut = false;
    this.bodyMissing = false;
  }

  /** The completed requests to `path` the page has counted. */
  async completions(path) {
    await this.facts.observe(this.page);
    return this.facts.completions(path);
  }

  /**
   * Waits for a response to `path` after the `seen` completed ones, with the `ready` and
   * `scroll` of `awaitCompletion`; returns whether the command can go on.
   */
  async next(path, seen, { ready = null, scroll = false } = {}) {
    const result = await awaitCompletion(this.page, this.facts, {
      path, seen, deadline: this.deadline, scroll, ready,
    });
    this.keep(result.exchanges);
    if (result.state === 'body_missing') this.bodyMissing = true;
    if (result.state === 'timed_out') this.timedOut = true;
    return result.state === 'completed' && !this.facts.ended();
  }

  /** Scrolls until one more response to `path` completes; returns whether the command can go on. */
  async more(path) {
    return this.next(path, await this.completions(path), { scroll: true });
  }

  /** Reads the exchanges still in the capture and returns the envelope. */
  async finish() {
    const exchanges = await readExchanges(this.page);
    this.facts.absorb(exchanges);
    this.keep(exchanges);
    return envelope({
      page: this.facts.facts,
      timed_out: this.timedOut,
      body_missing: this.bodyMissing,
      ...this.fields(),
    });
  }
}
