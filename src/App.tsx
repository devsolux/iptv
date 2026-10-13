import { IonApp } from '@ionic/react';
import { isTauri } from '@tauri-apps/api/core';
import {
  Component,
  type ErrorInfo,
  lazy,
  type ReactNode,
  Suspense,
  useEffect,
  useState,
} from 'react';
import { HashRouter, Navigate, Route, Routes, useLocation, useNavigate } from 'react-router';
import { ErrorNotice, Loading } from './components/Feedback';
import { Modal } from './components/Modal';
import { Shell } from './components/Shell';
import { api, errorMessage, isPreview } from './lib/bridge';
import ChannelsPage from './pages/ChannelsPage';
import { useUiStore } from './store/ui';

const GuidePage = lazy(() => import('./pages/GuidePage'));
const CountriesPage = lazy(() => import('./pages/CountriesPage'));
const SourcesPage = lazy(() => import('./pages/SourcesPage'));
const PlayerPage = lazy(() => import('./pages/PlayerPage'));

class ErrorBoundary extends Component<{ children: ReactNode }, { failed: boolean }> {
  state = { failed: false };
  static getDerivedStateFromError() {
    return { failed: true };
  }
  componentDidCatch(_error: Error, _info: ErrorInfo) {
    /* Do not log application state or credentials. */
  }
  render() {
    if (this.state.failed)
      return (
        <div className="startup-feedback">
          <ErrorNotice
            message="The interface encountered an unexpected error. Your stored catalogue has not been reset."
            onRetry={() => window.location.reload()}
          />
        </div>
      );
    return this.props.children;
  }
}
function NavigationLifecycle() {
  const navigate = useNavigate();
  const location = useLocation();
  const [exit, setExit] = useState(false);
  const [error, setError] = useState('');
  useEffect(() => {
    function back() {
      const dialog = document.querySelector<HTMLDialogElement>('dialog[open]');
      if (dialog) {
        dialog.dispatchEvent(
          new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }),
        );
        return;
      }
      const active = document.activeElement;
      if (
        (active instanceof HTMLInputElement &&
          !['checkbox', 'radio', 'range'].includes(active.type)) ||
        active instanceof HTMLTextAreaElement ||
        active instanceof HTMLSelectElement
      ) {
        // A remote's Back first leaves the field and its on-screen keyboard.
        active.blur();
        return;
      }
      if (location.pathname === '/channels') setExit(true);
      else if (location.pathname.startsWith('/play/')) {
        const returnTo: unknown = location.state?.returnTo;
        navigate(
          typeof returnTo === 'string' &&
            ['/channels', '/guide', '/favorites', '/countries'].includes(returnTo)
            ? returnTo
            : '/channels',
          { replace: true },
        );
      } else if (typeof window.history.state?.idx === 'number' && window.history.state.idx > 0)
        navigate(-1);
      else navigate('/channels', { replace: true });
    }
    const keyboard = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !event.defaultPrevented) {
        event.preventDefault();
        back();
      }
    };
    window.addEventListener('iptv-native-back', back);
    window.addEventListener('keydown', keyboard);
    return () => {
      window.removeEventListener('iptv-native-back', back);
      window.removeEventListener('keydown', keyboard);
    };
  }, [location.pathname, location.state, navigate]);
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    if (isTauri())
      void import('@tauri-apps/api/event')
        .then(({ listen }) => listen('catalogue-changed', () => useUiStore.getState().invalidate()))
        .then((stop) => {
          if (disposed) stop();
          else unlisten = stop;
        })
        .catch(() => undefined);
    const resume = () => {
      if (document.visibilityState === 'visible') useUiStore.getState().invalidate();
    };
    document.addEventListener('visibilitychange', resume);
    return () => {
      disposed = true;
      unlisten?.();
      document.removeEventListener('visibilitychange', resume);
    };
  }, []);
  async function close() {
    try {
      await api.exit();
    } catch (error) {
      setError(errorMessage(error));
    }
  }
  return exit ? (
    <Modal title="Close IPTV?" onClose={() => setExit(false)}>
      <p>
        {isPreview
          ? 'Close this browser tab to leave the development preview.'
          : 'Your catalogue is saved locally. Active imports will stop and can be restarted next time.'}
      </p>
      {error && <ErrorNotice message={error} />}
      <div className="modal-actions">
        <button
          type="button"
          data-focus
          className="button secondary"
          onClick={() => setExit(false)}
        >
          Keep watching
        </button>
        <button
          type="button"
          data-focus
          className="button primary"
          disabled={isPreview}
          onClick={() => void close()}
        >
          Close IPTV
        </button>
      </div>
    </Modal>
  ) : null;
}
function ApplicationRoutes() {
  return (
    <>
      <NavigationLifecycle />
      <Suspense fallback={<Loading />}>
        <Routes>
          <Route element={<Shell />}>
            <Route path="/channels" element={<ChannelsPage />} />
            <Route path="/favorites" element={<ChannelsPage favoritesOnly />} />
            <Route path="/guide" element={<GuidePage />} />
            <Route path="/countries" element={<CountriesPage />} />
            <Route path="/sources" element={<SourcesPage />} />
          </Route>
          <Route path="/play/:channelId" element={<PlayerPage />} />
          <Route path="*" element={<Navigate to="/channels" replace />} />
        </Routes>
      </Suspense>
    </>
  );
}
export default function App() {
  const status = useUiStore((s) => s.status);
  const [error, setError] = useState('');
  const [attempt, setAttempt] = useState(0);
  // biome-ignore lint/correctness/useExhaustiveDependencies: attempt is the retry trigger, not a value the effect reads.
  useEffect(() => {
    let active = true;
    void api
      .status()
      .then((status) => {
        if (active) {
          useUiStore.getState().setStatus(status);
          setError('');
        }
      })
      .catch((error: unknown) => {
        if (active) setError(errorMessage(error));
      });
    return () => {
      active = false;
    };
  }, [attempt]);
  return (
    <IonApp>
      <ErrorBoundary>
        {!status ? (
          <div className="startup-feedback">
            {error ? (
              <ErrorNotice message={error} onRetry={() => setAttempt((attempt) => attempt + 1)} />
            ) : (
              <Loading label="Opening IPTV…" />
            )}
          </div>
        ) : (
          <HashRouter>
            <ApplicationRoutes />
          </HashRouter>
        )}
      </ErrorBoundary>
    </IonApp>
  );
}
