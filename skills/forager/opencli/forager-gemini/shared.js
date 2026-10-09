// Shared deadlines, page facts, network capture reads, and output envelopes for the forager
// Gemini commands. The commands only operate the page as a user would and report what the page
// itself received; forager decodes and judges the responses.

export const CONTRACT = 'forager-gemini/1';
export const SITE = 'forager-gemini';
export const SITE_DOMAIN = 'gemini.google.com';
export const APP_URL = `https://${SITE_DOMAIN}/app`;

// OpenCLI closes the tab after the command returns, so the page reads stop this long before
// the timeout that forager passes.
const CLOSE_RESERVE_MS = 3000;
export const POLL_MS = 500;

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

/** Whether a batchexecute request URL calls `rpc`; one request can batch several RPCs. */
function callsRpc(href, rpc) {
  try {
    const url = new URL(href);
    return url.pathname.endsWith('/batchexecute')
      && (url.searchParams.get('rpcids') || '').split(',').includes(rpc);
  } catch {
    return false;
  }
}

// The page state that ends a read, and the completed calls to the tracked RPC. Resource Timing
// records a request only once its response has ended, and `buffered` also counts the calls that
// completed before the first read. The sign-in check follows OpenCLI's own Gemini adapter. The
// notice wording of a missing or foreign conversation is an unverified guess in both interface
// languages; it is read only while no conversation response has arrived, so text inside a
// conversation never counts.
function pageStateScript(rpc) {
  return `(() => {
    const rpc = ${JSON.stringify(rpc)};
    const callsRpc = ${callsRpc};
    if (rpc && !window.__foragerGemini) {
      window.__foragerGemini = { completions: 0 };
      new PerformanceObserver((list) => {
        for (const entry of list.getEntries()) {
          if (entry.responseEnd > 0 && callsRpc(entry.name, rpc)) window.__foragerGemini.completions += 1;
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
  /** `rpc`, when given, is the RPC whose completed calls the page counts. */
  constructor(rpc = null) {
    this.script = pageStateScript(rpc);
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
}

function decodeText(preview) {
  if (typeof preview !== 'string' || preview === '') return null;
  return preview.startsWith('base64:') ? Buffer.from(preview.slice(7), 'base64').toString('utf8') : preview;
}

/**
 * Drains the network capture into the calls of `rpc`, in request order, as `{status, body}`.
 * A body OpenCLI truncated counts as no body.
 */
export async function readRpcCalls(page, rpc) {
  const entries = await page.readNetworkCapture();
  return entries
    .filter((entry) => entry.method === 'POST' && callsRpc(entry.url, rpc))
    .map((entry) => ({
      status: entry.responseStatus || null,
      body: entry.responseBodyTruncated ? null : decodeText(entry.responsePreview),
    }));
}
