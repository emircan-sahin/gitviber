import { closestCenter, DndContext, type DragEndEvent, PointerSensor, type PointerSensorProps, useSensor, useSensors } from "@dnd-kit/core";
import { restrictToHorizontalAxis, restrictToParentElement, restrictToVerticalAxis } from "@dnd-kit/modifiers";
import { horizontalListSortingStrategy, SortableContext, useSortable, verticalListSortingStrategy } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { createContext, useContext, useRef } from "react";
import { buttonLost } from "@/lib/ui/held";

// The pointerup that ends a drag also clicks the item under it; items ask this before acting.
const DragGuard = createContext<() => boolean>(() => false);

/**
 * dnd-kit's pointer sensor, but a drag whose pointerup the window never saw (Mission Control, the
 * screen locking) ends at the next move without a button. It followed the pointer until the next
 * click, which dropped it: a commit moved or squashed by a plain click.
 */
class Sensor extends PointerSensor {
  constructor(props: PointerSensorProps) {
    // pointercancel on the document is the sensor's own way out: it detaches and cancels the drag.
    const move = (e: PointerEvent) => buttonLost(e) && document.dispatchEvent(new PointerEvent("pointercancel"));
    const stop = () => document.removeEventListener("pointermove", move, true);
    super({
      ...props,
      onEnd: () => {
        stop();
        props.onEnd();
      },
      onCancel: () => {
        stop();
        props.onCancel();
      },
    });
    document.addEventListener("pointermove", move, true);
  }
}

/** A press only becomes a drag after 5px, so clicks still work. */
export const useDragSensors = () => useSensors(useSensor(Sensor, { activationConstraint: { distance: 5 } }));

/** Call `ended` when a drag ends; `justDragged` then tells the click that came with it apart. */
export function useDragGuard() {
  const endedAt = useRef(0);
  return { ended: () => void (endedAt.current = performance.now()), justDragged: () => performance.now() - endedAt.current < 250 };
}

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
  const sensors = useDragSensors();
  const { ended, justDragged } = useDragGuard();
  const onDragEnd = ({ active, over }: DragEndEvent) => {
    ended();
    if (over && active.id !== over.id) onMove(ids.indexOf(String(active.id)), ids.indexOf(String(over.id)));
  };
  return (
    <DragGuard.Provider value={justDragged}>
      <DndContext
        sensors={sensors}
        collisionDetection={closestCenter}
        modifiers={[axis === "x" ? restrictToHorizontalAxis : restrictToVerticalAxis, restrictToParentElement]}
        onDragEnd={onDragEnd}
        onDragCancel={ended}
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
