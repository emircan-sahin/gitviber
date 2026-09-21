/** The bits of mdast this plugin touches. */
interface Node {
  type: string;
  value?: string;
  url?: string;
  children?: Node[];
}

// @login (GitHub's username rules) or #123, not inside a word, path or email address.
const REF = /(^|[^\w@/.-])(?:@([a-z\d](?:[a-z\d]|-(?=[a-z\d])){0,38})(?![\w/-])|#(\d+)\b)/gi;

/** Links @mentions to profiles and #123 to the repo's issues (GitHub redirects PRs), as GitHub does. */
export function remarkGithubRefs({ repo }: { repo: string }) {
  return (tree: Node) => walk(tree, repo);
}

function walk(node: Node, repo: string) {
  if (!node.children || node.type === "link" || node.type === "linkReference") return;
  node.children = node.children.flatMap((child) => {
    if (child.type !== "text") {
      walk(child, repo);
      return [child];
    }
    return split(child.value ?? "", repo);
  });
}

function split(value: string, repo: string): Node[] {
  const out: Node[] = [];
  let last = 0;
  for (const m of value.matchAll(REF)) {
    const start = m.index + m[1].length;
    if (start > last) out.push({ type: "text", value: value.slice(last, start) });
    const url = m[2] ? `https://github.com/${m[2]}` : `${repo}/issues/${m[3]}`;
    out.push({ type: "link", url, children: [{ type: "text", value: m[0].slice(m[1].length) }] });
    last = m.index + m[0].length;
  }
  if (last < value.length) out.push({ type: "text", value: value.slice(last) });
  return out;
}
