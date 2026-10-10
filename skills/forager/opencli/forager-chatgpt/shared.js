// Shared deadlines, page facts, network capture reads, and output envelopes for the forager
// ChatGPT commands. The commands only operate the page as a user would and report what the page
// itself received; forager decodes and judges the responses. They never read cookies or
// tokens, never request anything themselves, and never touch the page's fetch, XHR, or
// WebSocket (ADR 0024).

export const CONTRACT = 'forager-chatgpt/1';
export const SITE = 'forager-chatgpt';
export const SITE_DOMAIN = 'chatgpt.com';
export const HOME_URL = `https://${SITE_DOMAIN}/`;

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

/** The path of the request the page sends for a conversation when it opens `/c/<id>`. */
export function conversationPath(id) {
  return `/backend-api/conversations/${id}`;
}

/** Whether a request URL is the page's own read of the conversation at `path`. */
function isConversationRead(href, path) {
  try {
    const url = new URL(href);
    return url.hostname === 'chatgpt.com' && url.pathname === path;
  } catch {
    return false;
  }
}

// The page state that ends a read, and the completed conversation reads. Resource Timing
// records a request only once its response has ended, and `buffered` also counts the reads
// that completed before the first poll. The sign-in and notice wordings are unverified guesses
// in both interface languages; the notice is read only while no conversation response has
// arrived, so text inside a conversation never counts.
function pageStateScript(path) {
  return `(() => {
    const path = ${JSON.stringify(path)};
    const isConversationRead = ${isConversationRead};
    if (path && !window.__foragerChatgpt) {
      window.__foragerChatgpt = { completions: 0 };
      new PerformanceObserver((list) => {
        for (const entry of list.getEntries()) {
          if (entry.responseEnd > 0 && isConversationRead(entry.name, path)) window.__foragerChatgpt.completions += 1;
        }
      }).observe({ type: 'resource', buffered: true });
    }
    const signInNode = Array.from(document.querySelectorAll('a, button')).find((node) => {
      const text = (node.textContent || '').trim().toLowerCase();
      const testId = node.getAttribute('data-testid') || '';
      return testId === 'login-button' || text === 'log in' || text === '登录' || text === '登入';
    });
    const signedOut = Boolean(signInNode)
      || location.hostname === 'auth.openai.com'
      || location.pathname.startsWith('/auth/login');
    const text = document.body ? document.body.innerText : '';
    const notice = (text.match(/[^\\n]*(unable to load conversation|conversation not found|无法加载对话|無法載入對話|找不到(此|该|該)?对话|找不到(此|该|該)?對話)[^\\n]*/i) || [null])[0];
    const composer = document.querySelector('#prompt-textarea, [contenteditable="true"][role="textbox"]');
    return {
      url: location.href,
      signed_out: signedOut,
      notice: notice ? notice.trim().slice(0, 120) : null,
      ready: signedOut || Boolean(composer),
      completions: window.__foragerChatgpt ? window.__foragerChatgpt.completions : 0,
    };
  })()`;
}

/** Collects the facts about where the page is and what ChatGPT showed there. */
export class PageFacts {
  /** `path`, when given, is the conversation read whose completions the page counts. */
  constructor(path = null) {
    this.script = pageStateScript(path);
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
   * Waits until the bodies of the completed reads are stored. Reading the capture drains it,
   * and an entry drained before OpenCLI stores its body never gets one. OpenCLI asks Chrome for
   * the body on the request's loadingFinished event, which Chrome sends before the page can see
   * the completion, and a tab answers DevTools commands in order, so one more page round trip
   * returns only after the body request has been answered.
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
 * Drains the network capture into the page's own reads of the conversation at `path`, in
 * request order, as `{status, body}`. A body OpenCLI truncated counts as no body.
 */
export async function readConversationReads(page, path) {
  const entries = await page.readNetworkCapture();
  return entries
    .filter((entry) => entry.method === 'GET' && isConversationRead(entry.url, path))
    .map((entry) => ({
      status: entry.responseStatus || null,
      body: entry.responseBodyTruncated ? null : decodeText(entry.responsePreview),
    }));
}
