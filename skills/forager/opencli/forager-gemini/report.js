import { cli, Strategy } from '@jackwener/opencli/registry';

import {
  APP_URL, PageFacts, SITE, SITE_DOMAIN, envelope, navigate, readDeadline, readTrackedCalls,
  waitFor,
} from './shared.js';

// The RPC that lists a conversation's turns; the page calls it when it opens a conversation.
const CONVERSATION_RPC = 'hNvQHb';

// Browser Bridge fetches a response body once, when the request finishes, and a failed fetch
// leaves the entry complete but without a body. Live under Browser Bridge 1.0.26 and Chrome 155,
// about half of the opens of a 436 KB conversation lost the `hNvQHb` body this way, and other
// batchexecute calls lost theirs at random; the page's next load fetches it anew. Opens are few
// because Google answers a burst of page loads with its unusual-traffic page.
const MAX_OPENS = 3;

/** Opens the conversation and reads the first conversation response the page receives. */
async function readConversation(page, url, deadline) {
  const facts = new PageFacts(CONVERSATION_RPC);
  // The capture starts before the navigation: the page calls the RPC while the navigation is
  // still settling.
  await navigate(page, url);
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
  return {
    page: facts.facts,
    response: answered ? answered.body : null,
    body_missing: completed && !answered,
    timed_out: !completed && !answered && !facts.facts.signed_out && !facts.facts.notice,
  };
}

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
    const url = `${APP_URL}/${encodeURIComponent(String(kwargs.conversation))}`;
    let read = await readConversation(page, url, deadline);
    for (let opens = 1; read.body_missing && opens < MAX_OPENS && Date.now() < deadline; opens += 1) {
      // OpenCLI does not navigate a tab that already shows the URL, so the page reloads the
      // conversation by way of the app home.
      await navigate(page, APP_URL);
      read = await readConversation(page, url, deadline);
    }
    return envelope(read);
  },
});
