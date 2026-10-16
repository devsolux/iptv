/** Player failure messages and recovery rules shared by the desktop engines. */

export const CODEC =
  'This device cannot decode this stream (unsupported codec or container). Try another stream variant or channel.';
export const FORMAT =
  'This stream format is not supported by this device’s player. Try another stream variant or channel.';
export const LOST =
  'The stream connection was lost or refused. Retry, or try another stream variant.';
export const POLICY =
  'A stream address was blocked by this source’s network policy. Check the source’s HTTP and certificate settings.';
export const PROTECTED =
  'This stream is protected (DRM) and cannot be played here. Try another stream variant or channel.';
export const TOO_LARGE = 'This stream’s playlist exceeds the player’s size limit.';

/**
 * Message for a failed relay request, keyed by its HTTP status. The relay answers 415 for
 * manifests it cannot rewrite (DRM, variables), 403 for addresses the source policy rejects
 * and 413 for oversized playlists; anything else is the upstream host or the connection.
 */
export function relayFailure(status: number): string {
  switch (status) {
    case 403:
      return POLICY;
    case 413:
      return TOO_LARGE;
    case 415:
      return FORMAT;
    default:
      return LOST;
  }
}

/** A relay failure that another attempt cannot fix. */
export function permanentRelayFailure(status: number): boolean {
  return status === 403 || status === 413 || status === 415;
}

/** dash.js error codes, from `MediaPlayerErrors` in dash.js 5. */
const DASH_IGNORED = new Set([
  16, // TIME_SYNC_FAILED: the player falls back to the device clock.
  21, // REMOVE_ERROR: buffer pruning failed; playback continues.
  33, // TIMED_TEXT_ERROR_ID_PARSE: only the subtitles are lost.
]);
const DASH_NETWORK = new Set([11, 12, 15, 17, 18, 19, 25, 26, 27, 28, 29]);
const DASH_FORMAT = new Set([10, 31, 32, 34]);
const DASH_CODEC = new Set([20, 22, 23, 35]);

/** The numeric code of a dash.js ERROR event, whose `error` may also be a legacy string. */
export function dashErrorCode(event: unknown): number | null {
  if (typeof event !== 'object' || event === null || !('error' in event)) return null;
  const error = (event as { error: unknown }).error;
  if (typeof error !== 'object' || error === null || !('code' in error)) return null;
  const code = (error as { code: unknown }).code;
  return typeof code === 'number' ? code : null;
}

/**
 * Message for a dash.js ERROR event, or `null` when the event is not fatal. dash.js reports
 * clock synchronisation and subtitle problems through the same event as fatal failures.
 */
export function dashFailure(code: number | null): string | null {
  if (code === null) return FORMAT;
  if (DASH_IGNORED.has(code)) return null;
  if (DASH_NETWORK.has(code)) return LOST;
  if (DASH_FORMAT.has(code)) return FORMAT;
  if (DASH_CODEC.has(code)) return CODEC;
  if (code === 24 || (code >= 100 && code < 200)) return PROTECTED;
  return FORMAT;
}

/** Message for an HTMLMediaElement error code. */
export function mediaFailure(code: number | undefined): string {
  switch (code) {
    case 2:
      return LOST;
    case 3:
      return CODEC;
    case 4:
      return FORMAT;
    default:
      return 'Playback stopped unexpectedly. Retry, or try another stream variant.';
  }
}

/**
 * Bounds automatic reconnection of a stream that was playing: at most `limit` attempts per
 * sliding `windowMs`, so a stream that drops now and then keeps recovering while one that
 * fails repeatedly surfaces its error.
 */
export class ReconnectBudget {
  private attempts: number[] = [];
  private readonly limit: number;
  private readonly windowMs: number;
  constructor(limit = 3, windowMs = 120_000) {
    this.limit = limit;
    this.windowMs = windowMs;
  }
  take(now: number): boolean {
    this.attempts = this.attempts.filter((time) => now - time < this.windowMs);
    if (this.attempts.length >= this.limit) return false;
    this.attempts.push(now);
    return true;
  }
  reset() {
    this.attempts = [];
  }
}
