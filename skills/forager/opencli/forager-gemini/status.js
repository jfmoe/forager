import { cli, Strategy } from '@jackwener/opencli/registry';

import {
  APP_URL, POLL_MS, PageFacts, SITE, SITE_DOMAIN, envelope, readDeadline, sleep,
} from './shared.js';

cli({
  site: SITE,
  name: 'status',
  access: 'read',
  description: 'Open the Gemini app and report whether the browser is signed in, for forager doctor',
  domain: SITE_DOMAIN,
  strategy: Strategy.COOKIE,
  browser: true,
  navigateBefore: false,
  args: [
    { name: 'timeout', type: 'int', default: 30, help: 'Command timeout in seconds' },
  ],
  func: async (page, kwargs) => {
    const deadline = readDeadline(kwargs.timeout);
    const facts = new PageFacts();
    let timedOut = true;

    await page.goto(APP_URL, { waitUntil: 'load', settleMs: 1000 });
    while (Date.now() < deadline) {
      await facts.observe(page);
      if (facts.ready) {
        timedOut = false;
        break;
      }
      await sleep(POLL_MS);
    }
    return envelope({ page: facts.facts, timed_out: timedOut });
  },
});
