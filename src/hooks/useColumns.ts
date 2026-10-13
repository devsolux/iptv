import { type RefObject, useLayoutEffect, useState } from 'react';
export function useColumns(element: RefObject<HTMLElement | null>, minimum = 230) {
  const [columns, setColumns] = useState(3);
  useLayoutEffect(() => {
    const target = element.current;
    if (!target) return;
    const observer = new ResizeObserver(([entry]) => {
      if (entry)
        setColumns(Math.max(1, Math.min(6, Math.floor(entry.contentRect.width / minimum))));
    });
    observer.observe(target);
    return () => observer.disconnect();
  }, [element, minimum]);
  return columns;
}
