import { ask, open } from "@tauri-apps/plugin-dialog";
import { Pencil, Plus, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Tip } from "@/components/ui/tooltip";
import { api, errorMessage, type GitIdentity } from "@/lib/api";
import { runCommand } from "@/lib/commands/keybindings";
import { FETCH_INTERVALS, updateSettings, useSettings } from "@/lib/settings";
import { enableNotifications } from "@/lib/app/notify";
import { failed, toast } from "@/lib/app/toast";
import { Segmented } from "@/components/ui/segmented";
import { Switch } from "@/components/ui/switch";
import { Field } from "@/features/settings/controls";

export function GitSection() {
  const s = useSettings();
  return (
    <>
      <Field label="Fetch in the background" hint="Keeps ahead / behind and the remote branches current for the open repository. A fetch that fails, say while offline, stays quiet.">
        <Segmented<string>
          value={String(s.backgroundFetch)}
          onChange={(v) => updateSettings({ backgroundFetch: Number(v) })}
          options={FETCH_INTERVALS.map((m) => ({ value: String(m), label: m ? `${m} min` : "Off" }))}
          variant="field"
        />
      </Field>
      <Field label="Notify when done in the background" hint="A desktop notification when a push, pull, fetch or clone ends while GitViber isn't the app in front. Turning it on asks your OS for permission.">
        <Switch checked={s.notify} onChange={(v) => void enableNotifications(v)} />
      </Field>
      <WorktreeRootField />
      <RepoIdentityField />
      <RemotesField />
    </>
  );
}

/** Off by default: worktrees stay in `<project>.worktrees` beside the project. */
function WorktreeRootField() {
  const { worktreeRoot } = useSettings();
  // Turning it on is picking the folder; a cancelled picker leaves it as it was.
  const choose = async () => {
    const picked = await open({ directory: true, defaultPath: worktreeRoot ?? undefined, title: "Folder for all worktrees" });
    if (typeof picked === "string") updateSettings({ worktreeRoot: picked });
  };
  return (
    <Field
      label="One folder for all worktrees"
      hint="New worktrees go in a subfolder named after the project, instead of a .worktrees folder beside each project. A folder picked for a project when making a worktree still comes first."
    >
      <div className="flex items-center gap-2">
        {worktreeRoot && (
          <>
            {/* The end of the path is what tells folders apart; see the New worktree dialog. */}
            <span dir="rtl" className="max-w-44 truncate text-left font-mono text-[11px] text-muted-foreground" title={worktreeRoot}>
              {`\u200e${worktreeRoot}\u200e`}
            </span>
            <Button type="button" variant="outline" size="sm" onClick={() => void choose()}>
              Change…
            </Button>
          </>
        )}
        <Switch checked={!!worktreeRoot} onChange={(v) => (v ? void choose() : updateSettings({ worktreeRoot: null }))} />
      </div>
    </Field>
  );
}

type RemoteRow = Awaited<ReturnType<typeof api.remoteList>>[number];

