import { Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Tip } from "@/components/ui/tooltip";

/** A pane header's "new" action. */
export function NewButton({ label, disabled, onClick }: { label: string; disabled?: boolean; onClick: () => void }) {
  return (
    // A disabled button gets no hover, so the wrapper carries the tooltip that explains it.
    <Tip label={label}>
      <span>
        <Button variant="ghost" size="icon-sm" aria-label={label} disabled={disabled} onClick={onClick}>
          <Plus />
        </Button>
      </span>
    </Tip>
  );
}
