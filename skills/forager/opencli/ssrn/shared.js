// Helpers shared by the forager SSRN adapter commands. The commands only read page facts;
// forager checks and normalizes them.

import { AuthRequiredError } from '@jackwener/opencli/errors';

export const CONTRACT = 'forager-ssrn/1';
export const SITE_DOMAIN = 'papers.ssrn.com';

// OpenCLI closes the tab after the command returns, so the page reads stop this long before
// the timeout that forager passes.
const CLOSE_RESERVE_MS = 3000;
const POLL_MS = 1000;

export function envelope(status, data) {
  return { contract: CONTRACT, status, data };
}

/** Returns the time at which page reads must stop, from the `--timeout` seconds. */
export function readDeadline(timeoutSeconds) {
  return Date.now() + Math.max(1000, Number(timeoutSeconds) * 1000 - CLOSE_RESERVE_MS);
}

// Page-state checks that every SSRN page shares; `readScript` adds the page's own facts.
const STATE_SCRIPT = `
  const text = document.body ? document.body.innerText : '';
  const challenge = Boolean(document.querySelector('#challenge-form, #challenge-running, .cf-turnstile'))
    || /Just a moment|请稍候/.test(document.title)
    || (/Cloudflare/.test(text) && /Ray ID/.test(text));
  const blocked = /Content Blocked/.test(document.title) || /unsupported automated script/i.test(text);
`;

/**
 * Evaluates `readScript` until the page reaches a final state or the deadline passes. `readScript`
 * is the body of a function that can use `text`, and returns `{ state, data }` where `state`
 * is `pending` until the page shows a known final state.
 */
export async function readPage(page, deadline, readScript) {
  let facts = { state: 'pending', data: {} };
  while (Date.now() < deadline) {
    facts = await page.evaluate(`(() => {
      ${STATE_SCRIPT}
      if (challenge) return { state: 'challenge', data: { url: location.href } };
      if (blocked) return { state: 'blocked', data: { url: location.href } };
      ${readScript}
    })()`);
    // A transient security check clears by itself after a few seconds, so keep reading.
    if (facts.state !== 'pending' && facts.state !== 'challenge') break;
    await page.wait(POLL_MS / 1000);
  }
  if (facts.state === 'challenge') {
    throw new AuthRequiredError(
      SITE_DOMAIN,
      'SSRN security verification did not clear; open SSRN in Chrome once, then retry',
    );
  }
  if (facts.state === 'blocked') {
    throw new AuthRequiredError(SITE_DOMAIN, 'SSRN blocked the automated browser session');
  }
  return facts;
}
