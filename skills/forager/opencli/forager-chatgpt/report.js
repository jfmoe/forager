import { cli, Strategy } from '@jackwener/opencli/registry';

import { SITE, SITE_DOMAIN, envelope, openConversation, readDeadline } from './shared.js';

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
    const { page: facts, body, completed } = await openConversation(
      page, String(kwargs.conversation), deadline,
    );
    const answered = body !== null;
    // The page shows the conversation once the response arrived; its text is no notice.
    if (answered) facts.notice = null;
    return envelope({
      page: facts,
      response: body,
      body_missing: completed && !answered,
      timed_out: !completed && !answered && !facts.signed_out && !facts.notice,
    });
  },
});
