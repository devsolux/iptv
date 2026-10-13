import { useCallback, useLayoutEffect, useRef, useState, type KeyboardEvent, type RefObject } from 'react';
import { gridNeighbour, isDirection } from '../lib/focus';

interface Options {
  container: RefObject<HTMLDivElement | null>; count: number; columns: number; initialIndex?: number;
  scrollTo: (index: number) => void; request: (index: number) => void;
  onFocus?: (index: number) => void; updateToken: unknown;
}
export function useDpadFocus(options: Options) {
  const active = useRef(options.initialIndex ?? 0);
  const pending = useRef(false);
  const buttons = useRef(new Map<number, HTMLButtonElement>());
  const bindings = useRef(new Map<number, { ref: (element: HTMLButtonElement | null) => void; onFocus: () => void }>());
  const [selected, setSelected] = useState(active.current);
  const latest = useRef(options); latest.current = options;
  const focusIndex = useCallback((index: number) => {
    const current = latest.current;
    if (!current.count) return;
    const next = Math.max(0, Math.min(index, current.count - 1));
    active.current = next; pending.current = true; setSelected(next);
    current.request(next); current.scrollTo(next);
    const button = buttons.current.get(next);
    if (button) { button.focus({ preventScroll: true }); pending.current = false; }
  }, []);
  useLayoutEffect(() => {
    if (selected >= options.count && options.count > 0) { active.current = options.count - 1; setSelected(active.current); }
    if (pending.current) {
      const button = buttons.current.get(active.current);
      if (button) { button.focus({ preventScroll: true }); pending.current = false; }
    }
  }, [options.updateToken, options.count, selected]);
  const onKeyDown = useCallback((event: KeyboardEvent<HTMLDivElement>) => {
    if (!isDirection(event.key)) return;
    const current = latest.current;
    const next = gridNeighbour(active.current, current.count, current.columns, event.key);
    if (next !== null) { event.preventDefault(); event.stopPropagation(); focusIndex(next); }
  }, [focusIndex]);
  const bind = (index: number) => {
    let handlers = bindings.current.get(index);
    if (!handlers) {
      handlers = {
        ref: (element: HTMLButtonElement | null) => {
          if (element) {
            buttons.current.set(index, element);
            if (pending.current && active.current === index) {
              requestAnimationFrame(() => {
                if (buttons.current.get(index) === element && pending.current && active.current === index) {
                  element.focus({ preventScroll: true }); pending.current = false;
                }
              });
            }
          } else {
            const old = buttons.current.get(index);
            if (old && document.activeElement === old) latest.current.container.current?.focus({ preventScroll: true });
            buttons.current.delete(index); bindings.current.delete(index);
          }
        },
        onFocus: () => { active.current = index; setSelected(index); latest.current.onFocus?.(index); },
      };
      bindings.current.set(index, handlers);
    }
    return { tabIndex: selected === index ? 0 : -1, ...handlers };
  };
  return { selected, bind, onKeyDown, focusIndex };
}
