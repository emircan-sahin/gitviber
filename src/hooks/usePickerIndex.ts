import { useState } from "react";

/**
 * The highlighted row of a picker's `count`, -1 for none (the pointer left the list). `move`
 * steps it by rows: clamped at the ends, or with `wrap` a single step past one goes round to the
 * other (a page still stops there).
 */
export function usePickerIndex(count: number, { wrap = false }: { wrap?: boolean } = {}) {
  const [index, setIndex] = useState(0);
  const move = (step: number) =>
    setIndex((i) => {
      const to = i + step;
      if (wrap && Math.abs(step) === 1 && (to < 0 || to >= count)) return step > 0 ? 0 : count - 1;
      return step > 0 ? Math.min(count - 1, to) : Math.max(0, to);
    });
  return { index, setIndex, move };
}
