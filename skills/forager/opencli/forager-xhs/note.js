import { cli, Strategy } from '@jackwener/opencli/registry';

import { PageFacts, SITE, SITE_DOMAIN, envelope, readDeadline, readExchanges, sleep } from './shared.js';

const POLL_MS = 500;

// Reads the note the page rendered from its server-side state, keeping only the fields forager
// decodes. The signed, expiring video stream URLs and the page's own access token never leave
// the page.
function noteScript(id) {
  return `(() => {
    const id = ${JSON.stringify(id)};
    const state = window.__INITIAL_STATE__;
    const map = state && state.note && state.note.noteDetailMap;
    const entry = map && map[id];
    const raw = entry && entry.note;
    if (!raw || typeof raw !== 'object' || !raw.noteId) return null;
    const note = JSON.parse(JSON.stringify(raw));
    const pick = (object, keys) => {
      const picked = {};
      for (const key of keys) if (object && object[key] !== undefined) picked[key] = object[key];
      return picked;
    };
    let video = null;
    if (note.video && typeof note.video === 'object') {
      const stream = {};
      const source = (note.video.media && note.video.media.stream) || {};
      for (const [encoding, renditions] of Object.entries(source)) {
        if (Array.isArray(renditions)) stream[encoding] = renditions.map((item) => pick(item, ['width', 'height']));
      }
      video = { capa: pick(note.video.capa, ['duration']), media: { stream } };
    }
    return {
      ...pick(note, ['noteId', 'title', 'desc', 'type', 'time', 'lastUpdateTime', 'ipLocation']),
      tagList: (note.tagList || []).map((tag) => pick(tag, ['name'])),
      imageList: (note.imageList || []).map((image) => pick(image, ['urlDefault', 'width', 'height'])),
      interactInfo: pick(note.interactInfo, ['likedCount', 'collectedCount', 'commentCount', 'shareCount']),
      user: pick(note.user, ['userId', 'nickname']),
      video,
    };
  })()`;
}

cli({
  site: SITE,
  name: 'note',
  access: 'read',
  description: 'Read one Xiaohongshu note page, opened with its access token, for forager',
  domain: SITE_DOMAIN,
  strategy: Strategy.COOKIE,
  browser: true,
  navigateBefore: false,
  args: [
    { name: 'id', required: true, valueRequired: true, help: 'Note ID' },
    { name: 'xsec-token', required: true, valueRequired: true, help: 'Access token of the note' },
    { name: 'timeout', type: 'int', default: 120, help: 'Command timeout in seconds' },
  ],
  func: async (page, kwargs) => {
    const deadline = readDeadline(kwargs.timeout);
    const id = String(kwargs.id);
    const facts = new PageFacts();
    let note = null;
    let timedOut = true;

    await page.startNetworkCapture('xiaohongshu.com');
    const params = new URLSearchParams({ xsec_token: String(kwargs['xsec-token']), xsec_source: 'pc_search' });
    await page.goto(`https://${SITE_DOMAIN}/explore/${encodeURIComponent(id)}?${params}`, {
      waitUntil: 'load',
      settleMs: 1000,
    });
    while (Date.now() < deadline) {
      note = await page.evaluate(noteScript(id));
      facts.absorb(await readExchanges(page));
      const ended = await facts.observe(page);
      if (note || ended) {
        timedOut = false;
        break;
      }
      await sleep(POLL_MS);
    }
    // A rendered note is the expected page; text in the note or its comments is not a notice.
    if (note) facts.facts.notice = null;
    return envelope({ page: facts.facts, note, timed_out: timedOut });
  },
});
