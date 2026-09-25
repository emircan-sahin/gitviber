export function Section({ title, aside, children }: { title: string; aside?: string; children: React.ReactNode }) {
  return (
    <div className="mt-5 overflow-hidden rounded-md border border-border">
      <div className="flex h-8 items-center border-b border-border bg-panel px-3 text-[10.5px] font-semibold tracking-[0.08em] text-subtle uppercase">
        {title}
        {aside && <span className="ml-auto font-mono tracking-normal normal-case">{aside}</span>}
      </div>
      <div>{children}</div>
    </div>
  );
}
