// Shared deadlines, page helpers, network capture reads, read sessions, page facts, native input,
// and output envelopes for the forager Xiaohongshu commands. The commands only operate the page
// as a user would and report what the page itself requested and received; forager judges the
// facts.

export const CONTRACT = 'forager-xhs/2';
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
  return `(async () => {${PAGE_HELPERS}\n${body}\n})()`;
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

// Where the wheel turns to reach the end of the page, and how far that is. A note page lists its
// comments in its own scrolling panel; a results page scrolls as a whole.
const SCROLL_PLAN = pageScript(`
  const width = document.documentElement.clientWidth;
  const height = document.documentElement.clientHeight;
  const panel = [...document.querySelectorAll('.note-scroller')].find((element) => {
    const rect = element.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0;
  });
  if (panel) {
    const rect = panel.getBoundingClientRect();
    const left = Math.max(rect.left, 0);
    const right = Math.min(rect.right, width);
    const top = Math.max(rect.top, 0);
    const bottom = Math.min(rect.bottom, height);
    if (right > left && bottom > top) {
      return {
        x: (left + right) / 2,
        y: (top + bottom) / 2,
        distance: panel.scrollHeight - panel.clientHeight - panel.scrollTop,
      };
    }
  }
  const root = document.scrollingElement || document.documentElement;
  return { x: width / 2, y: height / 2, distance: root.scrollHeight - root.clientHeight - root.scrollTop };
`);

