import { cli, Strategy } from '@jackwener/opencli/registry';

import {
  HOME_URL, PageFacts, SITE, SITE_DOMAIN, conversationPath, envelope, readConversationReads,
  readDeadline, waitFor,
} from './shared.js';

cli({
  site: SITE,
  name: 'report',
  access: 'read',
  description: 'Open one ChatGPT conversation and report the conversation response the page receives, for forager',
  domain: SITE_DOMAIN,
  strategy: Strategy.COOKIE,
  browser: true,
  navigateBefore: false,
  args: [
    { name: 'conversation', required: true, valueRequired: true, help: 'Conversation UUID' },
    { name: 'timeout', type: 'int', default: 120, help: 'Command timeout in seconds' },
  ],
  func: async (page, kwargs) => {
    const deadline = readDeadline(kwargs.timeout);
    const id = String(kwargs.conversation);
    const path = conversationPath(id);
    const facts = new PageFacts(path);

    // Capture first: the page reads the conversation while the navigation is still settling.
    await page.startNetworkCapture(SITE_DOMAIN);
    await page.goto(`${HOME_URL}c/${encodeURIComponent(id)}`, {
      waitUntil: 'load',
      settleMs: 1000,
    });
    const ended = await waitFor(deadline, async () => {
      if (await facts.observe(page)) return 'page';
      return facts.completions > 0 ? 'completed' : null;
    });
    const completed = ended === 'completed';
    if (completed) await facts.settle(page);
    const reads = await readConversationReads(page, path);
    const answered = reads.find((read) => read.body !== null);
    // The page shows the conversation once the response arrived; its text is no notice.
    if (answered) facts.facts.notice = null;
    return envelope({
      page: facts.facts,
      response: answered ? answered.body : null,
      body_missing: completed && !answered,
      timed_out: !completed && !answered && !facts.facts.signed_out && !facts.facts.notice,
    });
  },
});
