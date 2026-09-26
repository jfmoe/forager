import { cli, Strategy } from '@jackwener/opencli/registry';

import { SITE_DOMAIN, envelope, readDeadline, readPage } from './shared.js';

// Reads one native results page (50 results) as SSRN shows it.
const READ_RESULTS = `
  const range = (text.match(/Displaying results[^\\n]*/) || [null])[0];
  const links = [...document.querySelectorAll('h3 a[href*="abstract_id="]')];
  const noResults = /(^|\\n)\\s*No results\\.?\\s*(\\n|$)/.test(text);
  if (!range && !(noResults && links.length === 0)) return { state: 'pending', data: {} };
  const next = document.querySelector('a[aria-label="Next page"]');
  return {
    state: range ? 'results' : 'no_results',
    data: {
      url: location.href,
      term: document.querySelector('#term') ? document.querySelector('#term').value : null,
      current_page: document.querySelector('a[aria-current="page"]')
        ? document.querySelector('a[aria-current="page"]').textContent
        : null,
      range,
      next_page: Boolean(next) && next.getAttribute('aria-hidden') !== 'true'
        && !/disabled/.test(next.className),
      results: links.map((link) => {
        const card = link.closest('[data-component="Stack"]') || link.parentElement;
        return {
          url: link.href,
          title: link.textContent,
          authors: [...card.querySelectorAll('a[href*="AbsByAuth.cfm"]')].map((a) => a.textContent),
          details: [...card.querySelectorAll('p')].map((p) => p.textContent)
            .find((line) => /Posted:/.test(line)) || '',
          snippets: [...card.querySelectorAll('.snippet-mark > div')].map((div) => div.textContent),
        };
      }),
    },
  };
`;

cli({
  site: 'ssrn',
  name: 'search',
  access: 'read',
  description: 'Read one SSRN results page for forager',
  domain: SITE_DOMAIN,
  strategy: Strategy.PUBLIC,
  browser: true,
  navigateBefore: false,
  args: [
    { name: 'query', required: true, valueRequired: true, help: 'Search terms' },
    { name: 'page', type: 'int', default: 1, help: 'Native results page, 50 results each' },
    { name: 'timeout', type: 'int', default: 60, help: 'Command timeout in seconds' },
  ],
  func: async (page, kwargs) => {
    const deadline = readDeadline(kwargs.timeout);
    const pageNumber = Number(kwargs.page);
    const url = `https://${SITE_DOMAIN}/searchresults.cfm?term=${encodeURIComponent(kwargs.query)}`
      + (pageNumber > 1 ? `&page=${pageNumber}` : '');
    await page.goto(url, { waitUntil: 'load', settleMs: 500 });
    const facts = await readPage(page, deadline, READ_RESULTS);
    return envelope(facts.state === 'no_results' ? 'no_results' : 'ok', facts.data);
  },
});
