import type {
  IptvOrgConfig,
  IptvOrgCountryMode,
  IptvOrgGroupBy,
  IptvOrgMethod,
  IptvOrgPlaylistKind,
} from './contracts';

export const DEFAULT_API_BASE = 'https://iptv-org.github.io/api';
export const DEFAULT_PLAYLIST_BASE = 'https://iptv-org.github.io/iptv';

export type MasterIndex = '' | 'master' | 'index-country' | 'index-category' | 'index-language';
export type GroupChoice = 'country-category' | 'country' | 'category';

/** Everything the iptv-org form collects; Rust validates the resulting contract again. */
export interface IptvOrgFormValues {
  method: IptvOrgMethod;
  countries: string;
  categories: string;
  languages: string;
  regions: string;
  subdivisions: string;
  cities: string;
  sources: string;
  master: MasterIndex;
  mode: IptvOrgCountryMode;
  includeInternational: boolean;
  excludeNsfw: boolean;
  excludeBlocked: boolean;
  excludeClosed: boolean;
  excludeGeoBlockedLabel: boolean;
  excludeNot247Label: boolean;
  groupBy: GroupChoice;
  includeEveryAlternativeStream: boolean;
  emitVlcHttpOptions: boolean;
  refreshHours: string;
  aliases: string;
  apiBase: string;
  playlistBase: string;
}
export const defaultIptvOrgValues: IptvOrgFormValues = {
  method: 'playlist',
  countries: '',
  categories: '',
  languages: '',
  regions: '',
  subdivisions: '',
  cities: '',
  sources: '',
  master: '',
  mode: 'coverage-strict',
  includeInternational: false,
  excludeNsfw: true,
  excludeBlocked: true,
  excludeClosed: true,
  excludeGeoBlockedLabel: true,
  excludeNot247Label: true,
  groupBy: 'country-category',
  includeEveryAlternativeStream: true,
  emitVlcHttpOptions: true,
  refreshHours: '24',
  aliases: 'GB=UK',
  apiBase: '',
  playlistBase: '',
};

