import { CheckCircle2, Info, X, XCircle } from "lucide-react";
import { dismissToast, holdToast, type ToastAction, useToasts } from "@/lib/app/toast";
import { cn } from "@/lib/utils";

const ICONS = { error: XCircle, success: CheckCircle2, info: Info };

const detail = (text: string) => <pre className="mt-1 max-h-40 overflow-auto font-mono text-[11.5px] leading-relaxed whitespace-pre-wrap text-muted-foreground select-text">{text}</pre>;

const buttons = (id: number, actions: ToastAction[]) =>
  actions.map((a) => (
    <button
      key={a.label}
      onClick={() => {
        dismissToast(id);
        a.run();
      }}
      className="h-fit rounded-sm px-1.5 py-0.5 text-[12px] font-medium whitespace-nowrap text-primary hover:bg-hover focus-visible:bg-hover"
    >
      {a.label}
    </button>
  ));

export function Toaster() {
  const toasts = useToasts();
  return (
    // Always mounted, so screen readers announce what's added; errors interrupt (role=alert).
    <div aria-live="polite" aria-label="Notifications" role="region" className="pointer-events-none fixed right-4 bottom-4 z-[60] flex w-96 flex-col gap-2">
      {toasts.map((t) => {
        const Icon = ICONS[t.kind];
        return (
          <div
            key={t.id}
            role={t.kind === "error" ? "alert" : "status"}
            onMouseEnter={() => holdToast(t.id, true)}
            onMouseLeave={() => holdToast(t.id, false)}
            onFocus={() => holdToast(t.id, true)}
            onBlur={(e) => !e.currentTarget.contains(e.relatedTarget) && holdToast(t.id, false)}
            className="pointer-events-auto flex gap-3 rounded-md border border-border-strong bg-elevated p-3 shadow-lg shadow-black/50 animate-in fade-in-0 slide-in-from-bottom-2"
          >
            <Icon className={cn("mt-0.5 size-4 shrink-0", t.kind === "error" ? "text-destructive" : t.kind === "success" ? "text-added" : "text-primary")} />
            <div className="min-w-0 flex-1">
              <div className="text-[12px] font-medium">{t.title}</div>
              {t.note && <div className="mt-1 text-[11.5px] leading-relaxed whitespace-pre-line text-muted-foreground select-text first-line:text-foreground">{t.note}</div>}
              {/* Beside a note there's no room for them. */}
              {t.note && t.actions.length > 0 && <div className="mt-1.5 -ml-1.5 flex flex-wrap gap-1">{buttons(t.id, t.actions)}</div>}
              {t.detail &&
                (t.note ? (
                  <details className="mt-1 text-[11px] text-subtle">
                    <summary className="cursor-default hover:text-foreground">Details</summary>
                    {detail(t.detail)}
                  </details>
                ) : (
                  detail(t.detail)
                ))}
            </div>
            {!t.note && t.actions.length > 0 && <div className="flex shrink-0 flex-col items-end gap-0.5">{buttons(t.id, t.actions)}</div>}
            <button onClick={() => dismissToast(t.id)} aria-label="Dismiss notification" className="h-fit text-subtle hover:text-foreground focus-visible:text-foreground">
              <X className="size-3.5" />
            </button>
          </div>
        );
      })}
    </div>
  );
}
