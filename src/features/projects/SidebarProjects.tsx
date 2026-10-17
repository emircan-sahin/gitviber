import { ProjectList, type ProjectListProps } from "./ProjectList";

/** The projects in the left panel (Settings → Appearance): about five rows show, the rest scroll. */
export function SidebarProjects(props: Omit<ProjectListProps, "compact">) {
  return (
    <div role="group" aria-label="Projects" className="max-h-[148px] shrink-0 overflow-x-hidden overflow-y-auto border-b border-border p-1">
      <ProjectList {...props} compact />
    </div>
  );
}
