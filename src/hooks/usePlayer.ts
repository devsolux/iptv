import { type RefObject, useCallback, useEffect, useRef, useState } from 'react';
import { api, errorMessage } from '../lib/bridge';
import type { Playback } from '../lib/contracts';

export interface Track {
  index: number;
  label: string;
}
export type Phase = 'opening' | 'buffering' | 'playing' | 'paused' | 'gesture' | 'error';
export interface Position {
  current: number;
  duration: number;
  seekableStart: number;
  seekableEnd: number;
}
interface PlayerState {
  phase: Phase;
  message: string;
  /** The failure happened before any frame played, so another stream variant may help. */
  startFailed: boolean;
  session: Playback | null;
  live: boolean;
  muted: boolean;
  volume: number;
  position: Position;
  audio: Track[];
  subtitles: Track[];
  selectedAudio: number;
  selectedSubtitle: number;
}
interface TrackControls {
  audio: (index: number) => void;
  subtitle: (index: number) => void;
}
const initial: PlayerState = {
  phase: 'opening',
  message: '',
  startFailed: false,
  session: null,
  live: false,
  muted: false,
  volume: 1,
  position: { current: 0, duration: 0, seekableStart: 0, seekableEnd: 0 },
  audio: [],
  subtitles: [],
  selectedAudio: 0,
  selectedSubtitle: -1,
};
const CODEC =
  'This device cannot decode this stream (unsupported codec or container). Try another stream variant or channel.';
const FORMAT =
  'This stream format is not supported by this device’s player. Try another stream variant or channel.';
