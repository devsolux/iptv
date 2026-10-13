import { IonContent, IonHeader, IonPage, IonTitle, IonToolbar } from '@ionic/react';
import { useEffect, useState } from 'react';
import { NavLink, Outlet } from 'react-router';
import { dpadScope } from '../hooks/useDpadScope';
import { api, errorMessage, isPreview } from '../lib/bridge';
import { useUiStore } from '../store/ui';
import { Icon } from './Icon';

const links = [
  { to: '/channels', text: 'Live TV', icon: 'channels' },
  { to: '/guide', text: 'TV guide', icon: 'guide' },
  { to: '/countries', text: 'Countries', icon: 'globe' },
  { to: '/favorites', text: 'Favorites', icon: 'star' },
  { to: '/sources', text: 'Sources', icon: 'sources' },
] as const;
export function focusNavigation() {
  const nav = document.getElementById('primary-navigation');
  nav?.querySelector<HTMLElement>('[aria-current="page"]')?.focus();
}
export function focusPageStart() {
  const start = document.getElementById('page-focus-start');
  if (!start) return;
  const typing =
    start instanceof HTMLInputElement && !['checkbox', 'radio', 'range'].includes(start.type);
  // Remote-controlled screens: entering a page must not open the on-screen keyboard.
  if (typing && !window.matchMedia('(hover: hover)').matches) {
    const scope = start.closest('.page') ?? document.getElementById('workspace');
    const next = scope?.querySelector<HTMLElement>(
      'select[data-focus]:not([disabled]), .virtual-viewport [data-focus], .guide-row button, button[data-focus]:not([disabled])',
    );
    if (next) {
      next.focus();
      return;
    }
  }
  start.focus();
}
export function Shell() {
  useEffect(() => {
    focusNavigation();
  }, []);
  const [error, setError] = useState('');
  const [locking, setLocking] = useState(false);
  async function lock() {
    setLocking(true);
    setError('');
    try {
      await api.lock();
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      try {
        useUiStore.getState().setStatus(await api.status());
      } catch {
        /* Existing error remains visible. */
      }
      setLocking(false);
    }
  }
  return (
    <IonPage>
      <IonHeader className="ion-no-border">
        <IonToolbar className="app-toolbar">
          <IonTitle>
            <span className="brand">
              <img src="/icon.png" alt="" />
              IPTV<span className="brand-version">1.0</span>
            </span>
          </IonTitle>
          <div slot="end" className="header-actions">
            <span className="local-label">
              <span className="status-dot" />
              Local library
            </span>
            <button
              type="button"
              id="lock-vault"
              className="icon-button"
              aria-label="Lock vault"
              onKeyDown={(event) => {
                if (['ArrowDown', 'ArrowLeft'].includes(event.key)) {
                  event.preventDefault();
                  focusNavigation();
                }
              }}
              disabled={isPreview || locking}
              onClick={() => void lock()}
            >
              <Icon name="lock" />
            </button>
          </div>
        </IonToolbar>
      </IonHeader>
      <IonContent scrollY={false} className="shell-content">
        <div className="app-layout">
          <aside className="sidebar">
            <p className="nav-label">YOUR LIBRARY</p>
            <nav
              id="primary-navigation"
              aria-label="Primary navigation"
              onKeyDown={(event) => {
                if (
                  event.key === 'ArrowUp' &&
                  event.target === event.currentTarget.firstElementChild &&
                  !isPreview
                ) {
                  event.preventDefault();
                  document.getElementById('lock-vault')?.focus();
                } else if (event.key === 'ArrowRight') {
                  event.preventDefault();
                  focusPageStart();
                } else dpadScope(event);
              }}
            >
              {links.map((link) => (
                <NavLink
                  data-focus
                  key={link.to}
                  to={link.to}
                  className={({ isActive }) => `nav-item${isActive ? ' active' : ''}`}
                >
                  <Icon name={link.icon} />
                  <span>{link.text}</span>
                </NavLink>
              ))}
            </nav>
            <div className="sidebar-note">
              <span className="eyebrow">MADE FOR YOUR SCREEN</span>
              <p>
                Your channels.
                <br />
                Your guide.
              </p>
              <small>
                Arrow keys to explore.
                <br />
                Enter to watch.
              </small>
            </div>
          </aside>
          <main className="workspace" id="workspace">
            {isPreview && (
              <div className="preview-banner">
                Development preview · 50,000 synthetic channels · Native imports and playback are
                disabled.
              </div>
            )}
            {error && (
              <p className="inline-error" role="alert">
                {error}
              </p>
            )}
            <Outlet />
          </main>
        </div>
      </IonContent>
    </IonPage>
  );
}
