import { IonPage } from '@ionic/react';
import { useCallback, useEffect, useRef, useState } from 'react';
import { useLocation, useNavigate, useParams } from 'react-router';
import { ErrorNotice, Loading } from '../components/Feedback';
import { Icon } from '../components/Icon';
import { dpadScope } from '../hooks/useDpadScope';
import { usePlayer } from '../hooks/usePlayer';
export default function PlayerPage() {
  const { channelId = '' } = useParams();
  const navigate = useNavigate();
  const location = useLocation();
  const video = useRef<HTMLVideoElement>(null);
  const surface = useRef<HTMLDivElement>(null);
  useEffect(() => {
    surface.current?.querySelector<HTMLButtonElement>('button')?.focus();
  }, []);
  const [variant, setVariant] = useState(0);
  const [fullscreenError, setFullscreenError] = useState('');
  const returnTo =
    typeof location.state?.returnTo === 'string' &&
    ['/channels', '/guide', '/favorites', '/countries'].includes(location.state.returnTo)
      ? location.state.returnTo
      : '/channels';
  const back = useCallback(() => {
    navigate(returnTo, { replace: true });
  }, [navigate, returnTo]);
  const player = usePlayer(video, channelId, variant, back);
  async function fullscreen() {
    setFullscreenError('');
    try {
      if (document.fullscreenElement) await document.exitFullscreen();
      else await surface.current?.requestFullscreen();
    } catch {
      setFullscreenError('Fullscreen is unavailable in this WebView.');
    }
  }
  return (
    <IonPage>
      <div
        ref={surface}
        className="player-page"
        onKeyDown={(event) => {
          if (event.key === ' ' || event.key === 'MediaPlayPause') {
            event.preventDefault();
            player.toggle();
          } else if (event.key === 'MediaPlay') {
            event.preventDefault();
            if (video.current?.paused) player.toggle();
          } else if (event.key === 'MediaPause') {
            event.preventDefault();
            if (!video.current?.paused) player.toggle();
          } else dpadScope(event);
        }}
      >
        <header className="player-heading">
          <button type="button" data-focus className="button quiet" onClick={back}>
            <Icon name="back" />
            Back
          </button>
          <div>
            <span className="eyebrow">{player.live ? 'LIVE TV' : 'NOW WATCHING'}</span>
            <h1>{player.session?.title ?? 'Opening channel'}</h1>
          </div>
          {player.live && (
            <span className="live-badge">
              <span />
              LIVE
            </span>
          )}
        </header>
        <div className="video-stage">
          <video
            ref={video}
            playsInline
            preload="metadata"
            aria-label={player.session?.title ?? 'IPTV video player'}
          />
          {(player.phase === 'opening' || player.phase === 'buffering') && (
            <div className="player-overlay">
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
              <div className="player-message">
                <p>{player.message}</p>
                <button
                  type="button"
                  data-focus
                  className="button primary"
                  onClick={player.phase === 'gesture' ? player.toggle : player.retry}
                >
                  {player.phase === 'gesture' ? 'Play channel' : 'Retry channel'}
                </button>
              </div>
            </div>
          )}
        </div>
        {!player.session?.native && (
          <footer className="player-controls">
            <div className="transport-controls">
              <button
                type="button"
                data-focus
                className="button primary"
                onClick={player.toggle}
                disabled={!player.session || player.phase === 'error'}
              >
                {player.phase === 'playing' || player.phase === 'buffering' ? 'Pause' : 'Play'}
              </button>
              <button
                type="button"
                data-focus
                className="button secondary"
                onClick={() => player.seek(-10)}
                disabled={!player.session}
              >
                −10s
              </button>
              <button
                type="button"
                data-focus
                className="button secondary"
                onClick={() => player.seek(10)}
                disabled={!player.session}
              >
                +10s
              </button>
              {player.live && (
                <button
                  type="button"
                  data-focus
                  className="button secondary"
                  onClick={player.liveEdge}
                >
                  Live edge
                </button>
              )}
              <button type="button" data-focus className="button secondary" onClick={player.mute}>
                {player.muted ? 'Unmute' : 'Mute'}
              </button>
              <button
                type="button"
                data-focus
                className="button secondary"
                onClick={() => void fullscreen()}
              >
                Fullscreen
              </button>
            </div>
            <div className="track-controls">
              {player.session && player.session.variants > 1 && (
                <label>
                  Stream
                  <select
                    data-focus
                    value={variant}
                    onChange={(event) => setVariant(Number(event.target.value))}
                  >
                    {Array.from({ length: player.session.variants }, (_, index) => (
                      <option key={index} value={index}>
                        Variant {index + 1}
                      </option>
                    ))}
                  </select>
                </label>
              )}
              {player.audio.length > 1 && (
                <label>
                  Audio
                  <select
                    data-focus
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
                <label>
                  Subtitles
                  <select
                    data-focus
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
            </div>
            {fullscreenError && <ErrorNotice message={fullscreenError} />}
          </footer>
        )}
      </div>
    </IonPage>
  );
}
