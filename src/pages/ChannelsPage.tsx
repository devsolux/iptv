import { useVirtualizer } from '@tanstack/react-virtual';
import { type CSSProperties, useEffect, useMemo, useRef, useState } from 'react';
import { Link, useLocation, useNavigate } from 'react-router';
import { Empty, ErrorNotice, Loading } from '../components/Feedback';
import { Filters } from '../components/Filters';
import { Flag } from '../components/Flag';
import { Icon } from '../components/Icon';
import { Modal } from '../components/Modal';
import { focusNavigation, focusPageStart } from '../components/Shell';
import { useCatalogue } from '../hooks/useCatalogue';
import { useColumns } from '../hooks/useColumns';
import { useDpadFocus } from '../hooks/useDpadFocus';
import { dpadScope } from '../hooks/useDpadScope';
import { useFilters } from '../hooks/useFilters';
import { api, errorMessage, isPreview } from '../lib/bridge';
import type { Channel } from '../lib/contracts';
import { useUiStore } from '../store/ui';

export default function ChannelsPage({ favoritesOnly = false }: { favoritesOnly?: boolean }) {
  const filters = useFilters(favoritesOnly);
  const scope = JSON.stringify({ view: 'channels', filters });
  const bookmark = useMemo(() => useUiStore.getState().focus[scope], [scope]);
  const catalogue = useCatalogue(filters, bookmark?.index ?? 0);
  const viewport = useRef<HTMLElement>(null);
  const columns = useColumns(viewport);
  const navigate = useNavigate();
  const location = useLocation();
  const [action, setAction] = useState<Channel | null>(null);
  const [actionError, setActionError] = useState('');
  const [saving, setSaving] = useState(false);
  const pendingRestore = useRef<number | null>(null);
  const virtual = useVirtualizer({
    count: Math.ceil(catalogue.total / columns),
    getScrollElement: () => viewport.current,
    estimateSize: () => 188,
    overscan: 2,
  });
  const items = virtual.getVirtualItems();
  const first = items[0]?.index ?? 0;
  const last = items.at(-1)?.index ?? 0;
  const focus = useDpadFocus({
    container: viewport,
    count: catalogue.total,
    columns,
    initialIndex: bookmark?.index ?? 0,
    scrollTo: (index) => virtual.scrollToIndex(Math.floor(index / columns), { align: 'auto' }),
    request: (index) => catalogue.pager.wantRange(index, index + columns),
    onFocus: (index) => {
      const channel = catalogue.pager.at(index);
      if (channel) useUiStore.getState().rememberFocus(scope, { index, id: channel.id });
    },
    updateToken: catalogue.pages,
  });
  useEffect(() => {
    if (catalogue.total) catalogue.pager.wantRange(first * columns, (last + 1) * columns - 1);
  }, [catalogue.pager, catalogue.total, first, last, columns]);
  useEffect(() => {
    if (!bookmark) return;
    let active = true;
    void api
      .locate({ ...filters, offset: 0, limit: 1, revision: null }, bookmark.id)
      .then((result) => {
        if (active) {
          const index = result.index ?? 0;
          pendingRestore.current = index;
          catalogue.pager.reset(filters, index);
        }
      })
      .catch(() => {
        if (active) pendingRestore.current = 0;
      });
    return () => {
      active = false;
    };
  }, [bookmark, filters, catalogue.pager]);
  // biome-ignore lint/correctness/useExhaustiveDependencies: each loaded page may make the restored index focusable.
  useEffect(() => {
    if (pendingRestore.current !== null && catalogue.ready && catalogue.total) {
      const index = pendingRestore.current;
      pendingRestore.current = null;
      focus.focusIndex(index);
    }
  }, [catalogue.ready, catalogue.total, catalogue.pages, focus.focusIndex]);
  const play = (channel: Channel) =>
    navigate(`/play/${channel.id}`, { state: { returnTo: location.pathname } });
  const openActions = () => {
    const channel = catalogue.pager.at(focus.selected);
    if (channel) {
      setActionError('');
      setAction(channel);
    }
  };
  async function favorite() {
    if (!action) return;
    setSaving(true);
    setActionError('');
    try {
      await api.favorite(action.id, !action.favorite);
      useUiStore.getState().invalidate();
      setAction(null);
    } catch (error) {
      setActionError(errorMessage(error));
    } finally {
      setSaving(false);
    }
  }
  return (
    <section className="catalogue-page page" data-cached-pages={catalogue.pages.size}>
      <header className="page-heading">
        <div>
          <span className="eyebrow">
            {favoritesOnly ? 'YOUR SHORTLIST' : 'FIND SOMETHING GOOD'}
          </span>
          <h1>{favoritesOnly ? 'Favorites' : 'Live TV'}</h1>
          <p className="muted">
            {catalogue.ready
              ? `${catalogue.total.toLocaleString()} channels in your library`
              : 'Your next channel is here.'}
          </p>
        </div>
        <div
          className="heading-actions"
          id="page-actions"
          role="toolbar"
          aria-label="Page actions"
          onKeyDown={(event) => {
            if (event.key === 'ArrowDown') {
              event.preventDefault();
              focusPageStart();
            } else dpadScope(event);
          }}
        >
          <button
            type="button"
            data-focus
            className="icon-button"
            aria-label="Reload catalogue"
            onClick={() => useUiStore.getState().invalidate()}
          >
            <Icon name="refresh" />
          </button>
          <button
            type="button"
            data-focus
            className="button secondary"
            onClick={openActions}
            disabled={!catalogue.total}
          >
            Channel actions <Icon name="more" size={18} />
          </button>
        </div>
      </header>
      <Filters onEnterContent={() => focus.focusIndex(focus.selected)} />
      {catalogue.error ? (
        <ErrorNotice
          message={errorMessage(catalogue.error)}
          onRetry={() => catalogue.pager.reset(filters)}
        />
      ) : null}
      {!catalogue.ready && <Loading />}
      {catalogue.ready && !catalogue.total && !catalogue.error && (
        <Empty title={favoritesOnly ? 'Keep your favorites close.' : 'Your library starts here.'}>
          <p>
            {favoritesOnly
              ? 'Use Channel actions to save the channels you watch most.'
              : 'Add an M3U source or clear your filters to find channels.'}
          </p>
          <Link className="button primary" to="/sources">
            Manage sources
          </Link>
        </Empty>
      )}
      <section
        ref={viewport}
        className="virtual-viewport channel-viewport"
        // biome-ignore lint/a11y/noNoninteractiveTabindex: a scrollable region must stay keyboard-focusable and is the focus fallback for virtualized cards.
        tabIndex={0}
        aria-label="Channel grid"
        data-roving-container
        onKeyDown={(event) => {
          if (event.key === 'ContextMenu' || event.key.toLowerCase() === 'm') {
            event.preventDefault();
            openActions();
            return;
          }
          if (event.key === 'Enter' && event.target === event.currentTarget) {
            const channel = catalogue.pager.at(focus.selected);
            if (channel) play(channel);
            return;
          }
          focus.onKeyDown(event);
          if (!event.defaultPrevented && event.key === 'ArrowLeft') {
            event.preventDefault();
            focusNavigation();
          }
          if (!event.defaultPrevented && event.key === 'ArrowUp') {
            event.preventDefault();
            focusPageStart();
          }
        }}
      >
        <div className="virtual-canvas" style={{ height: virtual.getTotalSize() }}>
          {items.map((row) => (
            <div
              key={row.key}
              className="channel-row"
              style={
                {
                  transform: `translateY(${row.start}px)`,
                  height: row.size,
                  '--columns': columns,
                } as CSSProperties
              }
            >
              {Array.from({ length: columns }, (_, col) => row.index * columns + col)
                .filter((index) => index < catalogue.total)
                .map((index) => {
                  const channel = catalogue.pager.at(index);
                  if (!channel)
                    return (
                      <div
                        key={`loading-${index}`}
                        className="channel-card skeleton"
                        aria-hidden="true"
                      />
                    );
                  return (
                    <button
                      key={channel.id}
                      type="button"
                      className="channel-card"
                      data-channel-index={index}
                      {...focus.bind(index)}
                      onClick={() => play(channel)}
                      aria-label={`Play ${channel.name}`}
                    >
                      <div className="channel-card-top">
                        <span className="channel-number">{String(index + 1).padStart(3, '0')}</span>
                        <span className="channel-status">
                          {channel.hasEpg ? 'GUIDE' : 'CHANNEL'}
                          {channel.favorite && <Icon name="star" size={14} />}
                        </span>
                      </div>
                      <div className="channel-monogram" aria-hidden="true">
                        {channel.name
                          .replace(/^Channel /, '')
                          .slice(0, 3)
                          .toUpperCase()}
                      </div>
                      <div className="channel-card-bottom">
                        <span className="channel-name">{channel.name}</span>
                        <Flag code={channel.country.split(/[,;\s]/)[0] ?? ''} />
                      </div>
                    </button>
                  );
                })}
            </div>
          ))}
        </div>
      </section>
      <footer className="remote-hint">
        Arrow keys to explore <span>·</span> Enter to watch <span>·</span> Menu / M for actions{' '}
        <span>·</span> Back to return
      </footer>
      {action && (
        <Modal
          title={action.name}
          onClose={() => {
            setAction(null);
            focus.focusIndex(focus.selected);
          }}
        >
          <p className="muted">
            {action.variants} stream {action.variants === 1 ? 'variant' : 'variants'} ·{' '}
            {action.hasEpg ? 'Guide matched' : 'No guide match'}
          </p>
          {actionError && <ErrorNotice message={actionError} />}
          <div className="modal-actions">
            <button
              type="button"
              data-focus
              className="button primary"
              onClick={() => play(action)}
            >
              Watch channel
            </button>
            <button
              type="button"
              data-focus
              className="button secondary"
              disabled={saving || isPreview}
              onClick={() => void favorite()}
            >
              {action.favorite ? 'Remove favorite' : 'Add to favorites'}
            </button>
          </div>
        </Modal>
      )}
    </section>
  );
}