/** The repository's remotes: add, rename, repoint or remove one (VS Code's Add / Remove Remote). */
function RemotesField() {
  const [remotes, setRemotes] = useState<RemoteRow[] | null>(null);
  // The remote being edited (its name then), or "" for a new one.
  const [editing, setEditing] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [url, setUrl] = useState("");
  const load = () => api.remoteList().then(setRemotes, () => setRemotes(null));
  useEffect(() => void load(), []);
  if (!remotes) return null;
  const edit = (r: RemoteRow | null) => {
    setEditing(r?.name ?? "");
    setName(r?.name ?? (remotes.length ? "" : "origin"));
    setUrl(r?.url ?? "");
  };
  const act = async (fn: () => Promise<void>) => {
    try {
      await fn();
      setEditing(null);
    } catch (e) {
      toast("error", "Could not change the remote", errorMessage(e));
    }
    await load();
    runCommand("repo.refresh");
  };
  const save = () =>
    act(async () => {
      if (!editing) return api.remoteEdit("add", name.trim(), url.trim());
      const was = remotes.find((r) => r.name === editing);
      if (was && url.trim() !== was.url) await api.remoteEdit("set-url", editing, url.trim());
      if (name.trim() !== editing) await api.remoteEdit("rename", editing, name.trim());
    });
  const remove = async (r: RemoteRow) => {
    const ok = await ask(`Remove ${r.name}? Its remote branches go from this repository, and branches that track them stop tracking. The repository at ${r.url} is untouched.`, {
      title: "Remove remote",
      kind: "warning",
      okLabel: "Remove",
    });
    if (ok) await act(() => api.remoteEdit("remove", r.name));
  };
  const form = (
    <form
      className="flex flex-col gap-2 rounded-md border border-border p-2"
      onSubmit={(e) => {
        e.preventDefault();
        if (name.trim() && url.trim()) void save();
      }}
    >
      <Input autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="Name (origin, upstream…)" spellCheck={false} />
      <Input value={url} onChange={(e) => setUrl(e.target.value)} placeholder="URL (https://… or git@…)" spellCheck={false} className="font-mono" />
      <div className="flex justify-end gap-2">
        <Button type="button" size="sm" variant="secondary" onClick={() => setEditing(null)}>
          Cancel
        </Button>
        <Button type="submit" size="sm" disabled={!name.trim() || !url.trim()}>
          {editing ? "Save" : "Add"}
        </Button>
      </div>
    </form>
  );
  return (
    <Field label="Remotes" hint="Where this repository fetches from and pushes to.">
      <div className="flex w-64 flex-col gap-1.5">
        {remotes.map((r) =>
          editing === r.name ? (
            <div key={r.name}>{form}</div>
          ) : (
            <div key={r.name} className="group flex items-center gap-2 text-[12px]">
              <span className="shrink-0 font-medium">{r.name}</span>
              <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-muted-foreground" title={r.pushUrl ? `${r.url}\npush: ${r.pushUrl}` : r.url}>
                {r.url}
              </span>
              <Tip label="Edit">
                <button type="button" className="text-subtle hover:text-foreground" onClick={() => edit(r)} aria-label={`Edit ${r.name}`}>
                  <Pencil className="size-3.5" />
                </button>
              </Tip>
              <Tip label="Remove…">
                <button type="button" className="text-subtle hover:text-destructive" onClick={() => void remove(r)} aria-label={`Remove ${r.name}`}>
                  <Trash2 className="size-3.5" />
                </button>
              </Tip>
            </div>
          ),
        )}
        {editing === "" ? (
          form
        ) : (
          <Button type="button" size="sm" variant="secondary" className="self-start" onClick={() => edit(null)}>
            <Plus /> Add remote
          </Button>
        )}
      </div>
    </Field>
  );
}

const who = (i: GitIdentity) => (i.name || i.email ? `${i.name ?? "no name"} <${i.email ?? "no email"}>` : "not set");

/** As GitHub Desktop's repository settings: commit here as someone else than everywhere else. */
function RepoIdentityField() {
  const [loaded, setLoaded] = useState<{ own: GitIdentity; global: GitIdentity } | null>(null);
  const [own, setOwn] = useState(false);
  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  const load = () =>
    api.repoIdentity().then(
      (r) => {
        setLoaded(r);
        setOwn(!!(r.own.name || r.own.email));
        setName(r.own.name ?? r.global.name ?? "");
        setEmail(r.own.email ?? r.global.email ?? "");
      },
      // No repository open: nothing to set.
      () => setLoaded(null),
    );
  useEffect(() => void load(), []);
  if (!loaded) return null;
  const save = (identity: { name: string; email: string } | null) =>
    api.setRepoIdentity(identity).then(load, failed("Could not set the identity"));
  const changed = own && (name.trim() !== (loaded.own.name ?? "") || email.trim() !== (loaded.own.email ?? ""));
  return (
    <Field label="Commit as" hint={`Who commits in this repository. Everywhere else: ${who(loaded.global)} (your global git config).`}>
      <div className="flex w-64 flex-col gap-2">
        <Segmented<"global" | "own">
          value={own ? "own" : "global"}
          onChange={(v) => {
            setOwn(v === "own");
            if (v === "global" && (loaded.own.name || loaded.own.email)) void save(null);
          }}
          options={[
            { value: "global", label: "Global" },
            { value: "own", label: "This repository" },
          ]}
          variant="field"
        />
        {own && (
          <form
            className="flex flex-col gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              if (name.trim() && email.trim()) void save({ name, email });
            }}
          >
            <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="Name" spellCheck={false} />
            <Input type="email" value={email} onChange={(e) => setEmail(e.target.value)} placeholder="Email" spellCheck={false} />
            <Button type="submit" size="sm" disabled={!changed || !name.trim() || !email.trim()}>
              Save
            </Button>
          </form>
        )}
      </div>
    </Field>
  );
}
