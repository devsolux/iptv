import { z } from 'zod';

const id = z.string().uuid();
export const statusSchema = z.object({
  name: z.literal('IPTV'),
  version: z.literal('1.0.0'),
  identifier: z.literal('com.devsolux.iptv'),
  unlocked: z.boolean(),
  vaultExists: z.boolean(),
  nativePlayer: z.boolean(),
});
export const sourceSchema = z.object({
  id,
  name: z.string().max(128),
  kind: z.enum(['m3u', 'iptv-org']),
  channels: z.number().int().nonnegative(),
  updatedAt: z.number().int().nullable(),
  hasEpg: z.boolean(),
});
export const channelSchema = z.object({
  id,
  name: z.string().max(512),
  sourceId: id,
  country: z.string(),
  favorite: z.boolean(),
  variants: z.number().int().min(1).max(32),
  hasEpg: z.boolean(),
});
export const channelPageSchema = z.object({
  items: z.array(channelSchema).max(120),
  total: z.number().int().nonnegative(),
  revision: z.number().int().nonnegative(),
  offset: z.number().int().nonnegative(),
});
export const programmeSchema = z.object({
  channelId: id,
  start: z.number().int(),
  end: z.number().int(),
  // Rust truncates to 512 / 8192 characters; zod counts UTF-16 units (up to two per character).
  title: z.string().max(1024),
  description: z.string().max(32768),
});
export const guideSchema = z.object({
  items: z.array(programmeSchema).max(4000),
  revision: z.number().int(),
});
export const jobSchema = z.object({
  id,
  sourceId: id,
  state: z.enum(['queued', 'running', 'completed', 'cancelled', 'failed']),
  phase: z.string(),
  processed: z.number().int().nonnegative(),
  error: z.string().nullable(),
  createdAt: z.number().int(),
});
export const playbackSchema = z.object({
  sessionId: id,
  channelId: id,
  title: z.string(),
  url: z
    .string()
    .url()
    .refine((value) => {
      const url = new URL(value);
      return (
        url.protocol === 'http:' &&
        url.hostname === '127.0.0.1' &&
        url.pathname.startsWith('/media/')
      );
    }, 'The player requires a local session URL.'),
  kind: z.enum(['hls', 'dash', 'progressive']),
  native: z.boolean(),
  variants: z.number().int().min(1).max(32),
});
export const publicErrorSchema = z.object({ code: z.string(), message: z.string() });
// Rust clips names to 128 characters, flags to 8 and language codes to 8; zod counts UTF-16 units.
export const iptvOrgCatalogSchema = z.object({
  countries: z
    .array(
      z.object({
        code: z.string().length(2),
        name: z.string().max(256),
        flag: z.string().max(16),
        officialLanguageCodes: z.array(z.string().max(16)).max(32),
        playlistCandidateUrl: z.string().max(600),
      }),
    )
    .max(400),
  categories: z.array(z.object({ id: z.string().max(64), name: z.string().max(256) })).max(128),
  languages: z.array(z.object({ code: z.string().length(3), name: z.string().max(256) })).max(1024),
  regions: z
    .array(
      z.object({
        code: z.string().max(8),
        name: z.string().max(256),
        countries: z.array(z.string().length(2)).max(400),
      }),
    )
    .max(128),
});
export type AppStatus = z.infer<typeof statusSchema>;
export type Source = z.infer<typeof sourceSchema>;
export type Channel = z.infer<typeof channelSchema>;
export type ChannelPage = z.infer<typeof channelPageSchema>;
export type Programme = z.infer<typeof programmeSchema>;
export type Job = z.infer<typeof jobSchema>;
export type Playback = z.infer<typeof playbackSchema>;
export type IptvOrgCatalog = z.infer<typeof iptvOrgCatalogSchema>;
export interface ChannelFilters {
  search: string;
  sourceId: string | null;
  group: string | null;
  country: string | null;
  favoritesOnly: boolean;
}
export interface ChannelQuery extends ChannelFilters {
  offset: number;
  limit: number;
  revision: number | null;
}
export interface GuideQuery {
  channelIds: string[];
  start: number;
  end: number;
  revision: number;
}
export type IptvOrgMethod = 'playlist' | 'catalogue';
export type IptvOrgCountryMode = 'origin' | 'coverage-strict' | 'coverage-with-regions';
export type IptvOrgGroupBy = 'country' | 'category';
export type IptvOrgPlaylistKind =
  | 'master'
  | 'index-country'
  | 'index-category'
  | 'index-language'
  | 'country'
  | 'category'
  | 'language'
  | 'region'
  | 'subdivision'
  | 'city'
  | 'source'
  | 'raw';
/** Mirrors the Rust `iptv_org::Config` contract; the playlist base travels in `playlistUrl`. */
export interface IptvOrgConfig {
  apiBase: string;
  method: IptvOrgMethod;
  playlists: { kind: IptvOrgPlaylistKind; code: string }[];
  geography: {
    countries: string[];
    mode: IptvOrgCountryMode;
    includeInternational: boolean;
    countryAliases: Record<string, string>;
  };
  filters: {
    categories: string[];
    languages: string[];
    excludeNsfw: boolean;
    excludeBlocked: boolean;
    excludeClosed: boolean;
    excludeGeoBlockedLabel: boolean;
    excludeNot247Label: boolean;
  };
  playlist: {
    groupBy: IptvOrgGroupBy[];
    includeEveryAlternativeStream: boolean;
    emitVlcHttpOptions: boolean;
  };
  cache: { metadataRefreshHours: number };
}
export interface SourceInput {
  name: string;
  playlistUrl: string;
  epgUrls: string[];
  allowHttp: boolean;
  allowLan: boolean;
  cdnHosts: string[];
  countryCode: string;
  /** Null for own M3U playlists; their contract is otherwise unchanged. */
  iptvOrg: IptvOrgConfig | null;
}
