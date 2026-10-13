import { type FormEvent, useEffect, useRef, useState } from 'react';
import { ErrorNotice } from '../components/Feedback';
import { Icon } from '../components/Icon';
import { Modal } from '../components/Modal';
import { dpadScope } from '../hooks/useDpadScope';
import { api, BridgeError, errorMessage, isPreview } from '../lib/bridge';
import type { Job, Source, SourceInput } from '../lib/contracts';
import { useUiStore } from '../store/ui';

const lines = (value: string) =>
  value
    .split(/[\n,]/)
    .map((line) => line.trim())
    .filter(Boolean);
export default function SourcesPage() {
  const [sources, setSources] = useState<Source[]>([]);
  const [jobs, setJobs] = useState<Job[]>([]);
  const [error, setError] = useState('');
  const [saving, setSaving] = useState(false);
  const [remove, setRemove] = useState<Source | null>(null);
  const [refresh, setRefresh] = useState(0);
  const form = useRef<HTMLFormElement>(null);
  useEffect(() => {
    let active = true;
    let pending = false;
    async function load() {
      if (pending || document.visibilityState === 'hidden') return;
      pending = true;
      try {
        const [sources, jobs] = await Promise.all([api.sources(), api.jobs()]);
        if (active) {
          setSources(sources);
          setJobs(jobs);
        }
      } catch (error) {
        if (active) setError(errorMessage(error));
      } finally {
        pending = false;
      }
    }
    void load();
    const timer = window.setInterval(() => void load(), 1500);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [refresh]);
  async function add(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (saving || isPreview) return;
    const data = new FormData(event.currentTarget);
    const input: SourceInput = {
      name: String(data.get('name') ?? '').trim(),
      playlistUrl: String(data.get('playlist') ?? '').trim(),
      epgUrls: lines(String(data.get('epg') ?? '')),
      cdnHosts: lines(String(data.get('cdn') ?? '')).map((host) => host.toLowerCase()),
      allowHttp: data.get('http') === 'on',
      allowLan: data.get('lan') === 'on',
      countryCode: String(data.get('country') ?? '')
        .trim()
        .toUpperCase(),
    };
    setSaving(true);
    setError('');
    try {
      const id = await api.addSource(input);
      form.current?.reset(); // Do not leave provider credentials in form fields after saving.
      try {
        await api.refreshSource(id);
      } catch (error) {
        if (!(error instanceof BridgeError && error.code === 'BUSY')) throw error;
      }
      setRefresh((n) => n + 1);
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setSaving(false);
    }
  }
  async function run(source: Source) {
    setError('');
    try {
      await api.refreshSource(source.id);
      setRefresh((n) => n + 1);
    } catch (error) {
      setError(errorMessage(error));
    }
  }
  async function cancel(job: Job) {
    try {
      await api.cancelJob(job.id);
      setRefresh((n) => n + 1);
    } catch (error) {
      setError(errorMessage(error));
    }
  }
  async function deleteSource() {
    if (!remove) return;
    setSaving(true);
    try {
      await api.removeSource(remove.id);
      setRemove(null);
      useUiStore.getState().setSource(null);
      useUiStore.getState().invalidate();
      setRefresh((n) => n + 1);
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setSaving(false);
    }
  }
  return (
    <section className="page sources-page scroll-page" onKeyDown={dpadScope}>
      <header className="page-heading">
        <div>
          <span className="eyebrow">MAKE IT YOURS</span>
          <h1>Sources</h1>
          <p className="muted">Bring your own playlists. Keep control of your library.</p>
        </div>
      </header>
      {error && (
        <ErrorNotice
          message={error}
          onRetry={() => {
            setError('');
            setRefresh((n) => n + 1);
          }}
        />
      )}
      <div className="sources-layout">
        <div className="source-list">
          {sources.length === 0 && (
            <div className="info-card">
              <h2>A fresh start.</h2>
              <p>
                Add a provider’s M3U URL and, optionally, its XMLTV guide. No channels or accounts
                are bundled with IPTV.
              </p>
            </div>
          )}
          {sources.map((source) => {
            const job = jobs.find((job) => job.sourceId === source.id);
            const busy = job && (job.state === 'queued' || job.state === 'running');
            return (
              <article className="source-card" key={source.id}>
                <div className="source-card-heading">
                  <span className="source-symbol">
                    <Icon name="sources" size={26} />
                  </span>
                  <div>
                    <h2>{source.name}</h2>
                    <p>
                      {source.channels.toLocaleString()} channels ·{' '}
                      {source.hasEpg ? 'Guide configured' : 'No guide configured'}
                    </p>
                  </div>
                </div>
                <p className="source-updated">
                  {source.updatedAt
                    ? `Updated ${new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' }).format(source.updatedAt * 1000)}`
                    : 'No successful import yet'}
                </p>
                {busy && (
                  <div className="job-progress" role="status">
                    <progress aria-label="Import in progress" />
                    <span>
                      {job.phase} · {job.processed.toLocaleString()} records
                    </span>
                  </div>
                )}
                {job?.state === 'failed' && (
                  <p className="inline-error" role="alert">
                    {job.error} The previous catalogue is unchanged.
                  </p>
                )}
                {job?.state === 'cancelled' && (
                  <p className="muted">Import cancelled. The previous catalogue is unchanged.</p>
                )}
                <div className="source-actions">
                  {busy ? (
                    <button
                      type="button"
                      data-focus
                      className="button secondary"
                      onClick={() => void cancel(job)}
                    >
                      Cancel import
                    </button>
                  ) : (
                    <button
                      type="button"
                      data-focus
                      className="button secondary"
                      disabled={isPreview}
                      onClick={() => void run(source)}
                    >
                      <Icon name="refresh" size={16} />
                      Refresh
                    </button>
                  )}
                  <button
                    type="button"
                    data-focus
                    className="button quiet"
                    disabled={Boolean(busy) || isPreview}
                    onClick={() => setRemove(source)}
                  >
                    Remove
                  </button>
                </div>
              </article>
            );
          })}
          <div className="source-notes">
            <h3>Built for a reliable library</h3>
            <p>
              Imports run outside the interface. Playlist and guide changes become visible together
              only after a successful import. Sources refresh approximately every six hours while
              IPTV is running and unlocked.
            </p>
            <p>
              Guide feeds are processed in the order entered. Exact TVG IDs take priority; ambiguous
              name matches remain unassigned.
            </p>
          </div>
        </div>
        <form
          ref={form}
          className="source-form panel"
          onSubmit={(event) => void add(event)}
          autoComplete="off"
        >
          <span className="eyebrow">ADD TO YOUR LIBRARY</span>
          <h2>New source</h2>
          <label>
            Source name
            <input
              id="page-focus-start"
              data-focus
              name="name"
              required
              maxLength={128}
              placeholder="My living room playlist"
              disabled={saving || isPreview}
            />
          </label>
          <label>
            M3U playlist URL
            <input
              data-focus
              name="playlist"
              type="url"
              required
              maxLength={16384}
              placeholder="https://provider.example/playlist.m3u"
              autoCapitalize="none"
              spellCheck={false}
              disabled={saving || isPreview}
            />
          </label>
          <label>
            XMLTV guide URLs <span className="optional">Optional · one per line, up to four</span>
            <textarea
              data-focus
              name="epg"
              rows={3}
              maxLength={65536}
              placeholder="https://provider.example/guide.xml.gz"
              autoCapitalize="none"
              spellCheck={false}
              disabled={saving || isPreview}
            />
          </label>
          <label>
            Default country{' '}
            <span className="optional">Optional · two letters, such as DE or TR</span>
            <input
              data-focus
              name="country"
              maxLength={2}
              pattern="[A-Za-z]{2}"
              placeholder="DE"
              autoCapitalize="characters"
              disabled={saving || isPreview}
            />
          </label>
          <label>
            Approved CDN hosts{' '}
            <span className="optional">Optional · exact hostnames, one per line</span>
            <textarea
              data-focus
              name="cdn"
              rows={2}
              maxLength={8192}
              placeholder="cdn.provider.example"
              autoCapitalize="none"
              spellCheck={false}
              disabled={saving || isPreview}
            />
          </label>
          <label className="checkbox-label">
            <input data-focus name="http" type="checkbox" disabled={saving || isPreview} />
            <span>Allow unencrypted HTTP for this source</span>
          </label>
          <label className="checkbox-label">
            <input data-focus name="lan" type="checkbox" disabled={saving || isPreview} />
            <span>Allow private LAN addresses for this source</span>
          </label>
          <p className="field-help">
            Enable these only for providers and home-network devices you trust. Loopback, link-local
            and metadata addresses are always blocked.
          </p>
          <button
            type="submit"
            data-focus
            className="button primary full-width"
            disabled={saving || isPreview}
          >
            {saving ? 'Saving source…' : 'Add source and import'}
          </button>
          <p className="field-help">
            Provider URLs and headers are encrypted locally. Only add sources you are authorized to
            access.
          </p>
        </form>
      </div>
      {remove && (
        <Modal title={`Remove ${remove.name}?`} onClose={() => setRemove(null)}>
          <p>
            This removes its local channels, guide and associated favorites. It does not change the
            provider’s playlist.
          </p>
          <div className="modal-actions">
            <button
              type="button"
              data-focus
              className="button secondary"
              disabled={saving}
              onClick={() => setRemove(null)}
            >
              Keep source
            </button>
            <button
              type="button"
              data-focus
              className="button danger"
              disabled={saving}
              onClick={() => void deleteSource()}
            >
              Remove source
            </button>
          </div>
        </Modal>
      )}
    </section>
  );
}
