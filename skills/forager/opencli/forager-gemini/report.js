import { cli, Strategy } from '@jackwener/opencli/registry';

import {
  APP_URL, POLL_MS, PageFacts, SITE, SITE_DOMAIN, envelope, readDeadline, readRpcCalls, sleep,
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
    const calls = [];
    let completed = false;

    // Capture first: the page calls the RPC while the navigation is still settling.
    await page.startNetworkCapture(SITE_DOMAIN);
    await page.goto(`${APP_URL}/${encodeURIComponent(String(kwargs.conversation))}`, {
      waitUntil: 'load',
      settleMs: 1000,
    });
    while (Date.now() < deadline) {
      if (await facts.observe(page)) break;
      if (facts.completions > 0) {
        // Reading the capture drains it, and an entry drained before OpenCLI stores its body
        // never gets one. OpenCLI asks Chrome for the body on the request's loadingFinished
        // event, which Chrome sends before the page can see the completion, and a tab answers
        // DevTools commands in order. One more page round trip therefore returns only after
        // the body request has been answered.
        await facts.observe(page);
        completed = true;
        break;
      }
      await sleep(POLL_MS);
    }
    calls.push(...await readRpcCalls(page, CONVERSATION_RPC));
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
