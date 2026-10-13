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
export type AppStatus = z.infer<typeof statusSchema>;
export type Source = z.infer<typeof sourceSchema>;
export type Channel = z.infer<typeof channelSchema>;
export type ChannelPage = z.infer<typeof channelPageSchema>;
export type Programme = z.infer<typeof programmeSchema>;
export type Job = z.infer<typeof jobSchema>;
export type Playback = z.infer<typeof playbackSchema>;
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
export interface SourceInput {
  name: string;
  playlistUrl: string;
  epgUrls: string[];
  allowHttp: boolean;
  allowLan: boolean;
  cdnHosts: string[];
  countryCode: string;
}