const COUNTRY = /^[A-Za-z]{2}$/;
const CATEGORY = /^[a-z][a-z0-9-]{0,63}$/;
const LANGUAGE = /^[a-z]{3}$/;
const BASE = /^https:\/\/[A-Za-z0-9.-]+(?::\d{1,5})?(?:\/[^\s?#]*)?$/;

/** Splits a typed list of codes on commas, semicolons and whitespace, keeping first-seen order. */
export function codeList(value: string): string[] {
  const seen = new Set<string>();
  const result: string[] = [];
  for (const raw of value.split(/[\s,;]+/)) {
    const code = raw.trim();
    const key = code.toLowerCase();
    if (!code || seen.has(key)) continue;
    seen.add(key);
    result.push(code);
  }
  return result;
}
/** Appends a picked code to a typed list unless it is already present. */
export function appendCode(current: string, code: string): string {
  const existing = codeList(current);
  if (existing.some((item) => item.toLowerCase() === code.toLowerCase())) return current;
  return [...existing, code].join(', ');
}
/** Parses alias lines such as `GB=UK`; returns null when a line is malformed. */
export function parseAliases(value: string): Record<string, string> | null {
  const aliases: Record<string, string> = {};
  for (const line of value.split(/\r?\n/)) {
    const text = line.trim();
    if (!text) continue;
    const [from, to, extra] = text.split(/\s*[=:]\s*|\s+/);
    if (!from || !to || extra !== undefined) return null;
    const source = from.toUpperCase();
    const target = to.toUpperCase();
    if (!COUNTRY.test(source) || !COUNTRY.test(target)) return null;
    aliases[source] = target;
  }
  return aliases;
}
export function canonicalCountry(code: string, aliases: Record<string, string>): string {
  const upper = code.trim().toUpperCase();
  return aliases[upper] ?? upper;
}
/** Relative path of an official playlist; mirrors the Rust builder, null when the code is invalid. */
export function officialPlaylistPath(
  kind: IptvOrgPlaylistKind,
  code: string,
  aliases: Record<string, string>,
): string | null {
  const value = code.trim();
  const fixed = (path: string) => (value ? null : path);
  switch (kind) {
    case 'master':
      return fixed('index.m3u');
    case 'index-country':
      return fixed('index.country.m3u');
    case 'index-category':
      return fixed('index.category.m3u');
    case 'index-language':
      return fixed('index.language.m3u');
    case 'country': {
      const country = canonicalCountry(value, aliases);
      return COUNTRY.test(country) || country === 'INT' || country === 'UNDEFINED'
        ? `countries/${country.toLowerCase()}.m3u`
        : null;
    }
    case 'category': {
      const id = value.toLowerCase();
      return CATEGORY.test(id) ? `categories/${id}.m3u` : null;
    }
    case 'language': {
      const id = value.toLowerCase();
      return LANGUAGE.test(id) || id === 'undefined' ? `languages/${id}.m3u` : null;
    }
    case 'region':
      return /^[A-Za-z0-9]{2,8}$/.test(value) ? `regions/${value.toLowerCase()}.m3u` : null;
    case 'subdivision':
      return /^[A-Za-z0-9]{2}-[A-Za-z0-9]{1,3}$/.test(value)
        ? `subdivisions/${value.toLowerCase()}.m3u`
        : null;
    case 'city':
      return /^[A-Za-z0-9]{3,12}$/.test(value) ? `cities/${value.toLowerCase()}.m3u` : null;
    case 'source':
    case 'raw': {
      const file = value.toLowerCase();
      return /^[a-z0-9_.-]{1,64}$/.test(file) && !file.includes('..')
        ? `${kind === 'source' ? 'sources' : 'raw'}/${file}.m3u`
        : null;
    }
  }
}
/** The official playlists a form selection expands to, in a stable order. */
export function officialPlaylists(
  values: IptvOrgFormValues,
): { kind: IptvOrgPlaylistKind; code: string }[] {
  const picks: { kind: IptvOrgPlaylistKind; code: string }[] = [];
  if (values.master) picks.push({ kind: values.master, code: '' });
  const lists: [IptvOrgPlaylistKind, string][] = [
    ['country', values.countries],
    ['category', values.categories],
    ['language', values.languages],
    ['region', values.regions],
    ['subdivision', values.subdivisions],
    ['city', values.cities],
    ['source', values.sources],
  ];
  for (const [kind, value] of lists) for (const code of codeList(value)) picks.push({ kind, code });
  return picks;
}

export type IptvOrgBuild =
  | { ok: true; config: IptvOrgConfig; playlistBase: string }
  | { ok: false; error: string };

/** Validates the form and produces the IPC contract. Rust re-validates every field. */
export function buildIptvOrgConfig(values: IptvOrgFormValues): IptvOrgBuild {
  const aliases = parseAliases(values.aliases);
  if (!aliases) return { ok: false, error: 'Country aliases use one GB=UK style line each.' };
  const hours = Number(values.refreshHours.trim());
  if (!Number.isInteger(hours) || hours < 1 || hours > 168)
    return { ok: false, error: 'Refresh iptv-org metadata between every 1 and 168 hours.' };
  const apiBase = values.apiBase.trim();
  const playlistBase = values.playlistBase.trim();
  for (const base of [apiBase, playlistBase])
    if (base && !BASE.test(base))
      return { ok: false, error: 'iptv-org base URLs must be plain HTTPS addresses.' };
  const playlists = values.method === 'playlist' ? officialPlaylists(values) : [];
  const countries =
    values.method === 'catalogue'
      ? codeList(values.countries).map((code) => code.toUpperCase())
      : [];
  if (values.method === 'playlist') {
    if (!playlists.length)
      return { ok: false, error: 'Select at least one official iptv-org playlist.' };
    if (playlists.length > 32)
      return { ok: false, error: 'Select at most 32 official playlists per source.' };
    for (const pick of playlists)
      if (officialPlaylistPath(pick.kind, pick.code, aliases) === null)
        return { ok: false, error: `“${pick.code}” is not a valid ${pick.kind} identifier.` };
  } else {
    if (!countries.length)
      return { ok: false, error: 'Select at least one country for a catalogue selection.' };
    for (const code of countries)
      if (!COUNTRY.test(code))
        return { ok: false, error: `“${code}” is not a two-letter country code.` };
  }
  const categories = codeList(values.categories).map((id) => id.toLowerCase());
  const languages = codeList(values.languages).map((id) => id.toLowerCase());
  if (values.method === 'catalogue') {
    for (const id of categories)
      if (!CATEGORY.test(id)) return { ok: false, error: `“${id}” is not a category ID.` };
    for (const code of languages)
      if (!LANGUAGE.test(code))
        return { ok: false, error: `“${code}” is not a three-letter language code.` };
  }
  const groupBy: IptvOrgGroupBy[] =
    values.groupBy === 'country-category' ? ['country', 'category'] : [values.groupBy];
  return {
    ok: true,
    playlistBase: playlistBase || DEFAULT_PLAYLIST_BASE,
    config: {
      apiBase,
      method: values.method,
      playlists,
      geography: {
        countries,
        mode: values.mode,
        includeInternational: values.includeInternational,
        countryAliases: aliases,
      },
      filters: {
        categories: values.method === 'catalogue' ? categories : [],
        languages: values.method === 'catalogue' ? languages : [],
        excludeNsfw: values.excludeNsfw,
        excludeBlocked: values.excludeBlocked,
        excludeClosed: values.excludeClosed,
        excludeGeoBlockedLabel: values.excludeGeoBlockedLabel,
        excludeNot247Label: values.excludeNot247Label,
      },
      playlist: {
        groupBy,
        includeEveryAlternativeStream: values.includeEveryAlternativeStream,
        emitVlcHttpOptions: values.emitVlcHttpOptions,
      },
      cache: { metadataRefreshHours: hours },
    },
  };
}
