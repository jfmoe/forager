import { cli, Strategy } from '@jackwener/opencli/registry';

import { SITE, SITE_DOMAIN, envelope, openConversation, readDeadline } from './shared.js';

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
    return envelope(await openConversation(page, kwargs.conversation, deadline));
  },
});
