import { type RefObject, useCallback, useEffect, useRef, useState } from 'react';
import { api, errorMessage } from '../lib/bridge';
import type { Playback } from '../lib/contracts';

export interface Track {
  index: number;
  label: string;
}
type Phase = 'opening' | 'buffering' | 'playing' | 'paused' | 'gesture' | 'error';
interface PlayerState {
  phase: Phase;
  message: string;
  session: Playback | null;
  live: boolean;
  muted: boolean;
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
  session: null,
  live: false,
  muted: false,
  audio: [],
  subtitles: [],
  selectedAudio: 0,
  selectedSubtitle: -1,
};

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
  useEffect(() => {
    const element = video.current;
    if (!element) return;
    let disposed = false;
    let session: Playback | null = null;
    let releaseEngine: () => void = () => undefined;
    let failed = false;
    let mediaRecoveries = 0;
    setState(initial);
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
      patch({ phase: 'error', message });
      if (session) void api.closePlayback(session.sessionId).catch(() => undefined);
    };
    const autoplay = () => {
      void element.play().catch((error: unknown) => {
        if (disposed) return;
        if (error instanceof DOMException && error.name === 'NotAllowedError')
          patch({ phase: 'gesture', message: 'Press Play to start this channel.' });
        else fail('This device could not start the stream. Try another stream variant.');
      });
    };
    const playing = () => patch({ phase: 'playing', message: '' });
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
    const volume = () => patch({ muted: element.muted });
    const nativeError = () =>
      fail(
        'Playback is unavailable on this device. The stream may be offline or use an unsupported codec.',
      );
    const duration = () => {
      if (element.duration === Infinity || session?.kind === 'progressive')
        patch({ live: element.duration === Infinity });
    };
    element.addEventListener('playing', playing);
    element.addEventListener('pause', paused);
    element.addEventListener('waiting', waiting);
    element.addEventListener('volumechange', volume);
    element.addEventListener('error', nativeError);
    element.addEventListener('durationchange', duration);
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
              else
                fail(
                  'The stream stopped after bounded recovery attempts. Retry or choose another variant.',
                );
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
          player.on(dash.MediaPlayer.events.ERROR, () =>
            fail('DASH playback failed. Check device codec support or try another variant.'),
          );
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
      element.removeEventListener('playing', playing);
      element.removeEventListener('pause', paused);
      element.removeEventListener('waiting', waiting);
      element.removeEventListener('volumechange', volume);
      element.removeEventListener('error', nativeError);
      element.removeEventListener('durationchange', duration);
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
      void element
        .play()
        .catch(() =>
          setState((state) => ({
            ...state,
            phase: 'error',
            message: 'The stream could not resume. Retry the channel.',
          })),
        );
    else element.pause();
  }, [video]);
  const seek = useCallback(
    (delta: number) => {
      const element = video.current;
      if (!element || !element.seekable.length) return;
      const last = element.seekable.length - 1;
      element.currentTime = Math.max(
        element.seekable.start(last),
        Math.min(element.seekable.end(last) - 0.2, element.currentTime + delta),
      );
    },
    [video],
  );
  return {
    ...state,
    toggle,
    seek,
    retry: () => setAttempt((attempt) => attempt + 1),
    mute: () => {
      if (video.current) video.current.muted = !video.current.muted;
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
