import type { Pull, PullDetail } from "@/lib/api";
import { checksSummary } from "@/lib/github/checks";
import { CheckRows } from "@/features/github/shared/Checks";
import { Section } from "@/features/github/shared/Section";

export function PullChecks({ pull, detail: d }: { pull: Pull; detail: PullDetail }) {
  if (!d.checks.length && !d.checksError) return null;
  return (
    <Section title="Checks" aside={d.checks.length > 0 ? checksSummary(d.checks) : undefined}>
      {d.checksError && <div className="px-3 py-2 text-[12px] text-removed">Could not load all checks: {d.checksError}</div>}
      <CheckRows checks={d.checks} home={pull} where={pull.url} />
    </Section>
  );
}