// A hidden background window renders no frame, so scrolling alone never loads the next page;
// a screenshot forces one.
async function scrollToEnd(page, deadline) {
  requireNativeInput(page, ['cdp', 'screenshot']);
  await humanPause(deadline);
  const plan = await page.evaluate(SCROLL_PLAN);
  // With the native input checked above, the only failure left is the deadline, which ends
  // the wait in `awaitCompletion` as a timeout.
  if (plan.distance > 0 && await inputFailureOf(() => wheelAt(page, plan, plan.distance, { deadline }))) return;
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
      await scrollToEnd(page, deadline);
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

// ---- Native input -------------------------------------------------------------------------------
// forager-xhs, forager-gemini and forager-chatgpt each carry an identical copy of this block,
// because OpenCLI loads every adapter directory on its own; change all three copies together.
//
// Every action that changes the page (pointer moves, clicks, wheel scrolls, keys, and text) goes
// through Chrome's input pipeline as CDP `Input.*` events, so the page sees trusted events with
// coordinates and a full pointer sequence. Reading the DOM, network capture, and screenshots are
// not actions. An action that cannot be done natively, or not before the command's deadline,
// throws `InputFailure` with its reason and never falls back to a scripted one; OpenCLI's
// `page.click`, `page.hover`, `page.typeText`, and `page.pressKey` are avoided because they
// silently fall back to `element.click()`, `dispatchEvent`, DOM setters, or script key events. The pacing is a person's: a pause before
// each action, a hover before each click, and short text typed one character at a time.

const INPUT_PAUSE_MS = [300, 1500];
const HOVER_DWELL_MS = [80, 250];
const KEY_GAP_MS = [30, 90];
const WHEEL_GAP_MS = [30, 90];
const WHEEL_STEP_PX = [300, 500];
const MAX_WHEEL_STEPS = 12;
const MAX_TYPED_CHARACTERS = 200;
// A target may render a moment after the action that reveals it.
const LOCATE_MS = 2000;
const LOCATE_POLL_MS = 200;
const MAX_SCROLLS_INTO_VIEW = 8;
// How far from the center a click may land, as a fraction of the target's size.
const CLICK_JITTER = 0.25;
const IS_MAC = process.platform === 'darwin';
// CDP modifier bits: Control 2, Meta 4.
const SHORTCUT_MODIFIER = IS_MAC
  ? { bit: 4, key: 'Meta', code: 'MetaLeft', keyCode: 91 }
  : { bit: 2, key: 'Control', code: 'ControlLeft', keyCode: 17 };
// Chrome on macOS performs editing keys only through the `commands` it is sent with them.
const NATIVE_KEYS = {
  selectAll: { key: 'a', code: 'KeyA', keyCode: 65, command: 'selectAll', shortcut: true },
  backspace: { key: 'Backspace', code: 'Backspace', keyCode: 8, command: 'deleteBackward' },
  delete: { key: 'Delete', code: 'Delete', keyCode: 46, command: 'deleteForward' },
  escape: { key: 'Escape', code: 'Escape', keyCode: 27 },
};

/** A page action that could not be done natively; `reason` says why. */
export class InputFailure extends Error {
  constructor(reason) {
    super(reason);
    this.name = 'InputFailure';
    this.reason = reason;
  }
}

function randomBetween([low, high]) {
  return low + Math.random() * (high - low);
}

/** Throws `InputFailure` once `deadline` has passed, so that no input goes out after it. */
function requireTime(deadline) {
  if (Date.now() >= deadline) throw new InputFailure('deadline_passed');
}

/** Waits `ms`, but never past `deadline`. */
function restFor(ms, deadline = Infinity) {
  const capped = Math.max(0, Math.min(ms, deadline - Date.now()));
  return new Promise((resolve) => setTimeout(resolve, capped));
}

/** Throws `InputFailure` unless `page` offers every named method. */
export function requireNativeInput(page, methods) {
  const missing = methods.filter((name) => typeof page[name] !== 'function');
  if (missing.length > 0) {
    throw new InputFailure(`no_native_input (${missing.map((name) => `page.${name}`).join(', ')})`);
  }
}

/** Waits 0.3–1.5 s, as a person does between two actions, but never past `deadline`. */
export function humanPause(deadline = Infinity) {
  return restFor(randomBetween(INPUT_PAUSE_MS), deadline);
}

/**
 * Runs `action`; returns null when it succeeds, or the reason of its `InputFailure`, renamed
 * through `names` when listed there. Other errors propagate.
 */
export async function inputFailureOf(action, names = {}) {
  try {
    await action();
    return null;
  } catch (error) {
    if (!(error instanceof InputFailure)) throw error;
    return names[error.reason] ?? error.reason;
  }
}

/** Polls `check` until it holds or `within` ms pass, never past `deadline`; returns whether it held. */
export async function confirmWithin(check, within, deadline = Infinity) {
  const until = Math.min(Date.now() + within, deadline);
  for (;;) {
    if (await check()) return true;
    if (Date.now() >= until) return false;
    await restFor(LOCATE_POLL_MS, until);
  }
}

// Finds the one target among the elements that the page expression `targets` yields. Sites stack
// invisible decoy copies of buttons, with the same class and text, under the real ones to catch
// scripts, so an element counts only outside `aria-hidden` subtrees, with a size, and when a
// pointer at its center hits it or its descendant. Returns a point on the target that a pointer
// hits, a wheel turn that brings the target into view, or the reason it cannot. With `at`, it
// instead reports whether a pointer at `at` still hits the one target.
function locateScript(targets, at = null) {
  return `(() => {
    const found = [...(${targets})].filter((element) => element instanceof Element);
    const center = (rect) => ({ x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 });
    const hits = (element, x, y) => {
      const hit = document.elementFromPoint(x, y);
      return Boolean(hit) && element.contains(hit);
    };
    const candidates = found.filter((element) => {
      const rect = element.getBoundingClientRect();
      return !element.closest('[aria-hidden="true"]') && rect.width > 0 && rect.height > 0;
    });
    if (candidates.length === 0) return { reason: 'no_target' };
    const hittable = candidates.filter((element) => {
      const point = center(element.getBoundingClientRect());
      return hits(element, point.x, point.y);
    });
    if (hittable.length > 1) return { reason: 'ambiguous_target' };
    const at = ${JSON.stringify(at)};
    if (at) return { hit: hittable.length === 1 && hits(hittable[0], at.x, at.y) };
    if (hittable.length === 1) {
      const rect = hittable[0].getBoundingClientRect();
      const middle = center(rect);
      for (let attempt = 0; attempt < 8; attempt += 1) {
        const x = middle.x + (Math.random() * 2 - 1) * rect.width * ${CLICK_JITTER};
        const y = middle.y + (Math.random() * 2 - 1) * rect.height * ${CLICK_JITTER};
        if (hits(hittable[0], x, y)) return { x, y };
      }
      return middle;
    }
    // No candidate is hit: either it lies outside the visible part of the page, and the wheel
    // over its nearest scrolling ancestor brings it in, or something covers it.
    const view = {
      left: 0, top: 0, right: document.documentElement.clientWidth, bottom: document.documentElement.clientHeight,
    };
    const clip = (area, rect) => ({
      left: Math.max(area.left, rect.left),
      top: Math.max(area.top, rect.top),
      right: Math.min(area.right, rect.right),
      bottom: Math.min(area.bottom, rect.bottom),
    });
    const target = candidates[0];
    let visible = view;
    let scroller = null;
    for (let element = target.parentElement; element && element !== document.body; element = element.parentElement) {
      const style = getComputedStyle(element);
      if (!/(auto|scroll|overlay)/.test(style.overflowY) || element.scrollHeight <= element.clientHeight) continue;
      const rect = element.getBoundingClientRect();
      scroller = scroller || clip(view, rect);
      visible = clip(visible, rect);
    }
    const point = center(target.getBoundingClientRect());
    if (point.x >= visible.left && point.x <= visible.right && point.y >= visible.top && point.y <= visible.bottom) {
      return { reason: 'obscured_target' };
    }
    const wheel = scroller || view;
    if (wheel.right <= wheel.left || wheel.bottom <= wheel.top) return { reason: 'out_of_view' };
    return {
      scroll: {
        x: (wheel.left + wheel.right) / 2,
        y: (wheel.top + wheel.bottom) / 2,
        deltaY: point.y - (visible.top + visible.bottom) / 2,
      },
    };
  })()`;
}

function movePointer(page, point) {
  return page.cdp('Input.dispatchMouseEvent', { type: 'mouseMoved', x: point.x, y: point.y, button: 'none' });
}

/**
 * Scrolls by `deltaY` CSS pixels (at most a few thousand per call) with wheel turns over `point`,
 * which scroll the innermost scrolling area under it. A hidden window applies the scroll only
 * when it renders a frame, which a screenshot forces.
 */
export async function wheelAt(page, point, deltaY, { deadline = Infinity } = {}) {
  requireNativeInput(page, ['cdp']);
  requireTime(deadline);
  await movePointer(page, point);
  let remaining = deltaY;
  for (let step = 0; step < MAX_WHEEL_STEPS && Math.abs(remaining) >= 1; step += 1) {
    if (step > 0) await restFor(randomBetween(WHEEL_GAP_MS), deadline);
    requireTime(deadline);
    const delta = Math.sign(remaining) * Math.min(Math.abs(remaining), randomBetween(WHEEL_STEP_PX));
    await page.cdp('Input.dispatchMouseEvent', {
      type: 'mouseWheel', x: point.x, y: point.y, deltaX: 0, deltaY: delta,
    });
    remaining -= delta;
  }
}

// Returns a point that hits the one target, scrolling it into view first when needed.
async function locateTarget(page, targets, deadline) {
  const started = Date.now();
  let scrolls = 0;
  for (;;) {
    const found = await page.evaluate(locateScript(targets));
    if (found.scroll) {
      if (scrolls >= MAX_SCROLLS_INTO_VIEW) throw new InputFailure('out_of_view');
      scrolls += 1;
      await wheelAt(page, found.scroll, found.scroll.deltaY, { deadline });
      await page.screenshot();
      continue;
    }
    if (!found.reason) return { x: found.x, y: found.y };
    const waited = Date.now() - started >= LOCATE_MS || Date.now() >= deadline;
    if (found.reason === 'ambiguous_target' || waited) throw new InputFailure(found.reason);
    await restFor(LOCATE_POLL_MS, deadline);
  }
}

/**
 * Moves the pointer onto the one target that `targets` (a page expression yielding elements)
 * names, scrolling it into view with the wheel first. Returns the point.
 */
export async function hoverTarget(page, targets, { deadline = Infinity } = {}) {
  requireNativeInput(page, ['cdp', 'screenshot']);
  requireTime(deadline);
  await humanPause(deadline);
  const point = await locateTarget(page, targets, deadline);
  requireTime(deadline);
  await movePointer(page, point);
  return point;
}

/**
 * Clicks the one target that `targets` (a page expression yielding elements) names: scrolls it
 * into view with the wheel, hovers a point near its center that still hits it, and clicks there
 * once the pointer still hits it after the hover. Callers confirm the click's effect on the page.
 */
export async function clickTarget(page, targets, { deadline = Infinity } = {}) {
  requireNativeInput(page, ['cdp', 'screenshot', 'nativeClick']);
  requireTime(deadline);
  await humanPause(deadline);
  const point = await locateTarget(page, targets, deadline);
  requireTime(deadline);
  await movePointer(page, point);
  await restFor(randomBetween(HOVER_DWELL_MS), deadline);
  const still = await page.evaluate(locateScript(targets, point));
  if (!still.hit) throw new InputFailure(still.reason || 'target_moved');
  requireTime(deadline);
  await page.nativeClick(point.x, point.y);
}

/**
 * Types `text` into the focused element: up to 200 characters one at a time, 30–90 ms apart;
 * longer text in one insertion, so that it fits the command's time.
 */
export async function typeText(page, text, { deadline = Infinity } = {}) {
  const characters = [...String(text)];
  const long = characters.length > MAX_TYPED_CHARACTERS;
  requireNativeInput(page, [long ? 'insertText' : 'nativeType']);
  requireTime(deadline);
  await humanPause(deadline);
  if (long) {
    requireTime(deadline);
    await page.insertText(String(text));
    return;
  }
  for (const [index, character] of characters.entries()) {
    if (index > 0) await restFor(randomBetween(KEY_GAP_MS), deadline);
    requireTime(deadline);
    await page.nativeType(character);
  }
}

async function dispatchKey(page, type, key, modifiers, commands = []) {
  await page.cdp('Input.dispatchKeyEvent', {
    type,
    key: key.key,
    code: key.code,
    windowsVirtualKeyCode: key.keyCode,
    modifiers,
    ...(commands.length > 0 ? { commands } : {}),
  });
}

/** Presses one of `NATIVE_KEYS` (`selectAll`, `backspace`, `delete`, `escape`) in the focused element. */
export async function pressNativeKey(page, name, { deadline = Infinity } = {}) {
  const key = NATIVE_KEYS[name];
  if (!key) throw new Error(`unknown native key ${name}`);
  requireNativeInput(page, ['cdp']);
  requireTime(deadline);
  await humanPause(deadline);
  requireTime(deadline);
  const modifiers = key.shortcut ? SHORTCUT_MODIFIER.bit : 0;
  const commands = IS_MAC && key.command ? [key.command] : [];
  if (key.shortcut) await dispatchKey(page, 'rawKeyDown', SHORTCUT_MODIFIER, modifiers);
  await dispatchKey(page, 'rawKeyDown', key, modifiers, commands);
  await dispatchKey(page, 'keyUp', key, modifiers);
  if (key.shortcut) await dispatchKey(page, 'keyUp', SHORTCUT_MODIFIER, 0);
}
// ---- End of native input ------------------------------------------------------------------------
