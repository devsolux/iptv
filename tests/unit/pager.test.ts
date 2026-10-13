import assert from 'node:assert/strict';
import { test } from 'node:test';
import { CataloguePager, PAGE_SIZE } from '../../src/lib/catalogue-pager.ts';
import type { ChannelFilters, ChannelPage, ChannelQuery } from '../../src/lib/contracts.ts';

const filters: ChannelFilters = {
  search: '',
  sourceId: null,
  group: null,
  country: null,
  favoritesOnly: false,
};
const tick = () => new Promise<void>((resolve) => setImmediate(resolve));
function page(query: ChannelQuery, revision = 1): ChannelPage {
  return {
    items: [
      {
        id: `channel-${query.offset}`,
        name: `Page ${query.offset}`,
        sourceId: 'source',
        country: 'DE',
        favorite: false,
        hasEpg: false,
        variants: 1,
        logoUrl: null,
      },
    ],
    total: 50000,
    offset: query.offset,
    revision,
  };
}
test('cache stays bounded after scrolling a 50,000 channel catalogue', async () => {
  const pager = new CataloguePager(async (query) => page(query));
  pager.reset(filters);
  await tick();
  for (let index = 0; index < 40; index++) {
    pager.wantRange(index * PAGE_SIZE, index * PAGE_SIZE + 79);
    await tick();
    assert.ok(pager.getSnapshot().pages.size <= 6);
  }
  assert.equal(pager.getSnapshot().total, 50000);
  assert.equal(pager.at(39 * PAGE_SIZE)?.id, `channel-${39 * PAGE_SIZE}`);
  pager.dispose();
});
test('at most two page requests run while a newer filter waits for obsolete requests', async () => {
  const requests: { query: ChannelQuery; resolve: (value: ChannelPage) => void }[] = [];
  const pager = new CataloguePager(
    (query) => new Promise((resolve) => requests.push({ query, resolve })),
  );
  pager.reset(filters);
  pager.wantRange(0, 160);
  assert.equal(requests.length, 2);
  pager.reset({ ...filters, search: 'new' });
  assert.equal(requests.length, 2);
  requests[0]?.resolve(page(requests[0].query, 1));
  await tick();
  assert.equal(requests.length, 3);
  assert.equal(requests[2]?.query.search, 'new');
  assert.equal(pager.getSnapshot().pages.size, 0);
  requests[1]?.resolve(page(requests[1].query, 1));
  requests[2]?.resolve(page(requests[2].query, 2));
  await tick();
  assert.equal(pager.getSnapshot().revision, 2);
  pager.dispose();
});
test('stale revisions retry finitely, then expose an error rather than looping forever', async () => {
  let calls = 0;
  const pager = new CataloguePager(async () => {
    calls++;
    throw Object.assign(new Error('Stale'), { code: 'STALE' });
  });
  pager.reset(filters);
  for (let i = 0; i < 8; i++) await tick();
  assert.equal(calls, 3);
  assert.ok(pager.getSnapshot().error);
  assert.equal(pager.getSnapshot().loading, false);
  pager.dispose();
});
test('mixed catalogue revisions cannot be retained in one page cache', async () => {
  const pager = new CataloguePager(async (query) => page(query, query.offset === 0 ? 1 : 2));
  pager.reset(filters);
  await tick();
  pager.wantRange(80, 159);
  await tick();
  await tick();
  assert.equal(pager.getSnapshot().revision, 2);
  assert.equal(pager.getSnapshot().pages.has(0), false);
  pager.dispose();
});
test('late responses after disposal do not publish a page or emit to subscribers', async () => {
  let resolve!: (value: ChannelPage) => void;
  let query!: ChannelQuery;
  let events = 0;
  const pager = new CataloguePager((q) => {
    query = q;
    return new Promise((r) => {
      resolve = r;
    });
  });
  pager.subscribe(() => events++);
  pager.reset(filters);
  pager.dispose();
  const before = events;
  resolve(page(query));
  await tick();
  assert.equal(events, before);
  assert.equal(pager.getSnapshot().pages.size, 0);
});
test('catalogue refresh keeps the total and visible window instead of collapsing the view', async () => {
  let revision = 1;
  const queries: ChannelQuery[] = [];
  const pager = new CataloguePager(async (query) => {
    queries.push(query);
    return page(query, revision);
  });
  pager.reset(filters);
  await tick();
  pager.wantRange(40 * PAGE_SIZE, 40 * PAGE_SIZE + 79);
  await tick();
  revision = 2;
  queries.length = 0;
  const totals: number[] = [];
  pager.subscribe(() => totals.push(pager.getSnapshot().total));
  pager.refresh();
  assert.ok(totals.length > 0 && totals.every((total) => total === 50000));
  assert.equal(pager.getSnapshot().ready, true);
  await tick();
  assert.deepEqual(
    queries.map((query) => [query.offset, query.revision]),
    [[40 * PAGE_SIZE, null]],
  );
  assert.equal(pager.getSnapshot().revision, 2);
  assert.equal(pager.at(40 * PAGE_SIZE)?.id, `channel-${40 * PAGE_SIZE}`);
  pager.dispose();
  pager.refresh();
  assert.equal(queries.length, 1);
});
