import { cli, Strategy } from '@jackwener/opencli/registry';
import { AuthRequiredError, TimeoutError } from '@jackwener/opencli/errors';

import { SITE_DOMAIN, VERIFICATION_MESSAGE, envelope, readDeadline } from './shared.js';

const SEARCH_API = 'https://api.ssrn.com/papers/v1/papers/search/advanced';

// Sends the request the SSRN results page itself sends, from the command tab without navigating:
// the API needs no cookie, so the tab stays blank and page challenges never load. Network
// failures are returned as facts.
function requestScript(url, timeoutMs) {
  return `(async () => {
    const url = ${JSON.stringify(url)};
    try {
      const response = await fetch(url, {
        credentials: 'omit',
        signal: AbortSignal.timeout(${timeoutMs}),
      });
      const contentType = response.headers.get('content-type');
      const text = await response.text();
      let body = text;
      if (/json/i.test(contentType || '')) {
        try { body = JSON.parse(text); } catch { body = text; }
      }
      return { request_url: url, http_status: response.status, content_type: contentType, body };
    } catch (error) {
      if (error && error.name === 'TimeoutError') return { timed_out: true };
      return { request_url: url, network_error: String(error) };
    }
  })()`;
}

cli({
  site: 'ssrn',
  name: 'search',
  access: 'read',
  description: 'Request one SSRN native search page for forager',
  domain: SITE_DOMAIN,
  strategy: Strategy.PUBLIC,
  browser: true,
  navigateBefore: false,
  args: [
    { name: 'query', required: true, valueRequired: true, help: 'Search terms' },
    { name: 'page', type: 'int', default: 1, help: 'Native results page, 50 results each' },
    { name: 'scope', default: 'title-abstract-keywords', valueRequired: true, help: 'Native text fields' },
    { name: 'mode', default: 'fuzzy', valueRequired: true, help: 'Native matching mode' },
    { name: 'author', default: '', valueRequired: true, help: 'Native author text query' },
    { name: 'date', default: 'all_time', valueRequired: true, help: 'Native date preset' },
    { name: 'sort', default: '', valueRequired: true, help: 'Native sort; empty means relevance' },
    { name: 'timeout', type: 'int', default: 60, help: 'Command timeout in seconds' },
  ],
  func: async (page, kwargs) => {
    const deadline = readDeadline(kwargs.timeout);
    const params = new URLSearchParams({
      text: kwargs.query, text_fields: kwargs.scope, search_mode: kwargs.mode,
      authors: kwargs.author, date: kwargs.date, sort_by: kwargs.sort, page: String(kwargs.page),
    });
    const facts = await page.evaluate(requestScript(`${SEARCH_API}?${params}`, deadline - Date.now()));
    if (facts.timed_out) throw new TimeoutError('ssrn/search', kwargs.timeout);
    // Cloudflare answers a challenge or a block with an HTML page; the API itself answers JSON.
    const bodyIsJson = facts.body !== null && typeof facts.body === 'object';
    if ((facts.http_status === 403 || facts.http_status === 503) && !bodyIsJson) {
      throw new AuthRequiredError(SITE_DOMAIN, VERIFICATION_MESSAGE);
    }
    const empty = facts.http_status === 200 && bodyIsJson && facts.body.total === 0
      && Array.isArray(facts.body.papers) && facts.body.papers.length === 0;
    return envelope(empty ? 'no_results' : 'ok', facts);
  },
});
