import { cli, Strategy } from '@jackwener/opencli/registry';

import { SITE_DOMAIN, envelope, readDeadline, readPage } from './shared.js';

// Reads the abstract page of one paper as SSRN shows it.
const READ_PAPER = `
  const unavailable = (text.match(/[^\\n]*(under review or has been removed)[^\\n]*/) || [null])[0];
  const heading = document.querySelector('.box-abstract-main h1, h1');
  if (unavailable && !heading) {
    return { state: 'no_results', data: { url: location.href, notice: unavailable } };
  }
  if (!heading) return { state: 'pending', data: {} };
  const main = document.querySelector('.box-abstract-main') || document;
  const meta = (name) => {
    const element = document.querySelector('meta[name="' + name + '"]');
    return element ? element.content : null;
  };
  const canonical = document.querySelector('link[rel="canonical"]');
  const notes = main.querySelector('.note-list') || document.querySelector('.note-list');
  return {
    state: 'paper',
    data: {
      url: location.href,
      canonical_url: canonical ? canonical.href : null,
      doi: meta('citation_doi'),
      title: heading.textContent,
      authors: [...main.querySelectorAll('.authors h2')].map((h2) => h2.textContent),
      abstract_paragraphs: [...main.querySelectorAll('.abstract-text p')].map((p) => p.textContent),
      notes: notes ? [...notes.querySelectorAll('span')].map((span) => span.textContent) : [],
      date_written: [...main.querySelectorAll('p')].map((p) => p.textContent.trim())
        .find((line) => line.startsWith('Date Written:')) || null,
    },
  };
`;

cli({
  site: 'ssrn',
  name: 'paper',
  access: 'read',
  description: 'Read the abstract page of one SSRN paper for forager',
  domain: SITE_DOMAIN,
  strategy: Strategy.PUBLIC,
  browser: true,
  navigateBefore: false,
  args: [
    { name: 'id', required: true, valueRequired: true, help: 'SSRN abstract ID' },
    { name: 'timeout', type: 'int', default: 60, help: 'Command timeout in seconds' },
  ],
  func: async (page, kwargs) => {
    const deadline = readDeadline(kwargs.timeout);
    const id = String(kwargs.id);
    await page.goto(`https://${SITE_DOMAIN}/sol3/papers.cfm?abstract_id=${encodeURIComponent(id)}`, {
      waitUntil: 'load',
      settleMs: 500,
    });
    const facts = await readPage(page, deadline, READ_PAPER);
    return envelope(facts.state === 'no_results' ? 'no_results' : 'ok', facts.data);
  },
});