const LOST = 'The stream connection was lost or refused. Retry, or try another stream variant.';
function mediaErrorMessage(error: MediaError | null): string {
  switch (error?.code) {
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
function positionOf(element: HTMLVideoElement): Position {
  const last = element.seekable.length - 1;
  return {
    current: element.currentTime,
    duration: Number.isFinite(element.duration) ? element.duration : 0,
    seekableStart: last >= 0 ? element.seekable.start(last) : 0,
    seekableEnd: last >= 0 ? element.seekable.end(last) : 0,
  };
}

export function usePlayer(
  video: RefObject<HTMLVideoElement | null>,
  channelId: string,
  variant: number,
  onNativeEnd: () => void,
) {
  const [state, setState] = useState<PlayerState>(initial);
  const [attempt, setAttempt] = useState(0);
  const trackControls = useRef<TrackControls | null>(null);
  const onEnd = useRef(onNativeEnd);
  onEnd.current = onNativeEnd;
  // biome-ignore lint/correctness/useExhaustiveDependencies: attempt is the retry trigger that reopens the session.
  useEffect(() => {
    const element = video.current;
    if (!element) return;
    let disposed = false;
    let session: Playback | null = null;
    let releaseEngine: () => void = () => undefined;
    let failed = false;
    let everPlayed = false;
    let mediaRecoveries = 0;
    let triedMpegts = false;
    let lastTick = 0;
    setState({ ...initial, muted: element.muted, volume: element.volume });
    const patch = (change: Partial<PlayerState>) => {
      if (!disposed) setState((current) => ({ ...current, ...change }));
    };
    const fail = (message: string) => {
      if (disposed || failed) return;
      failed = true;
      const release = releaseEngine;
      releaseEngine = () => undefined;
      release();
      element.pause();
      patch({ phase: 'error', message, startFailed: !everPlayed });
      if (session) void api.closePlayback(session.sessionId).catch(() => undefined);
    };
    const autoplay = () => {
      void element.play().catch((error: unknown) => {
        if (disposed || !(error instanceof DOMException)) return;
        if (error.name === 'NotAllowedError')
          patch({ phase: 'gesture', message: 'Press Play to start this channel.' });
        // AbortError: a newer load interrupted this play request; the next load reports itself.
        else if (error.name === 'NotSupportedError') fail(FORMAT);
        else if (error.name !== 'AbortError')
          fail('This device could not start the stream. Try another stream variant.');
      });
    };
    const startMpegts = async (type: 'mpegts' | 'flv', url: string) => {
      const { default: mpegts } = await import('mpegts.js');
      if (disposed || failed) return;
      if (!mpegts.isSupported()) {
        fail('MPEG-TS playback is unavailable in this system WebView.');
        return;
      }
      releaseEngine();
      const player = mpegts.createPlayer(
        { type, isLive: true, url, cors: true, withCredentials: false },
        {
          enableWorker: false,
          enableStashBuffer: true,
          stashInitialSize: 384 * 1024,
          autoCleanupSourceBuffer: true,
          autoCleanupMaxBackwardDuration: 20,
          autoCleanupMinBackwardDuration: 10,
          liveBufferLatencyChasing: true,
          liveBufferLatencyMaxLatency: 8,
          liveBufferLatencyMinRemain: 2,
          lazyLoad: false,
        },
      );
      releaseEngine = () => {
        try {
          player.pause();
          player.unload();
          player.detachMediaElement();
          player.destroy();
        } catch {
          /* The engine is already gone. */
        }
        trackControls.current = null;
      };
      player.on(mpegts.Events.ERROR, (kind: string) =>
        fail(kind === mpegts.ErrorTypes.NETWORK_ERROR ? LOST : CODEC),
      );
      player.on(mpegts.Events.MEDIA_INFO, () => patch({ live: true }));
      player.attachMediaElement(element);
      player.load();
      autoplay();
    };
    const played = () =>
      setState((current) =>
        disposed || current.phase === 'error' || current.phase === 'playing'
          ? current
          : { ...current, phase: 'buffering' },
      );
    const playing = () => {
      everPlayed = true;
      patch({ phase: 'playing', message: '', position: positionOf(element) });
    };
    const paused = () => {
      if (!element.ended)
        setState((current) =>
          disposed || current.phase === 'error' ? current : { ...current, phase: 'paused' },
        );
    };
    const waiting = () =>
      setState((current) =>
        disposed || current.phase === 'error' ? current : { ...current, phase: 'buffering' },
      );
    const volume = () => patch({ muted: element.muted, volume: element.volume });
    const tick = () => {
      const now = Date.now();
      if (now - lastTick < 500) return;
      lastTick = now;
      patch({ position: positionOf(element) });
    };
    const nativeError = () => {
      const error = element.error;
      if (error?.code === 1) return; // Aborted by our own load() while switching.
      if (error?.code === 4 && session?.kind === 'progressive' && !triedMpegts && session) {
        // WebKit cannot play raw MPEG-TS in a <video>; remux it through MSE instead.
        triedMpegts = true;
        void startMpegts('mpegts', session.url);
        return;
      }
      fail(mediaErrorMessage(error));
    };
    const duration = () => {
      if (element.duration === Infinity || session?.kind === 'progressive')
        patch({ live: element.duration === Infinity });
      patch({ position: positionOf(element) });
    };
    element.addEventListener('play', played);
    element.addEventListener('playing', playing);
    element.addEventListener('pause', paused);
    element.addEventListener('waiting', waiting);
    element.addEventListener('volumechange', volume);
    element.addEventListener('error', nativeError);
    element.addEventListener('durationchange', duration);
    element.addEventListener('timeupdate', tick);
    element.addEventListener('progress', tick);
    const open = async () => {
      try {
        const prepared = await api.openPlayback(channelId, variant);
        if (disposed) {
          await api.closePlayback(prepared.sessionId);
          return;
        }
        session = prepared;
        patch({ session, phase: 'buffering' });
        if (prepared.native) {
          await api.playNative(prepared.sessionId);
          if (!disposed) onEnd.current();
          return;
        }
        if (prepared.kind === 'hls') {
          if (element.canPlayType('application/vnd.apple.mpegurl')) {
            element.src = prepared.url;
            releaseEngine = () => {
              element.removeAttribute('src');
              element.load();
            };
            autoplay();
          } else {
            const { default: Hls } = await import('hls.js');
            if (disposed) return;
            if (!Hls.isSupported()) {
              fail('HLS playback is unavailable in this system WebView.');
              return;
            }
            const hls = new Hls({
              enableWorker: true,
              lowLatencyMode: false,
              maxBufferLength: 12,
              maxMaxBufferLength: 20,
              maxBufferSize: 16 * 1024 * 1024,
              backBufferLength: 5,
              debug: false,
            });
            releaseEngine = () => {
              hls.destroy();
              trackControls.current = null;
            };
            hls.on(Hls.Events.MANIFEST_PARSED, autoplay);
            hls.on(Hls.Events.LEVEL_LOADED, (_event, data) => patch({ live: data.details.live }));
            hls.on(Hls.Events.AUDIO_TRACKS_UPDATED, () =>
              patch({
                audio: hls.audioTracks.map((track, index) => ({
                  index,
                  label: track.name || track.lang || `Audio ${index + 1}`,
                })),
              }),
            );
            hls.on(Hls.Events.SUBTITLE_TRACKS_UPDATED, () =>
              patch({
                subtitles: hls.subtitleTracks.map((track, index) => ({
                  index,
                  label: track.name || track.lang || `Subtitles ${index + 1}`,
                })),
              }),
            );
            hls.on(Hls.Events.ERROR, (_event, data) => {
              if (!data.fatal || disposed) return;
              if (data.type === Hls.ErrorTypes.MEDIA_ERROR && mediaRecoveries++ < 2)
                hls.recoverMediaError();
              else if (data.type === Hls.ErrorTypes.NETWORK_ERROR) fail(LOST);
              else fail(CODEC);
            });
            trackControls.current = {
              audio: (index) => {
                hls.audioTrack = index;
              },
              subtitle: (index) => {
                hls.subtitleTrack = index;
                hls.subtitleDisplay = index >= 0;
              },
            };
            hls.attachMedia(element);
            hls.loadSource(prepared.url);
          }
        } else if (prepared.kind === 'dash') {
          const dash = await import('dashjs');
          if (disposed) return;
          const player = dash.MediaPlayer().create();
          releaseEngine = () => {
            player.reset();
            trackControls.current = null;
          };
          player.updateSettings({
            debug: { logLevel: 0 },
            streaming: {
              buffer: {
                bufferTimeDefault: 12,
                bufferTimeAtTopQuality: 16,
                bufferTimeAtTopQualityLongForm: 16,
                bufferToKeep: 5,
              },
              retryAttempts: { MPD: 2, MediaSegment: 2, InitializationSegment: 2 },
            },
          });
          player.on(dash.MediaPlayer.events.STREAM_INITIALIZED, () => {
            patch({
              live: Boolean(player.isDynamic()),
              audio: player
                .getTracksFor('audio')
                .map((track, index) => ({ index, label: track.lang || `Audio ${index + 1}` })),
              subtitles: player
                .getTracksFor('text')
                .map((track, index) => ({ index, label: track.lang || `Subtitles ${index + 1}` })),
            });
            autoplay();
          });
          player.on(dash.MediaPlayer.events.ERROR, () => fail(CODEC));
          trackControls.current = {
            audio: (index) => {
              const track = player.getTracksFor('audio')[index];
              if (track) player.setCurrentTrack(track);
            },
            subtitle: (index) => {
              player.enableText(index >= 0);
              if (index >= 0) player.setTextTrack(index);
            },
          };
          player.initialize(element, prepared.url, false);
        } else if (prepared.kind === 'mpegts' || prepared.kind === 'flv') {
          triedMpegts = true;
          await startMpegts(prepared.kind, prepared.url);
        } else {
          element.src = prepared.url;
          releaseEngine = () => {
            element.removeAttribute('src');
            element.load();
          };
          autoplay();
        }
      } catch (error) {
        if (!disposed) fail(errorMessage(error));
      }
    };
    void open();
    const visibility = () => {
      if (document.visibilityState === 'hidden' && !session?.native) element.pause();
    };
    document.addEventListener('visibilitychange', visibility);
    return () => {
      disposed = true;
      document.removeEventListener('visibilitychange', visibility);
      element.removeEventListener('play', played);
      element.removeEventListener('playing', playing);
      element.removeEventListener('pause', paused);
      element.removeEventListener('waiting', waiting);
      element.removeEventListener('volumechange', volume);
      element.removeEventListener('error', nativeError);
      element.removeEventListener('durationchange', duration);
      element.removeEventListener('timeupdate', tick);
      element.removeEventListener('progress', tick);
      releaseEngine();
      trackControls.current = null;
      element.pause();
      element.removeAttribute('src');
      element.load();
      if (session) void api.closePlayback(session.sessionId).catch(() => undefined);
    };
  }, [video, channelId, variant, attempt]);
  const toggle = useCallback(() => {
    const element = video.current;
    if (!element) return;
    if (element.paused)
      void element.play().catch((error: unknown) => {
        if (error instanceof DOMException && error.name === 'AbortError') return;
        setState((state) => ({
          ...state,
          phase: 'error',
          message: 'The stream could not resume. Retry the channel.',
        }));
      });
    else element.pause();
  }, [video]);
  const seekTo = useCallback(
    (time: number) => {
      const element = video.current;
      if (!element?.seekable.length) return;
      const last = element.seekable.length - 1;
      element.currentTime = Math.max(
        element.seekable.start(last),
        Math.min(element.seekable.end(last) - 0.2, time),
      );
    },
    [video],
  );
  const seek = useCallback(
    (delta: number) => {
      if (video.current) seekTo(video.current.currentTime + delta);
    },
    [video, seekTo],
  );
  return {
    ...state,
    toggle,
    seek,
    seekTo,
    retry: () => setAttempt((attempt) => attempt + 1),
    mute: () => {
      if (video.current) video.current.muted = !video.current.muted;
    },
    setVolume: (level: number) => {
      const element = video.current;
      if (!element) return;
      element.volume = Math.max(0, Math.min(1, level));
      element.muted = element.volume === 0;
    },
    liveEdge: () => {
      const element = video.current;
      if (element?.seekable.length)
        element.currentTime = Math.max(0, element.seekable.end(element.seekable.length - 1) - 1);
    },
    chooseAudio: (index: number) => {
      trackControls.current?.audio(index);
      setState((state) => ({ ...state, selectedAudio: index }));
    },
    chooseSubtitle: (index: number) => {
      trackControls.current?.subtitle(index);
      setState((state) => ({ ...state, selectedSubtitle: index }));
    },
  };
}
