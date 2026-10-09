import { cli, Strategy } from '@jackwener/opencli/registry';

import {
  APP_URL, PageFacts, SITE, SITE_DOMAIN, envelope, readDeadline, readTrackedCalls, waitFor,
} from './shared.js';

// The RPC that lists a conversation's turns; the page calls it when it opens a conversation.
const CONVERSATION_RPC = 'hNvQHb';

cli({
  site: SITE,
  name: 'report',
  access: 'read',
  description: 'Open one Gemini conversation and report the turn list the page receives, for forager',
  domain: SITE_DOMAIN,
  strategy: Strategy.COOKIE,
  browser: true,
  navigateBefore: false,
  args: [
    { name: 'conversation', required: true, valueRequired: true, help: 'Hexadecimal conversation ID' },
    { name: 'timeout', type: 'int', default: 120, help: 'Command timeout in seconds' },
  ],
  func: async (page, kwargs) => {
    const deadline = readDeadline(kwargs.timeout);
    const facts = new PageFacts(CONVERSATION_RPC);

    // Capture first: the page calls the RPC while the navigation is still settling.
    await page.startNetworkCapture(SITE_DOMAIN);
    await page.goto(`${APP_URL}/${encodeURIComponent(String(kwargs.conversation))}`, {
      waitUntil: 'load',
      settleMs: 1000,
    });
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
    return envelope({
      page: facts.facts,
      response: answered ? answered.body : null,
      body_missing: completed && !answered,
      timed_out: !completed && !answered && !facts.facts.signed_out && !facts.facts.notice,
    });
  },
});
