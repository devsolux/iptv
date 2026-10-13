import type { KeyboardEvent } from 'react';
import { isDirection } from '../lib/focus';

/** Linear navigation for bounded toolbars/forms. Grid/guide handlers own their own arrows. */
export function dpadScope(event: KeyboardEvent<HTMLElement>): void {
  if (
    event.defaultPrevented ||
    !isDirection(event.key) ||
    event.altKey ||
    event.ctrlKey ||
    event.metaKey
  )
    return;
  const target = event.target;
  if (!(target instanceof HTMLElement)) return;
  if (
    target instanceof HTMLInputElement &&
    !['checkbox', 'radio', 'range'].includes(target.type) &&
    (event.key === 'ArrowLeft' || event.key === 'ArrowRight')
  )
    return;
  if (target instanceof HTMLTextAreaElement) {
    if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') return;
    const before = target.value.slice(0, target.selectionStart);
    const after = target.value.slice(target.selectionEnd);
    if (event.key === 'ArrowUp' && before.includes('\n')) return;
    if (event.key === 'ArrowDown' && after.includes('\n')) return;
  }
  if (target instanceof HTMLSelectElement && (event.key === 'ArrowUp' || event.key === 'ArrowDown'))
    return;
  const controls = [...event.currentTarget.querySelectorAll<HTMLElement>('[data-focus]')].filter(
    (element) =>
      !element.hasAttribute('disabled') &&
      !element.closest('[hidden]') &&
      element.getClientRects().length > 0,
  );
  const index = controls.findIndex((element) => element === target || element.contains(target));
  const increment = event.key === 'ArrowUp' || event.key === 'ArrowLeft' ? -1 : 1;
  const next = controls[index + increment];
  if (next) {
    event.preventDefault();
    event.stopPropagation();
    next.focus();
  }
}
