// Shared deadlines, navigation, page facts, network capture reads, conversation reads, native
// input, and output envelopes for the forager Gemini commands. The commands only operate the page
// as a user would and report what the page itself received; forager decodes and judges them.

export const CONTRACT = 'forager-gemini/3';
export const SITE = 'forager-gemini';
export const SITE_DOMAIN = 'gemini.google.com';
export const APP_URL = `https://${SITE_DOMAIN}/app`;

// OpenCLI closes the tab after the command returns, so the page reads stop this long before
// the timeout that forager passes.
const CLOSE_RESERVE_MS = 3000;
const POLL_MS = 500;

export function envelope(data) {
  return { contract: CONTRACT, status: 'ok', data };
}

/** Returns the time at which page reads must stop, from the `--timeout` seconds. */
export function readDeadline(timeoutSeconds) {
  return Date.now() + Math.max(1000, Number(timeoutSeconds) * 1000 - CLOSE_RESERVE_MS);
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/** Polls `probe` until it returns a truthy value or `until` passes; returns the last value. */
export async function waitFor(until, probe) {
  for (;;) {
    const value = await probe();
    if (value || Date.now() >= until) return value;
    await sleep(POLL_MS);
  }
}

/**
 * Opens `url` in the tab with the network capture on `SITE_DOMAIN` armed and emptied, so a later
 * read holds only requests from this navigation on.
 */
export async function navigate(page, url) {
  // OpenCLI's extension detaches its debugger before a navigation unless capture is armed on the
  // tab, and Chrome rejects that navigation ("Navigation rejected."). Arming first avoids the
  // detach; drop this once OpenCLI navigates without detaching. Re-arming does not empty the
  // capture in OpenCLI's direct CDP mode, so the read discards what earlier pages left.
  await page.startNetworkCapture(SITE_DOMAIN);
  await page.readNetworkCapture();
  await page.goto(url, { waitUntil: 'load', settleMs: 1000 });
}

/**
 * Whether a request URL is a batchexecute request that calls the RPC `track` (one request can
 * batch several RPCs).
 */
function isTracked(href, track) {
  try {
    const url = new URL(href);
    return url.pathname.endsWith('/batchexecute')
      && (url.searchParams.get('rpcids') || '').split(',').includes(track);
  } catch {
    return false;
  }
}

// The page state that ends a read, and the completed tracked calls. Resource Timing records a
// request only once its response has ended, and `buffered` also counts the calls that completed
// before the first read. The sign-in check follows OpenCLI's own Gemini adapter. The notice
// wording of a missing or foreign conversation is an unverified guess in both interface
// languages; it is read only while no conversation response has arrived, so text inside a
// conversation never counts.
function pageStateScript(track) {
  return `(() => {
    const track = ${JSON.stringify(track)};
    const isTracked = ${isTracked};
    if (track && !window.__foragerGemini) {
      window.__foragerGemini = { completions: 0 };
      new PerformanceObserver((list) => {
        for (const entry of list.getEntries()) {
          if (entry.responseEnd > 0 && isTracked(entry.name, track)) window.__foragerGemini.completions += 1;
        }
      }).observe({ type: 'resource', buffered: true });
    }
    const signInNode = Array.from(document.querySelectorAll('a, button')).find((node) => {
      const text = (node.textContent || '').trim().toLowerCase();
      const aria = (node.getAttribute('aria-label') || '').trim().toLowerCase();
      const href = node.getAttribute('href') || '';
      return text === 'sign in' || aria === 'sign in' || text === '登录' || aria === '登录'
        || href.includes('accounts.google.com/ServiceLogin');
    });
    const signedOut = Boolean(signInNode) || location.hostname === 'accounts.google.com';
    const text = document.body ? document.body.innerText : '';
    const notice = (text.match(/[^\\n]*(conversation not found|couldn.t (load|find) (this )?(chat|conversation)|无法(加载|找到)(此|该)?对话|找不到(此|该)?对话)[^\\n]*/i) || [null])[0];
    const composer = document.querySelector('rich-textarea, [contenteditable="true"][role="textbox"], div.ql-editor');
    return {
      url: location.href,
      signed_out: signedOut,
      notice: notice ? notice.trim().slice(0, 120) : null,
      ready: signedOut || Boolean(composer),
      completions: window.__foragerGemini ? window.__foragerGemini.completions : 0,
    };
  })()`;
}

/** Collects the facts about where the page is and what Gemini showed there. */
export class PageFacts {
  /** `track`, when given, is the RPC id whose completed calls the page counts. */
  constructor(track = null) {
    this.script = pageStateScript(track);
    this.facts = { url: '', signed_out: false, notice: null };
    this.ready = false;
    this.completions = 0;
  }

  /** Reads the page state; returns whether it ends the command. */
  async observe(page) {
    const state = await page.evaluate(this.script);
    this.facts = { url: state.url, signed_out: state.signed_out, notice: state.notice };
    this.ready = state.ready;
    this.completions = state.completions;
    return this.facts.signed_out || Boolean(this.facts.notice);
  }

  /**
   * Waits until the bodies of the completed tracked calls are stored. Reading the capture
   * drains it, and an entry drained before OpenCLI stores its body never gets one. OpenCLI asks
   * Chrome for the body on the request's loadingFinished event, which Chrome sends before the
   * page can see the completion, and a tab answers DevTools commands in order, so one more page
   * round trip returns only after the body request has been answered.
   */
  async settle(page) {
    await this.observe(page);
  }
}

function decodeText(preview) {
  if (typeof preview !== 'string' || preview === '') return null;
  return preview.startsWith('base64:') ? Buffer.from(preview.slice(7), 'base64').toString('utf8') : preview;
}

/**
 * Drains the network capture into the calls of the RPC `track`, in request order, as
 * `{status, body}`. A body OpenCLI truncated counts as no body.
 */
async function readTrackedCalls(page, track) {
  const entries = await page.readNetworkCapture();
  return entries
    .filter((entry) => entry.method === 'POST' && isTracked(entry.url, track))
    .map((entry) => ({
      status: entry.responseStatus || null,
      body: entry.responseBodyTruncated ? null : decodeText(entry.responsePreview),
    }));
}

// The RPC that lists a conversation's turns; the page calls it when it opens a conversation.
const CONVERSATION_RPC = 'hNvQHb';

// Browser Bridge fetches a response body once, when the request finishes, and a failed fetch
// leaves the entry complete but without a body. Live under Browser Bridge 1.0.26 and Chrome 155,
// about half of the opens of a 436 KB conversation lost the `hNvQHb` body this way, and other
// batchexecute calls lost theirs at random; the page's next load fetches it anew. Opens are few
// because Google answers a burst of page loads with its unusual-traffic page.
const MAX_OPENS = 3;

/** The id in a conversation URL `/app/<id>`, or null. */
export function conversationOf(href) {
  try {
    const url = new URL(href);
    const match = url.hostname === SITE_DOMAIN && url.pathname.match(/^\/app\/([0-9a-f]+)\/?$/i);
    return match ? match[1].toLowerCase() : null;
  } catch {
    return null;
  }
}

/** Opens `url` and reads the first conversation response the page receives. */
async function readConversation(page, url, deadline) {
  const facts = new PageFacts(CONVERSATION_RPC);
  // The capture starts before the navigation: the page calls the RPC while the navigation is
  // still settling.
  await navigate(page, url);
  const ended = await waitFor(deadline, async () => {
    if (await facts.observe(page)) return 'page';
    return facts.completions > 0 ? 'completed' : null;
  });
  const completed = ended === 'completed';
  if (completed) await facts.settle(page);
  const calls = await readTrackedCalls(page, CONVERSATION_RPC);
  const answered = calls.find((call) => call.body !== null);
  // The page shows the conversation once the response arrived; its text is no notice.
  if (answered) facts.facts.notice = null;
  return {
    page: facts.facts,
    response: answered ? answered.body : null,
    body_missing: completed && !answered,
    timed_out: !completed && !answered && !facts.facts.signed_out && !facts.facts.notice,
  };
}

/**
 * Opens a conversation and reads the conversation response the page receives, as
 * `{page, response, body_missing, timed_out}`. While the response completed without a captured
 * body, it opens the conversation again, at most `MAX_OPENS` times in all. With `reload`, the
 * tab already shows the conversation, which the first open then reloads too.
 */
export async function openConversation(page, conversation, deadline, { reload = false } = {}) {
  const url = `${APP_URL}/${encodeURIComponent(String(conversation))}`;
  let read = null;
  for (let opens = 0; opens < MAX_OPENS && (!read || (read.body_missing && Date.now() < deadline)); opens += 1) {
    // OpenCLI does not navigate a tab that already shows the URL, so the page reloads the
    // conversation by way of the app home.
    if (reload || opens > 0) await navigate(page, APP_URL);
    read = await readConversation(page, url, deadline);
  }
  return read;
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
