import { IonContent, IonPage } from '@ionic/react';
import { type FormEvent, useEffect, useRef, useState } from 'react';
import { ErrorNotice } from '../components/Feedback';
import { dpadScope } from '../hooks/useDpadScope';
import { api, errorMessage } from '../lib/bridge';
import { useUiStore } from '../store/ui';
export function VaultPage() {
  const exists = useUiStore((s) => s.status?.vaultExists ?? false);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const form = useRef<HTMLFormElement>(null);
  useEffect(() => {
    form.current?.querySelector<HTMLInputElement>('input')?.focus();
    const back = () => {
      if (window.confirm('Exit IPTV?')) void api.exit();
    };
    window.addEventListener('iptv-native-back', back);
    return () => window.removeEventListener('iptv-native-back', back);
  }, []);
  async function unlock(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (busy) return;
    const data = new FormData(event.currentTarget);
    const passphrase = String(data.get('passphrase') ?? '');
    if (!exists && passphrase !== data.get('confirm')) {
      setError('The passphrases do not match.');
      return;
    }
    setBusy(true);
    setError('');
    try {
      await api.unlock(passphrase);
      form.current?.reset();
      useUiStore.getState().setStatus(await api.status());
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }
  return (
    <IonPage>
      <IonContent>
        <div className="vault-layout">
          <div className="vault-intro">
            <div className="brand">
              <img src="/icon.png" alt="" />
              IPTV
            </div>
            <span className="eyebrow">YOUR CHANNELS. YOUR GUIDE.</span>
            <h1>
              A world to watch.
              <br />A library that’s yours.
            </h1>
            <p>
              One place for your playlists and programme guide, built for the living room and every
              screen in between.
            </p>
            <small>IPTV · Local-first · No bundled channels</small>
          </div>
          <form
            ref={form}
            className="vault-form panel"
            onSubmit={(event) => void unlock(event)}
            onKeyDown={dpadScope}
          >
            <span className="eyebrow">{exists ? 'WELCOME BACK' : 'SET UP YOUR DEVICE'}</span>
            <h2>{exists ? 'Unlock your library' : 'Protect your sources'}</h2>
            <p className="muted">
              {exists
                ? 'Enter the passphrase for this device’s encrypted source vault.'
                : 'Choose a passphrase of at least 12 characters. It protects provider URLs and credentials stored on this device.'}
            </p>
            {error && <ErrorNotice message={error} />}
            <label>
              Passphrase
              <input
                data-focus
                name="passphrase"
                type="password"
                minLength={exists ? 1 : 12}
                maxLength={1024}
                required
                autoComplete={exists ? 'current-password' : 'new-password'}
                disabled={busy}
              />
            </label>
            {!exists && (
              <label>
                Confirm passphrase
                <input
                  data-focus
                  name="confirm"
                  type="password"
                  minLength={12}
                  maxLength={1024}
                  required
                  autoComplete="new-password"
                  disabled={busy}
                />
              </label>
            )}
            <button type="submit" data-focus className="button primary full-width" disabled={busy}>
              {busy ? 'Unlocking…' : exists ? 'Unlock library' : 'Create encrypted vault'}
            </button>
            <p className="field-help">
              The passphrase is not stored and cannot be recovered. Keep a safe copy. Channel names
              and programme data are not encrypted.
            </p>
          </form>
        </div>
      </IonContent>
    </IonPage>
  );
}
