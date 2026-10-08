import { cli, Strategy } from '@jackwener/opencli/registry';

import { SITE, envelope } from './shared.js';

cli({
  site: SITE,
  name: 'contract',
  access: 'read',
  description: 'Report the forager output contract of the Xiaohongshu adapter commands',
  strategy: Strategy.PUBLIC,
  browser: false,
  // forager passes the same session flags to every command; this command needs no browser,
  // so it accepts them as ordinary options.
  args: [
    { name: 'timeout', type: 'int', default: 10, help: 'Command timeout in seconds' },
    { name: 'window', help: 'Accepted for forager; unused' },
    { name: 'site-session', help: 'Accepted for forager; unused' },
    { name: 'keep-tab', help: 'Accepted for forager; unused' },
  ],
  func: async () => envelope({ commands: ['search', 'contract'] }),
});
