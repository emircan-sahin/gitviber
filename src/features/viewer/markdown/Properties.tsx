import { type ReactNode, use, useEffect, useState } from "react";
import { parseWikilink, wikilinkLabel } from "@/lib/markdown/syntax";
import { MarkdownHostContext } from "./host";

type Yaml = typeof import("yaml");
let yaml: Yaml | null = null;
let loading: Promise<Yaml> | null = null;
const load = () => (loading ??= import("yaml").then((m) => (yaml = m)));

function parse(y: Yaml, source: string): Record<string, unknown> | string {
  try {
    const data: unknown = y.parse(source);
    return data && typeof data === "object" && !Array.isArray(data) ? (data as Record<string, unknown>) : source;
  } catch (e) {
    return e instanceof Error ? e.message : String(e);
  }
}

// Properties Obsidian shows as lists of tags; a single string counts as one.
const TAGGED = new Set(["tags", "tag"]);

/**
 * A note's YAML frontmatter as its properties, one per row: lists as chips, tags with their #,
 * true/false as a checkbox, [[links]] linked where the host resolves them. YAML that doesn't
 * parse to keys shows as written.
 */
export function Properties({ source }: { source: string }) {
  const [parsed, setParsed] = useState(() => (yaml ? parse(yaml, source) : null));
  useEffect(() => {
    let live = true;
    void load().then((y) => live && setParsed(parse(y, source)));
    return () => {
      live = false;
    };
  }, [source]);
  if (parsed === null) return null;
  if (typeof parsed === "string")
    return (
      <pre className="properties-raw">
        <code>{source}</code>
      </pre>
    );
  const entries = Object.entries(parsed);
  if (!entries.length) return null;
  return (
    <table className="properties">
      <tbody>
        {entries.map(([key, value]) => (
          <tr key={key}>
            <th scope="row">{key}</th>
            <td>
              <Value value={value} tags={TAGGED.has(key.toLowerCase())} />
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function Value({ value, tags }: { value: unknown; tags: boolean }): ReactNode {
  if (Array.isArray(value) || (tags && typeof value === "string"))
    return (
      <span className="flex flex-wrap gap-1">
        {(Array.isArray(value) ? value : value.split(/[\s,]+/).filter(Boolean)).map((v, i) => (
          <span key={i} className={tags ? "property-tag" : "property-chip"}>
            {tags ? `#${String(v).replace(/^#/, "")}` : <Value value={v} tags={false} />}
          </span>
        ))}
      </span>
    );
  if (typeof value === "boolean") return <input type="checkbox" checked={value} disabled aria-label={String(value)} />;
  if (value === null || value === undefined) return <span className="text-subtle">—</span>;
  if (value instanceof Date) return value.toISOString().slice(0, 10);
  if (typeof value === "object") return <code>{JSON.stringify(value)}</code>;
  return <Text value={String(value)} />;
}

/** A string, with the [[links]] in it linked when the host can. */
function Text({ value }: { value: string }) {
  const host = use(MarkdownHostContext);
  if (!host.wikilink || !value.includes("[[")) return value;
  const parts: ReactNode[] = [];
  let last = 0;
  for (const m of value.matchAll(/\[\[([^[\]\n]+?)\]\]/g)) {
    if (m.index > last) parts.push(value.slice(last, m.index));
    const target = parseWikilink(m[1]);
    parts.push(<span key={m.index}>{host.wikilink(target.anchor ? `${target.path}#${target.anchor}` : target.path, wikilinkLabel(target))}</span>);
    last = m.index + m[0].length;
  }
  if (last < value.length) parts.push(value.slice(last));
  return parts;
}
