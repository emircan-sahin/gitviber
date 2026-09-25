import { Tip, Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";

const VARIANTS = {
  default:
    "flex size-5 items-center justify-center rounded-sm text-muted-foreground outline-none hover:bg-active focus-visible:bg-active hover:text-foreground focus-visible:text-foreground focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-40 [&_svg]:size-3.5",
  subtle:
    "flex size-5 items-center justify-center rounded-sm text-subtle outline-none hover:bg-active hover:text-foreground focus-visible:bg-active focus-visible:text-foreground focus-visible:ring-1 focus-visible:ring-ring [&_svg]:size-3",
  // On a picker's highlighted row, which is filled with the primary color.
  picker: "flex size-5 items-center justify-center rounded-sm bg-white/15 hover:bg-white/25 focus-visible:bg-white/25 [&_svg]:size-3",
};

/**
 * An icon button at the end of a list row, labeled by its tooltip. `stopPropagation`: the row
 * drags or opens on its own, and a press on the button must do neither. `hot` (pickers): the
 * row is highlighted; the tooltip shows only then and closes as soon as the pointer leaves.
 */
export function RowAction({
  label,
  onClick,
  children,
  variant = "default",
  disabled,
  stopPropagation,
  hot,
}: {
  label: string;
  onClick: (e: React.MouseEvent<HTMLButtonElement>) => void;
  children: React.ReactNode;
  variant?: keyof typeof VARIANTS;
  disabled?: boolean;
  stopPropagation?: boolean;
  hot?: boolean;
}) {
  const button = (
    <button
      aria-label={label}
      disabled={disabled}
      onPointerDown={stopPropagation ? (e) => e.stopPropagation() : undefined}
      onClick={(e) => {
        if (stopPropagation) e.stopPropagation();
        onClick(e);
      }}
      className={VARIANTS[variant]}
    >
      {children}
    </button>
  );
  if (hot === undefined) return <Tip label={label}>{button}</Tip>;
  return (
    <Tooltip disableHoverableContent>
      <TooltipTrigger asChild>{button}</TooltipTrigger>
      {/* Arrow keys can move the highlight off a hovered button without a pointerleave. */}
      {hot && <TooltipContent>{label}</TooltipContent>}
    </Tooltip>
  );
}
