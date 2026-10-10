import { cli, Strategy } from '@jackwener/opencli/registry';

import {
  HOME_URL, PageFacts, SITE, SITE_DOMAIN, envelope, readDeadline, waitFor,
} from './shared.js';

cli({
  site: SITE,
  name: 'status',
  access: 'read',
  description: 'Open the ChatGPT home page and report whether the browser is signed in, for forager doctor',
  domain: SITE_DOMAIN,
  strategy: Strategy.COOKIE,
  browser: true,
  navigateBefore: false,
  args: [
    { name: 'timeout', type: 'int', default: 30, help: 'Command timeout in seconds' },
  ],
  // Only looks: no composer mode switch, no menu.
  func: async (page, kwargs) => {
    const deadline = readDeadline(kwargs.timeout);
    const facts = new PageFacts();

    await page.goto(HOME_URL, { waitUntil: 'load', settleMs: 1000 });
    await waitFor(deadline, async () => {
      await facts.observe(page);
      return facts.ready;
    });
    return envelope({ page: facts.facts, timed_out: !facts.ready });
  },
});
