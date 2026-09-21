import { closestCenter, DndContext, type DragEndEvent, PointerSensor, useSensor, useSensors } from "@dnd-kit/core";
import { restrictToHorizontalAxis, restrictToParentElement, restrictToVerticalAxis } from "@dnd-kit/modifiers";
import { horizontalListSortingStrategy, SortableContext, useSortable, verticalListSortingStrategy } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { createContext, useContext, useRef } from "react";

// The pointerup that ends a drag also clicks the item under it; items ask this before acting.
const DragGuard = createContext<() => boolean>(() => false);

/**
 * Animated drag-to-reorder list (dnd-kit). Neighbours slide out of the way while dragging,
 * movement is locked to one axis, and a press only becomes a drag after 5px so clicks
 * still work.
 */
export function SortableList({
  ids,
  axis,
  onMove,
  children,
}: {
  ids: string[];
  axis: "x" | "y";
  onMove: (from: number, to: number) => void;
  children: React.ReactNode;
}) {
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 5 } }));
  const endedAt = useRef(0);
  const onDragEnd = ({ active, over }: DragEndEvent) => {
    endedAt.current = performance.now();
    if (over && active.id !== over.id) onMove(ids.indexOf(String(active.id)), ids.indexOf(String(over.id)));
  };
  return (
    <DragGuard.Provider value={() => performance.now() - endedAt.current < 250}>
      <DndContext
        sensors={sensors}
        collisionDetection={closestCenter}
        modifiers={[axis === "x" ? restrictToHorizontalAxis : restrictToVerticalAxis, restrictToParentElement]}
        onDragEnd={onDragEnd}
        onDragCancel={() => (endedAt.current = performance.now())}
      >
        <SortableContext items={ids} strategy={axis === "x" ? horizontalListSortingStrategy : verticalListSortingStrategy}>
          {children}
        </SortableContext>
      </DndContext>
    </DragGuard.Provider>
  );
}

/** Props for one sortable item: ref, style (animated transform) and drag listeners. */
export function useSortableItem(id: string) {
  const { setNodeRef, transform, transition, isDragging, listeners } = useSortable({ id });
  const justDragged = useContext(DragGuard);
  return {
    dragging: isDragging,
    /** Wrap click handlers: ignores the click that ends a drag. */
    guard:
      <A extends unknown[]>(fn: (...a: A) => void) =>
      (...a: A) => {
        if (!justDragged()) fn(...a);
      },
    props: {
      ref: setNodeRef,
      ...listeners,
      style: {
        transform: CSS.Translate.toString(transform),
        transition,
        position: "relative" as const,
        zIndex: isDragging ? 20 : undefined,
      },
    },
  };
}
