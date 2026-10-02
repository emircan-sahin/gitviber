import { DndContext, DragOverlay } from "@dnd-kit/core";
import { useEffect, useRef, useState } from "react";
import { useDragGuard, useDragSensors } from "@/components/Sortable";
import type { DropAt } from "./edits";

/**
 * Dragging commits in History, as GitHub Desktop does: onto a commit squashes them into it,
 * between two moves them there. The rows stay put, as the graph's lines run through them; a
 * line or a ring marks the spot. `drag(sha)`: the commits a drag from that row takes.
 */
export function CommitDrag({
  drag,
  canDrop,
  onDrop,
  children,
}: {
  drag: (sha: string) => string[];
  canDrop: (sha: string) => boolean;
  onDrop: (shas: string[], at: DropAt) => void;
  children: (state: { dragged: Set<string>; at: DropAt | null; justDragged: () => boolean }) => React.ReactNode;
}) {
  const sensors = useDragSensors();
  const { ended, justDragged } = useDragGuard();
  const [dragged, setDragged] = useState<string[] | null>(null);
  const [at, setAt] = useState<DropAt | null>(null);
  const target = useRef<DropAt | null>(null);
  const allowed = useRef(canDrop);
  allowed.current = canDrop;

  // Read from the pointer, not dnd-kit's rects: auto-scroll moves rows under a still pointer.
  useEffect(() => {
    if (!dragged) return;
    const skip = new Set(dragged);
    let x = -1;
    let y = -1;
    const update = () => {
      target.current = x < 0 ? null : dropAt(x, y, skip, allowed.current);
      setAt((a) => (a?.sha === target.current?.sha && a?.where === target.current?.where ? a : target.current));
    };
    const move = (e: PointerEvent) => {
      x = e.clientX;
      y = e.clientY;
      update();
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("scroll", update, true);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("scroll", update, true);
    };
  }, [dragged]);

  const end = (drop: boolean) => {
    ended();
    if (drop && dragged && target.current) onDrop(dragged, target.current);
    target.current = null;
    setDragged(null);
    setAt(null);
  };

  return (
    <DndContext sensors={sensors} onDragStart={({ active }) => setDragged(drag(String(active.id)))} onDragEnd={() => end(true)} onDragCancel={() => end(false)}>
      {children({ dragged: new Set(dragged), at, justDragged })}
      <DragOverlay dropAnimation={null} className="pointer-events-none">
        {dragged && (
          <div className="w-fit rounded-md border border-border bg-popover px-2.5 py-1 text-[12px] text-popover-foreground shadow-md">
            {at?.where === "onto" ? "Squash" : "Move"} {dragged.length === 1 ? "1 commit" : `${dragged.length} commits`}
          </div>
        )}
      </DragOverlay>
    </DndContext>
  );
}

/** The commit row under the pointer, by quarters: the top and bottom ones are the gaps around it. */
function dropAt(x: number, y: number, dragged: Set<string>, canDrop: (sha: string) => boolean): DropAt | null {
  const row = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-commit]");
  const sha = row?.dataset.commit;
  if (!row || !sha || dragged.has(sha) || !canDrop(sha)) return null;
  const r = row.getBoundingClientRect();
  return { sha, where: y < r.top + r.height / 4 ? "above" : y > r.bottom - r.height / 4 ? "below" : "onto" };
}
