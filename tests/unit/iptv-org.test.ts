import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  appendCode,
  buildIptvOrgConfig,
  codeList,
  DEFAULT_PLAYLIST_BASE,
  defaultIptvOrgValues,
  officialPlaylistPath,
  officialPlaylists,
  parseAliases,
} from '../../src/lib/iptv-org.ts';

test('code lists split on separators, trim and deduplicate case-insensitively', () => {
  assert.deepEqual(codeList(' de, TR;us\n de  tr '), ['de', 'TR', 'us']);
  assert.equal(appendCode('DE, TR', 'tr'), 'DE, TR');
  assert.equal(appendCode('', 'US'), 'US');
  assert.equal(appendCode('DE', 'US'), 'DE, US');
});
test('aliases canonicalize GB to UK and reject malformed lines', () => {
  assert.deepEqual(parseAliases('GB=UK\n\ngb : uk\nDE UK'), { GB: 'UK', DE: 'UK' });
  assert.equal(parseAliases('GBR=UK'), null);
  assert.equal(parseAliases('GB=UK=DE'), null);
});
test('official playlist paths follow the published catalogue and never rewrite subdivisions', () => {
  const aliases = { GB: 'UK' };
  assert.equal(officialPlaylistPath('country', 'gb', aliases), 'countries/uk.m3u');
  assert.equal(officialPlaylistPath('country', 'XK', aliases), 'countries/xk.m3u');
  assert.equal(officialPlaylistPath('country', 'int', aliases), 'countries/int.m3u');
  assert.equal(officialPlaylistPath('country', 'undefined', aliases), 'countries/undefined.m3u');
  assert.equal(officialPlaylistPath('country', 'DEU', aliases), null);
  assert.equal(officialPlaylistPath('category', 'News', aliases), 'categories/news.m3u');
  assert.equal(officialPlaylistPath('category', 'bad slug', aliases), null);
  assert.equal(officialPlaylistPath('language', 'DEU', aliases), 'languages/deu.m3u');
  assert.equal(officialPlaylistPath('language', 'de', aliases), null);
  assert.equal(officialPlaylistPath('region', 'EUR', aliases), 'regions/eur.m3u');
  assert.equal(officialPlaylistPath('subdivision', 'GB-WLS', aliases), 'subdivisions/gb-wls.m3u');
  assert.equal(officialPlaylistPath('city', 'USNYC', aliases), 'cities/usnyc.m3u');
  assert.equal(officialPlaylistPath('source', 'us_pluto', aliases), 'sources/us_pluto.m3u');
  assert.equal(officialPlaylistPath('raw', '../etc', aliases), null);
  assert.equal(officialPlaylistPath('master', '', aliases), 'index.m3u');
  assert.equal(officialPlaylistPath('master', 'x', aliases), null);
});
test('the playlist method expands typed lists into ordered official picks', () => {
  const picks = officialPlaylists({
    ...defaultIptvOrgValues,
    master: 'index-country',
    countries: 'DE, TR',
    categories: 'news',
    cities: 'usnyc',
  });
  assert.deepEqual(picks, [
    { kind: 'index-country', code: '' },
    { kind: 'country', code: 'DE' },
    { kind: 'country', code: 'TR' },
    { kind: 'category', code: 'news' },
    { kind: 'city', code: 'usnyc' },
  ]);
  const built = buildIptvOrgConfig({ ...defaultIptvOrgValues, countries: 'DE' });
  assert.ok(built.ok);
  assert.equal(built.playlistBase, DEFAULT_PLAYLIST_BASE);
  assert.equal(built.config.method, 'playlist');
  assert.deepEqual(built.config.playlists, [{ kind: 'country', code: 'DE' }]);
  assert.deepEqual(built.config.geography.countries, []);
  const empty = buildIptvOrgConfig(defaultIptvOrgValues);
  assert.ok(!empty.ok);
});
test('the catalogue method requires countries and normalizes codes before IPC', () => {
  const built = buildIptvOrgConfig({
    ...defaultIptvOrgValues,
    method: 'catalogue',
    countries: 'de, gb',
    categories: 'News, Sports',
    languages: 'DEU',
    mode: 'coverage-with-regions',
    includeInternational: true,
    groupBy: 'category',
    refreshHours: '12',
    playlistBase: 'https://mirror.example/iptv',
  });
  assert.ok(built.ok);
  assert.equal(built.playlistBase, 'https://mirror.example/iptv');
  assert.deepEqual(built.config.geography.countries, ['DE', 'GB']);
  assert.deepEqual(built.config.geography.countryAliases, { GB: 'UK' });
  assert.deepEqual(built.config.filters.categories, ['news', 'sports']);
  assert.deepEqual(built.config.filters.languages, ['deu']);
  assert.deepEqual(built.config.playlist.groupBy, ['category']);
  assert.equal(built.config.cache.metadataRefreshHours, 12);
  for (const broken of [
    { countries: '' },
    { countries: 'DEU' },
    { countries: 'DE', languages: 'de' },
    { countries: 'DE', refreshHours: '0' },
    { countries: 'DE', apiBase: 'http://insecure.example/api' },
    { countries: 'DE', aliases: 'GB->UK' },
  ]) {
    const result = buildIptvOrgConfig({ ...defaultIptvOrgValues, method: 'catalogue', ...broken });
    assert.ok(!result.ok, JSON.stringify(broken));
  }
});
