import { useEffect, useState } from "react";
import { Select } from "@/components/ui/select";
import { api, type Branch } from "@/lib/api";

/** What a new branch starts at: a local or remote branch, a tag, or with `head` HEAD. Values are full refs. */
export function BaseSelect({ value, onChange, branches, head }: { value: string; onChange: (ref: string) => void; branches: Branch[]; head: boolean }) {
  const [tags, setTags] = useState<string[]>([]);
  useEffect(() => {
    api.tags().then(setTags, () => setTags([]));
  }, []);
  const group = (title: string, prefix: string, names: string[]) =>
    names.length > 0 && (
      <optgroup label={title}>
        {names.map((b) => (
          <option key={b} value={`${prefix}${b}`}>
            {b}
          </option>
        ))}
      </optgroup>
    );
  return (
    <label className="mt-3 block text-[11.5px] text-muted-foreground">
      From
      <Select value={value} onChange={(e) => onChange(e.target.value)} className="mt-1 w-full font-mono">
        {head && <option value="HEAD">HEAD</option>}
        {group(
          "Local",
          "refs/heads/",
          branches.filter((b) => !b.remote).map((b) => b.name),
        )}
        {group(
          "Remote",
          "refs/remotes/",
          branches.filter((b) => b.remote).map((b) => b.name),
        )}
        {group("Tags", "refs/tags/", tags)}
      </Select>
    </label>
  );
}
