import { useVirtualizer } from '@tanstack/react-virtual';
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useLocation, useNavigate } from 'react-router';
import { ChannelArt } from '../components/ChannelArt';
import { Empty, ErrorNotice, Loading } from '../components/Feedback';
import { Filters } from '../components/Filters';
import { focusNavigation, focusPageStart } from '../components/Shell';
import { useCatalogue } from '../hooks/useCatalogue';
import { dpadScope } from '../hooks/useDpadScope';
import { useFilters } from '../hooks/useFilters';
import { api, BridgeError, errorMessage } from '../lib/bridge';
import type { Channel, Programme } from '../lib/contracts';
import { nearestAtTime, timelineSpans } from '../lib/focus';
import { useUiStore } from '../store/ui';

const WINDOW = 3 * 3600;
const clock = (value: number) =>
  new Intl.DateTimeFormat(undefined, { hour: '2-digit', minute: '2-digit' }).format(value * 1000);
const initialStart = () => Math.floor(Date.now() / 1000 / 1800) * 1800;
interface Selection {
  index: number;
  time: number;
  header: boolean;
}
export default function GuidePage() {
  const filters = useFilters();
  const catalogue = useCatalogue(filters);
  const viewport = useRef<HTMLElement>(null);
  const navigate = useNavigate();
  const location = useLocation();
  const [start, setStart] = useState(initialStart);
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000));
  const [programmes, setProgrammes] = useState<Programme[]>([]);
  const [error, setError] = useState('');
  const [guideLoading, setGuideLoading] = useState(false);
  const [selection, setSelection] = useState<Selection>({ index: 0, time: now, header: false });
  const selected = useRef(selection);
  selected.current = selection;
  const pending = useRef(false);
  const buttons = useRef(new Map<string, HTMLButtonElement>());
  const virtual = useVirtualizer({
    count: catalogue.total,
    getScrollElement: () => viewport.current,
    estimateSize: () => 102,
    overscan: 2,
  });
  const rows = virtual.getVirtualItems();
  const first = rows[0]?.index ?? 0;
  const last = rows.at(-1)?.index ?? 0;
  const visible = rows
    .map((row) => catalogue.pager.at(row.index))
    .filter((channel): channel is Channel => Boolean(channel))
    .slice(0, 40);
  const ids = visible.map((channel) => channel.id).join(',');
  useEffect(() => {
    if (catalogue.total) catalogue.pager.wantRange(first, last);
  }, [catalogue.pager, catalogue.total, first, last]);
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Math.floor(Date.now() / 1000)), 30_000);
    return () => window.clearInterval(timer);
  }, []);
  useEffect(() => {
    if (!ids || catalogue.revision === null) {
      setProgrammes([]);
      return;
    }
    let active = true;
    setGuideLoading(true);
    const query = {
      channelIds: ids.split(','),
      start,
      end: start + WINDOW,
      revision: catalogue.revision,
    };
    const timer = window.setTimeout(() => {
      void api
        .guide(query)
        .then((result) => {
          if (active) {
            setProgrammes(result.items);
            setError('');
          }
        })
        .catch((error: unknown) => {
          if (!active) return;
          if (error instanceof BridgeError && error.code === 'STALE')
            useUiStore.getState().invalidate();
          else setError(errorMessage(error));
        })
        .finally(() => {
          if (active) setGuideLoading(false);
        });
    }, 80);
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, [ids, start, catalogue.revision]);
  const schedules = useMemo(() => {
    const grouped = new Map<string, Programme[]>();
    for (const programme of programmes) {
      const list = grouped.get(programme.channelId) ?? [];
      list.push(programme);
      grouped.set(programme.channelId, list);
    }
    return new Map(
      [...grouped].map(([id, items]) => [id, timelineSpans(items, start, start + WINDOW)]),
    );
  }, [programmes, start]);
  const requestFocus = (next: Selection) => {
    if (!catalogue.total) return;
    const clamped = { ...next, index: Math.min(catalogue.total - 1, Math.max(0, next.index)) };
    selected.current = clamped;
    pending.current = true;
    setSelection(clamped);
    catalogue.pager.wantRange(clamped.index, clamped.index + 4);
    virtual.scrollToIndex(clamped.index, { align: 'auto' });
  };
  // biome-ignore lint/correctness/useExhaustiveDependencies: loaded pages and scrolled rows may mount the button to focus.
  useLayoutEffect(() => {
    if (!pending.current || guideLoading) return;
    const channel = catalogue.pager.at(selection.index);
    if (!channel) return;
    const programme = nearestAtTime(
      schedules.get(channel.id)?.map((span) => span.item) ?? [],
      selection.time,
    );
    const suffix = selection.header ? 'header' : programme ? String(programme.start) : 'empty';
    const button = buttons.current.get(`${channel.id}:${suffix}`);
    if (button) {
      button.focus({ preventScroll: true });
      pending.current = false;
    }
  }, [selection, schedules, guideLoading, catalogue.pager, catalogue.pages, first, last]);
  const bind = (channel: Channel, index: number, suffix: string, time: number, header = false) => ({
    ref: (element: HTMLButtonElement | null) => {
      const key = `${channel.id}:${suffix}`;
      if (element) buttons.current.set(key, element);
      else buttons.current.delete(key);
    },
    onFocus: () => {
      const value = { index, time: pending.current ? selected.current.time : time, header };
      selected.current = value;
      setSelection(value);
    },
  });
  function play(channel: Channel) {
    navigate(`/play/${channel.id}`, { state: { returnTo: location.pathname } });
  }
  function moveWindow(direction: number) {
    const next = start + direction * WINDOW;
    if (next < now - 86400 || next > now + 14 * 86400) return;
    setStart(next);
    setProgrammes([]);
    requestFocus({
      ...selected.current,
      time: direction > 0 ? next : next + WINDOW - 1,
      header: false,
    });
  }
  const activeChannel = catalogue.pager.at(selection.index);
  const activeProgramme = activeChannel
    ? nearestAtTime(schedules.get(activeChannel.id)?.map((span) => span.item) ?? [], selection.time)
    : undefined;
  return (
    <section className="page guide-page" data-cached-pages={catalogue.pages.size}>
      <header className="page-heading">
        <div>
          <span className="eyebrow">PLAN YOUR VIEWING</span>
          <h1>TV guide</h1>
          <p className="muted">
            {new Intl.DateTimeFormat(undefined, {
              weekday: 'long',
              day: 'numeric',
              month: 'long',
            }).format(start * 1000)}{' '}
            · Times shown in your device timezone
          </p>
        </div>
        <div
          className="heading-actions"
          id="page-actions"
          role="toolbar"
          aria-label="Guide time window"
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
            className="button secondary"
            aria-label="Previous three hours"
            onClick={() => moveWindow(-1)}
          >
            ←
          </button>
          <button
            type="button"
            data-focus
            className="button secondary"
            onClick={() => {
              const next = initialStart();
              setStart(next);
              requestFocus({ ...selection, time: now, header: false });
            }}
          >
            Now
          </button>
          <button
            type="button"
            data-focus
            className="button secondary"
            aria-label="Next three hours"
            onClick={() => moveWindow(1)}
          >
            →
          </button>
        </div>
      </header>
      <Filters onEnterContent={() => requestFocus({ ...selection, index: 0, header: false })} />
      {error && <ErrorNotice message={error} />}
      {catalogue.error ? (
        <ErrorNotice
          message={errorMessage(catalogue.error)}
          onRetry={() => catalogue.pager.reset(filters)}
        />
      ) : null}
      {!catalogue.ready && <Loading label="Opening the guide…" />}
      {catalogue.ready && !catalogue.total && (
        <Empty title="Your guide is waiting.">
          <p>Add a playlist and an XMLTV feed in Sources, or clear your channel filters.</p>
        </Empty>
      )}
      <div className="guide-axis">
        <div>CHANNEL</div>
        <div className="guide-times">
          {Array.from({ length: 6 }, (_, index) => start + index * 1800).map((time) => (
            <span key={time}>{clock(time)}</span>
          ))}
        </div>
      </div>
      <section
        ref={viewport}
        className="virtual-viewport guide-viewport"
        // biome-ignore lint/a11y/noNoninteractiveTabindex: a scrollable region must stay keyboard-focusable and is the focus fallback for virtualized rows.
        tabIndex={0}
        aria-label="TV guide"
        onKeyDown={(event) => {
          const current = selected.current;
          if (event.key === 'ArrowUp') {
            event.preventDefault();
            if (!current.index) focusPageStart();
            else requestFocus({ ...current, index: current.index - 1 });
          } else if (event.key === 'ArrowDown') {
            event.preventDefault();
            requestFocus({ ...current, index: current.index + 1 });
          } else if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
            event.preventDefault();
            const forward = event.key === 'ArrowRight';
            if (current.header && !forward) {
              focusNavigation();
              return;
            }
            const channel = catalogue.pager.at(current.index);
            const list = channel ? (schedules.get(channel.id)?.map((span) => span.item) ?? []) : [];
            if (current.header) {
              requestFocus({ ...current, header: false, time: list[0]?.start ?? start });
              return;
            }
            const active = nearestAtTime(list, current.time);
            const index = active ? list.indexOf(active) : -1;
            const next = list[index + (forward ? 1 : -1)];
            if (next) requestFocus({ ...current, time: Math.max(start, next.start) + 1 });
            else if (forward) moveWindow(1);
            else requestFocus({ ...current, header: true });
          } else if (event.key === 'Enter' && event.target === event.currentTarget && activeChannel)
            play(activeChannel);
        }}
      >
        <div className="virtual-canvas" style={{ height: virtual.getTotalSize() }}>
          {rows.map((row) => {
            const channel = catalogue.pager.at(row.index);
            if (!channel)
              return (
                <div
                  key={row.key}
                  className="guide-row skeleton"
                  style={{ height: row.size, transform: `translateY(${row.start}px)` }}
                />
              );
            const spans = schedules.get(channel.id) ?? [];
            const chosen = nearestAtTime(
              spans.map((span) => span.item),
              selection.time,
            );
            return (
              <div
                className="guide-row"
                key={row.key}
                style={{ height: row.size, transform: `translateY(${row.start}px)` }}
              >
                <button
                  type="button"
                  className="guide-channel"
                  tabIndex={selection.index === row.index && selection.header ? 0 : -1}
                  {...bind(channel, row.index, 'header', selection.time, true)}
                  onClick={() => play(channel)}
                >
                  <span className="channel-number">{String(row.index + 1).padStart(3, '0')}</span>
                  <span className="guide-identity">
                    <ChannelArt name={channel.name} logoUrl={channel.logoUrl} compact />
                    <strong>{channel.name}</strong>
                  </span>
                </button>
                <div className="programme-track">
                  {!spans.length && (
                    <button
                      type="button"
                      className="programme empty-programme"
                      tabIndex={selection.index === row.index && !selection.header ? 0 : -1}
                      {...bind(channel, row.index, 'empty', selection.time)}
                      onClick={() => play(channel)}
                    >
                      {guideLoading ? 'Loading guide…' : 'No guide data · Watch live'}
                    </button>
                  )}
                  {spans.map(({ item, left, width }) => (
                    <button
                      key={`${item.start}:${item.end}:${item.title}`}
                      type="button"
                      className="programme"
                      style={{ left: `${left}%`, width: `${width}%` }}
                      tabIndex={
                        selection.index === row.index && !selection.header && chosen === item
                          ? 0
                          : -1
                      }
                      {...bind(
                        channel,
                        row.index,
                        String(item.start),
                        Math.max(start, item.start) + 1,
                      )}
                      onClick={() => play(channel)}
                      aria-label={`${item.title}, ${clock(item.start)}. Watch ${channel.name} live.`}
                    >
                      <small>
                        {clock(item.start)} — {clock(item.end)}
                      </small>
                      <strong>{item.title || 'Untitled programme'}</strong>
                    </button>
                  ))}
                  {now >= start && now < start + WINDOW && (
                    <div
                      className="now-line"
                      style={{ left: `${((now - start) / WINDOW) * 100}%` }}
                      aria-hidden="true"
                    />
                  )}
                </div>
              </div>
            );
          })}
        </div>
      </section>
      <aside className="programme-detail" aria-live="polite">
        <strong>{activeProgramme?.title || activeChannel?.name || 'Explore your TV guide'}</strong>
        <p>
          {activeProgramme?.description ||
            'Up and down keep your selected time. Left and right explore programmes. Enter watches the channel live.'}
        </p>
      </aside>
    </section>
  );
}
