import { invoke, isTauri } from '@tauri-apps/api/core';
import { z } from 'zod';
import {
  statusSchema, sourceSchema, channelPageSchema, guideSchema, playbackSchema, jobSchema, publicErrorSchema,
  type ChannelQuery, type GuideQuery, type SourceInput,
} from './contracts';

export const isPreview = import.meta.env.DEV && !isTauri();
export class BridgeError extends Error {
  readonly code: string;
  constructor(code: string, message: string) { super(message); this.name = 'BridgeError'; this.code = code; }
}
export function errorMessage(error: unknown): string {
  return error instanceof BridgeError ? error.message : 'The operation could not be completed. Please retry.';
}
async function call<T>(command: string, args: Record<string, unknown>, schema: z.ZodType<T>): Promise<T> {
  try {
    let result: unknown;
    if (isPreview) {
      // Compiled away in production. This is synthetic, read-only UI data, not an alternate parser.
      result = await (await import('./preview')).previewCommand(command, args);
    } else {
      if (!isTauri()) throw new BridgeError('RUNTIME', 'Start IPTV with the Tauri application, not as a standalone website.');
      result = await invoke<unknown>(command, args);
    }
    const parsed = schema.safeParse(result);
    if (!parsed.success) throw new BridgeError('CONTRACT', 'The application returned an incompatible response.');
    return parsed.data;
  } catch (error) {
    if (error instanceof BridgeError) throw error;
    const parsed = publicErrorSchema.safeParse(error);
    if (parsed.success) throw new BridgeError(parsed.data.code, parsed.data.message);
    // Never render native exception strings, which might contain URLs or credentials.
    throw new BridgeError('INTERNAL', 'The operation could not be completed. Please retry.');
  }
}
const empty = z.null().transform(() => undefined);
export const api = {
  status: () => call('app_status', {}, statusSchema),
  unlock: (passphrase: string) => call('unlock', { passphrase }, empty),
  lock: () => call('lock', {}, empty),
  exit: () => call('exit_app', {}, empty),
  jobs: () => call('list_jobs', {}, z.array(jobSchema).max(64)),
  countries: (sourceId: string | null) => call('query_countries', { sourceId }, z.array(z.object({ code: z.string().length(2), count: z.number().int().nonnegative() })).max(300)),
  locate: (query: ChannelQuery, channelId: string) => call('locate_channel', { query, channelId }, z.object({ index: z.number().int().nonnegative().nullable(), revision: z.number().int() })),
  sources: () => call('list_sources', {}, z.array(sourceSchema).max(32)),
  addSource: (input: SourceInput) => call('save_source', { input }, z.string().uuid()),
  removeSource: (sourceId: string) => call('remove_source', { sourceId }, empty),
  refreshSource: (sourceId: string) => call('refresh_source', { sourceId }, z.string().uuid()),
  job: (jobId: string) => call('job_status', { jobId }, jobSchema),
  cancelJob: (jobId: string) => call('cancel_job', { jobId }, empty),
  channels: (query: ChannelQuery) => call('query_channels', { query }, channelPageSchema),
  groups: (sourceId: string | null) => call('query_groups', { sourceId }, z.array(z.string()).max(512)),
  guide: (query: GuideQuery) => call('query_guide', { query }, guideSchema),
  favorite: (channelId: string, enabled: boolean) => call('set_favorite', { channelId, enabled }, empty),
  mapEpg: (channelId: string, feedId: string | null, epgId: string | null) => call('set_epg_mapping', { channelId, feedId, epgId }, empty),
  openPlayback: (channelId: string, variant = 0) => call('open_playback', { channelId, variant }, playbackSchema),
  playNative: (sessionId: string) => call('play_native', { sessionId }, empty),
  closePlayback: (sessionId: string) => call('close_playback', { sessionId }, empty),
};
