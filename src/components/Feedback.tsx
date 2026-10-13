import { IonSpinner } from '@ionic/react';
import type { ReactNode } from 'react';
export function Loading({ label = 'Loading your library…' }: { label?: string }) {
  return (
    <div className="feedback" role="status">
      <IonSpinner name="crescent" />
      <p>{label}</p>
    </div>
  );
}
export function Empty({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="empty-state">
      <div className="empty-mark" aria-hidden="true">
        TV
      </div>
      <h2>{title}</h2>
      <div className="muted">{children}</div>
    </div>
  );
}
export function ErrorNotice({ message, onRetry }: { message: string; onRetry?: () => void }) {
  return (
    <div className="error-notice" role="alert">
      <p>{message}</p>
      {onRetry && (
        <button type="button" data-focus className="button secondary" onClick={onRetry}>
          Try again
        </button>
      )}
    </div>
  );
}
