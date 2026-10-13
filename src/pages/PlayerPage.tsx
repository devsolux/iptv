import { IonPage } from '@ionic/react';
import { type KeyboardEvent, useCallback, useEffect, useRef, useState } from 'react';
import { useLocation, useNavigate, useParams } from 'react-router';
import { Loading } from '../components/Feedback';
import { Icon } from '../components/Icon';
import { dpadScope } from '../hooks/useDpadScope';
import { usePlayer } from '../hooks/usePlayer';

const IDLE_MS = 3200;
const RETURN_TARGETS = ['/channels', '/guide', '/favorites', '/countries'];
const pad = (value: number) => String(value).padStart(2, '0');
function clock(seconds: number): string {
  const total = Math.max(0, Math.floor(seconds));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const rest = total % 60;
  return hours ? `${hours}:${pad(minutes)}:${pad(rest)}` : `${minutes}:${pad(rest)}`;
}

export default function PlayerPage() {
  const { channelId = '' } = useParams();
  const navigate = useNavigate();
  const location = useLocation();
  const video = useRef<HTMLVideoElement>(null);
  const surface = useRef<HTMLElement>(null);
  const [variant, setVariant] = useState(0);
  const tried = useRef(new Set<number>([0]));
  const [notice, setNotice] = useState('');
  const [fullscreen, setFullscreen] = useState(false);
  const [idle, setIdle] = useState(false);
  const idleTimer = useRef<number | null>(null);
  const routeState = location.state as { returnTo?: unknown; variants?: unknown } | null;
  const returnTo =
    typeof routeState?.returnTo === 'string' && RETURN_TARGETS.includes(routeState.returnTo)
      ? routeState.returnTo
      : '/channels';
  const knownVariants = typeof routeState?.variants === 'number' ? routeState.variants : 0;
  const back = useCallback(() => {
    navigate(returnTo, { replace: true });
  }, [navigate, returnTo]);
  const player = usePlayer(video, channelId, variant, back);
  const variants = player.session?.variants ?? knownVariants;

  // A stream that never started is retried on the next untried variant before giving up.
  useEffect(() => {
    if (player.phase !== 'error' || !player.startFailed) return;
    const next = Array.from({ length: variants }, (_, index) => index).find(
      (index) => !tried.current.has(index),
    );
    if (next === undefined) return;
    tried.current.add(next);
    setNotice(`Stream ${variant + 1} did not start. Trying stream ${next + 1} of ${variants}…`);
    setVariant(next);
  }, [player.phase, player.startFailed, variants, variant]);
  // biome-ignore lint/correctness/useExhaustiveDependencies: channelId is the reset trigger for the fallback state.
  useEffect(() => {
    tried.current = new Set([0]);
    setVariant(0);
    setNotice('');
  }, [channelId]);
  useEffect(() => {
    if (player.phase === 'playing') setNotice('');
  }, [player.phase]);

  const wake = useCallback(() => {
    setIdle(false);
    if (idleTimer.current) window.clearTimeout(idleTimer.current);
    idleTimer.current = window.setTimeout(() => setIdle(true), IDLE_MS);
  }, []);
  useEffect(() => {
    wake();
    return () => {
      if (idleTimer.current) window.clearTimeout(idleTimer.current);
    };
  }, [wake]);
  useEffect(() => {
    const sync = () => setFullscreen(Boolean(document.fullscreenElement));
    document.addEventListener('fullscreenchange', sync);
    return () => document.removeEventListener('fullscreenchange', sync);
  }, []);
  useEffect(() => {
    surface.current?.querySelector<HTMLElement>('#player-focus')?.focus();
  }, []);

  const hideable = player.phase === 'playing' || player.phase === 'buffering';
  const chromeHidden = idle && hideable;
  const paused = player.phase !== 'playing' && player.phase !== 'buffering';
  const { position } = player;
  const dvrWindow = position.seekableEnd - position.seekableStart;
  const dvr = player.live && dvrWindow > 60;
  const vod = !player.live && position.duration > 0;
  const behind = player.live && position.seekableEnd - position.current > 12;
  const title = player.session?.title ?? 'Opening channel';

  async function toggleFullscreen() {
    try {
      if (document.fullscreenElement) await document.exitFullscreen();
      else await surface.current?.requestFullscreen();
    } catch {
      setNotice('Fullscreen is unavailable in this WebView.');
    }
  }
  function focusControls() {
    const active = document.activeElement;
    if (!surface.current?.contains(active) || active === surface.current)
      surface.current?.querySelector<HTMLElement>('#player-focus')?.focus();
  }
  function keyDown(event: KeyboardEvent<HTMLElement>) {
    const target = event.target;
    const inField = target instanceof HTMLInputElement || target instanceof HTMLSelectElement;
    if (chromeHidden) {
      // The first key only reveals the controls; Back still flows to the navigation layer.
      wake();
      focusControls();
      if (event.key !== 'Escape') {
        event.preventDefault();
        event.stopPropagation();
      }
      return;
    }
    wake();
    if ((event.key === ' ' || event.key === 'MediaPlayPause') && !inField) {
      event.preventDefault();
      player.toggle();
    } else if (event.key === 'MediaPlay') {
      event.preventDefault();
      if (video.current?.paused) player.toggle();
    } else if (event.key === 'MediaPause') {
      event.preventDefault();
      if (!video.current?.paused) player.toggle();
    } else if (event.key === 'MediaRewind' || event.key === 'MediaTrackPrevious') {
      event.preventDefault();
      player.seek(-10);
    } else if (event.key === 'MediaFastForward' || event.key === 'MediaTrackNext') {
      event.preventDefault();
      player.seek(10);
    } else if (event.key === 'MediaStop') {
      event.preventDefault();
      back();
    } else if ((event.key === 'f' || event.key === 'F') && !inField) {
      event.preventDefault();
      void toggleFullscreen();
    } else if ((event.key === 'm' || event.key === 'M') && !inField) {
      event.preventDefault();
      player.mute();
    } else if (
      target instanceof HTMLInputElement &&
      target.type === 'range' &&
      (event.key === 'ArrowLeft' || event.key === 'ArrowRight')
    ) {
      /* Sliders adjust natively with left and right. */
    } else if (
      (event.key === 'ArrowLeft' || event.key === 'ArrowRight') &&
      target === event.currentTarget
    ) {
      event.preventDefault();
      player.seek(event.key === 'ArrowLeft' ? -10 : 10);
    } else dpadScope(event);
  }
  return (
    <IonPage>
      <section
        ref={surface}
        className={`player-page${fullscreen ? ' is-fullscreen' : ''}${chromeHidden ? ' is-idle' : ''}`}
        aria-label="Player"
        onKeyDown={keyDown}
        onMouseMove={wake}
        onPointerDown={wake}
      >
        <div className="video-stage">
          {/* biome-ignore lint/a11y/useMediaCaption: live captions arrive as stream text tracks exposed by the player. */}
          <video
            ref={video}
            playsInline
            preload="metadata"
            aria-label={player.session?.title ?? 'IPTV video player'}
            onClick={player.toggle}
            onDoubleClick={() => void toggleFullscreen()}
          />
          {(player.phase === 'opening' || player.phase === 'buffering') && (
            <div className={`player-overlay${player.phase === 'buffering' ? ' soft' : ''}`}>
              <Loading
                label={
                  player.session?.native
                    ? 'Playing in the native player…'
                    : player.phase === 'opening'
                      ? 'Opening a secure playback session…'
                      : 'Buffering…'
                }
              />
            </div>
          )}
          {(player.phase === 'error' || player.phase === 'gesture') && (
            <div className="player-overlay">
              <div className="player-message" role="alert">
                <p>{player.message}</p>
                <div className="player-message-actions">
                  <button
                    type="button"
                    data-focus
                    className="button primary"
                    onClick={player.phase === 'gesture' ? player.toggle : player.retry}
                  >
                    {player.phase === 'gesture' ? 'Play channel' : 'Retry channel'}
                  </button>
                  {player.phase === 'error' && variants > 1 && (
                    <button
                      type="button"
                      data-focus
                      className="button secondary"
                      onClick={() => {
                        const next = (variant + 1) % variants;
                        tried.current.add(next);
                        setVariant(next);
                      }}
                    >
                      Try stream {((variant + 1) % variants) + 1}
                    </button>
                  )}
                  <button type="button" data-focus className="button secondary" onClick={back}>
                    Back to channels
                  </button>
                </div>
              </div>
            </div>
          )}
          {notice && (
            <div className="player-notice" role="status">
              {notice}
            </div>
          )}
          <header className="player-chrome player-top">
            <button type="button" data-focus className="control-button" onClick={back}>
              <Icon name="back" size={20} />
              Back
            </button>
            <div className="player-title">
              <span className="eyebrow">{player.live ? 'LIVE TV' : 'NOW WATCHING'}</span>
              <h1>{title}</h1>
            </div>
            {player.live && (
              <span className="live-badge">
                <span />
                LIVE
              </span>
            )}
          </header>
          {!player.session?.native && (
            <footer
              className="player-chrome player-bottom"
              role="toolbar"
              aria-label="Playback controls"
            >
              {(dvr || vod) && (
                <input
                  data-focus
                  className="timeline"
                  type="range"
                  aria-label="Position"
                  min={0}
                  max={Math.max(1, vod ? position.duration : dvrWindow)}
                  step={1}
                  value={
                    vod ? position.current : Math.max(0, position.current - position.seekableStart)
                  }
                  onChange={(event) =>
                    player.seekTo(
                      vod
                        ? Number(event.target.value)
                        : position.seekableStart + Number(event.target.value),
                    )
                  }
                />
              )}
              <div className="control-row">
                <div className="control-group">
                  <button
                    type="button"
                    id="player-focus"
                    data-focus
                    className="control-button primary"
                    aria-label={paused ? 'Play' : 'Pause'}
                    title={paused ? 'Play' : 'Pause'}
                    onClick={player.toggle}
                    disabled={!player.session || player.phase === 'error'}
                  >
                    <Icon name={paused ? 'play' : 'pause'} size={26} />
                  </button>
                  <button
                    type="button"
                    data-focus
                    className="control-button"
                    aria-label="Back 10 seconds"
                    title="Back 10 seconds"
                    onClick={() => player.seek(-10)}
                    disabled={!player.session}
                  >
                    <Icon name="rewind" size={20} />
                  </button>
                  <button
                    type="button"
                    data-focus
                    className="control-button"
                    aria-label="Forward 10 seconds"
                    title="Forward 10 seconds"
                    onClick={() => player.seek(10)}
                    disabled={!player.session}
                  >
                    <Icon name="forward" size={20} />
                  </button>
                  {behind && (
                    <button
                      type="button"
                      data-focus
                      className="control-button accent"
                      onClick={player.liveEdge}
                    >
                      <Icon name="live" size={18} />
                      Go live
                    </button>
                  )}
                  <button
                    type="button"
                    data-focus
                    className="control-button"
                    aria-label={player.muted ? 'Unmute' : 'Mute'}
                    title={player.muted ? 'Unmute' : 'Mute'}
                    onClick={player.mute}
                  >
                    <Icon name={player.muted ? 'volumeOff' : 'volume'} size={20} />
                  </button>
                  <input
                    data-focus
                    className="volume"
                    type="range"
                    aria-label="Volume"
                    min={0}
                    max={1}
                    step={0.05}
                    value={player.muted ? 0 : player.volume}
                    onChange={(event) => player.setVolume(Number(event.target.value))}
                  />
                </div>
                <div className="control-time" aria-live="off">
                  {vod
                    ? `${clock(position.current)} / ${clock(position.duration)}`
                    : player.live
                      ? behind
                        ? `${clock(position.seekableEnd - position.current)} behind live`
                        : 'Live'
                      : ''}
                </div>
                <div className="control-group">
                  {variants > 1 && (
                    <label className="control-select">
                      Stream
                      <select
                        data-focus
                        aria-label="Stream variant"
                        value={variant}
                        onChange={(event) => {
                          const next = Number(event.target.value);
                          tried.current.add(next);
                          setVariant(next);
                        }}
                      >
                        {Array.from({ length: variants }, (_, index) => index).map((option) => (
                          <option key={option} value={option}>
                            {option + 1} of {variants}
                          </option>
                        ))}
                      </select>
                    </label>
                  )}
                  {player.audio.length > 1 && (
                    <label className="control-select">
                      Audio
                      <select
                        data-focus
                        aria-label="Audio track"
                        value={player.selectedAudio}
                        onChange={(event) => player.chooseAudio(Number(event.target.value))}
                      >
                        {player.audio.map((track) => (
                          <option key={track.index} value={track.index}>
                            {track.label}
                          </option>
                        ))}
                      </select>
                    </label>
                  )}
                  {player.subtitles.length > 0 && (
                    <label className="control-select">
                      Subtitles
                      <select
                        data-focus
                        aria-label="Subtitles"
                        value={player.selectedSubtitle}
                        onChange={(event) => player.chooseSubtitle(Number(event.target.value))}
                      >
                        <option value={-1}>Off</option>
                        {player.subtitles.map((track) => (
                          <option key={track.index} value={track.index}>
                            {track.label}
                          </option>
                        ))}
                      </select>
                    </label>
                  )}
                  <button
                    type="button"
                    data-focus
                    className="control-button"
                    aria-label={fullscreen ? 'Exit fullscreen' : 'Fullscreen'}
                    title={fullscreen ? 'Exit fullscreen' : 'Fullscreen'}
                    onClick={() => void toggleFullscreen()}
                  >
                    <Icon name={fullscreen ? 'fullscreenExit' : 'fullscreen'} size={20} />
                  </button>
                </div>
              </div>
            </footer>
          )}
        </div>
      </section>
    </IonPage>
  );
}
