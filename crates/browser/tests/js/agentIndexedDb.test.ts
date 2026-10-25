import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { browser } from "./agentSource.ts";

// scripts/indexeddb.js, what a saved session keeps of a page's IndexedDB, against a stand-in one:
// values of every kind it carries come back as they were, and a database it can't carry is named.
const script = readFileSync(new URL("scripts/indexeddb.js", browser), "utf8");
const AsyncFunction = Object.getPrototypeOf(async () => {}).constructor;
const run = new AsyncFunction("input", "indexedDB", "btoa", "atob", "setTimeout", script);

interface Index {
  name: string;
  keyPath: string | string[];
  unique: boolean;
  multiEntry: boolean;
}

interface StoreData {
  keyPath: string | string[] | null;
  autoIncrement: boolean;
  indexes: Index[];
  records: { key: unknown; value: unknown }[];
}

interface Data {
  version: number;
  stores: Map<string, StoreData>;
  open: number;
}

class Request<T = unknown> {
  result!: T;
  error: unknown = null;
  transaction?: Transaction;
  onsuccess?: () => void;
  onerror?: () => void;
  onblocked?: () => void;
  onupgradeneeded?: (event: { oldVersion: number }) => void;
}

class Transaction {
  aborted = false;
  error: unknown = null;
  oncomplete?: () => void;
  onabort?: () => void;
  db: Data;
  constructor(db: Data) {
    this.db = db;
  }
  objectStore(name: string) {
    const data = this.db.stores.get(name)!;
    return new ObjectStore(this, data);
  }
  abort() {
    this.aborted = true;
  }
}

class ObjectStore {
  tx: Transaction;
  data: StoreData;
  constructor(tx: Transaction, data: StoreData) {
    this.tx = tx;
    this.data = data;
  }
  get keyPath() {
    return this.data.keyPath;
  }
  get autoIncrement() {
    return this.data.autoIncrement;
  }
  get indexNames() {
    return this.data.indexes.map((i) => i.name);
  }
  index(name: string) {
    return this.data.indexes.find((i) => i.name === name)!;
  }
  createIndex(name: string, keyPath: string | string[], options: { unique: boolean; multiEntry: boolean }) {
    this.data.indexes.push({ name, keyPath, ...options });
  }
  clear() {
    this.data.records.length = 0;
  }
  put(value: unknown, key?: unknown) {
    const path = this.data.keyPath;
    if (path !== null && key !== undefined) throw new DOMException("key given for an in-line store", "DataError");
    if (path === null && key === undefined) throw new DOMException("no key", "DataError");
    const stored = structuredClone(value);
    const at = path === null ? key : (stored as Record<string, unknown>)?.[path as string];
    if (at === undefined) throw new DOMException("no key in the value", "DataError");
    this.data.records.push({ key: at, value: stored });
  }
  openCursor() {
    const request = new Request<unknown>();
    let i = 0;
    const step = () =>
      setTimeout(() => {
        if (this.tx.aborted) {
          request.error = new DOMException("aborted", "AbortError");
          return request.onerror?.();
        }
        const record = this.data.records[i++];
        request.result = record
          ? { primaryKey: structuredClone(record.key), value: structuredClone(record.value), continue: step }
          : null;
        request.onsuccess?.();
      }, 0);
    step();
    return request;
  }
}

/** An IndexedDB with the calls the script makes; `held` are databases the page keeps open. */
function world(databases: Record<string, StoreSpec[]> = {}, versions: Record<string, number> = {}) {
  const dbs = new Map<string, Data>();
  const deletes: string[] = [];
  const build = (name: string, specs: StoreSpec[], version = 1) => {
    const stores = new Map<string, StoreData>();
    for (const s of specs) {
      stores.set(s.name, { keyPath: s.keyPath ?? null, autoIncrement: s.autoIncrement ?? false, indexes: s.indexes ?? [], records: [] });
      const store = new ObjectStore(new Transaction({ version, stores, open: 0 }), stores.get(s.name)!);
      for (const [key, value] of s.records) store.put(value, s.keyPath ? undefined : key);
    }
    dbs.set(name, { version, stores, open: 0 });
  };
  for (const [name, specs] of Object.entries(databases)) build(name, specs, versions[name]);
  const connection = (data: Data, tx: Transaction) => ({
    version: data.version,
    get objectStoreNames() {
      return [...data.stores.keys()];
    },
    createObjectStore(name: string, options: { keyPath: string | null; autoIncrement: boolean }) {
      data.stores.set(name, { keyPath: options.keyPath, autoIncrement: options.autoIncrement, indexes: [], records: [] });
      return new ObjectStore(tx, data.stores.get(name)!);
    },
    transaction(names: string[]) {
      if (!names.length) throw new DOMException("no stores", "InvalidAccessError");
      const tx = new Transaction(data);
      setTimeout(() => (tx.aborted ? tx.onabort?.() : tx.oncomplete?.()), 5);
      return tx;
    },
    close() {
      data.open--;
    },
  });
  const indexedDB = {
    databases: async () => [...dbs].map(([name, d]) => ({ name, version: d.version })),
    open(name: string, version?: number) {
      const request = new Request<ReturnType<typeof connection>>();
      setTimeout(() => {
        let data = dbs.get(name);
        const created = !data;
        if (!data) dbs.set(name, (data = { version: version ?? 1, stores: new Map(), open: 0 }));
        const upgrading = created || (version !== undefined && version > data.version);
        if (version !== undefined && version < data.version) {
          request.error = new DOMException("older", "VersionError");
          return request.onerror?.();
        }
        if (upgrading && data.open) return request.onblocked?.();
        const tx = new Transaction(data);
        request.transaction = tx;
        request.result = connection(data, tx);
        if (upgrading) {
          data.version = version ?? data.version;
          request.onupgradeneeded?.({ oldVersion: created ? 0 : data.version - 1 } as never);
          if (tx.aborted) {
            if (created) dbs.delete(name);
            request.error = new DOMException("aborted", "AbortError");
            return request.onerror?.();
          }
        }
        data.open++;
        request.onsuccess?.();
      }, 0);
      return request;
    },
    deleteDatabase(name: string) {
      deletes.push(name);
      const request = new Request();
      setTimeout(() => {
        if (dbs.get(name)?.open) return request.onblocked?.();
        dbs.delete(name);
        request.onsuccess?.();
      }, 0);
      return request;
    },
  };
  return { dbs, deletes, indexedDB, keepOpen: (name: string) => void (dbs.get(name)!.open += 1) };
}

interface StoreSpec {
  name: string;
  keyPath?: string;
  autoIncrement?: boolean;
  indexes?: Index[];
  records: [unknown, unknown][];
}

const quick = (fn: () => void) => setTimeout(fn, 1);
const call = async (w: ReturnType<typeof world>, ask: object) =>
  JSON.parse(await run(JSON.stringify(ask), w.indexedDB, btoa, atob, quick));
const contents = (w: ReturnType<typeof world>) =>
  Object.fromEntries([...w.dbs].map(([n, d]) => [n, { version: d.version, stores: Object.fromEntries([...d.stores].map(([s, v]) => [s, v])) }]));

const kinds: StoreSpec = {
  name: "kinds",
  records: [
    ["date", new Date(1_700_000_000_000)],
    ["bad date", new Date(NaN)],
    ["buffer", new Uint8Array([1, 2, 255]).buffer],
    ["view", new Float64Array([1.5, -2])],
    ["map", new Map<unknown, unknown>([["a", { b: 1 }], [2, [3]]])],
    ["set", new Set([1, "x"])],
    ["odd numbers", [NaN, Infinity, -Infinity, -0, 0, 1.25]],
    ["undefined", { a: undefined, b: null }],
    ["bigint", 12345678901234567890n],
    ["regexp", /a+b/gi],
    ["tagged", { $: "date", v: 1, deeper: { $: "map" } }],
    ["unicode", "héllo \u{1F600} \u0000"],
  ],
};

test("values of every kind it carries come back as they were, with their stores' keys and indexes", async () => {
  const w = world({
    firebaseLocalStorageDb: [
      { name: "firebaseLocalStorage", keyPath: "fbase_key", records: [["", { fbase_key: "authUser", value: { uid: "u1", stsTokenManager: { accessToken: "tok" } } }]] },
    ],
    app: [kinds, { name: "byDate", keyPath: "id", autoIncrement: true, indexes: [{ name: "when", keyPath: "at", unique: false, multiEntry: false }], records: [["", { id: 7, at: new Date(5) }]] }, { name: "empty", records: [] }],
  }, { app: 40 });
  const before = structuredClone(contents(w));
  const read = await call(w, { op: "read", limit: 1 << 20 });
  assert.deepEqual(read.skipped, []);
  assert.deepEqual(read.databases.map((d: { name: string }) => d.name), ["firebaseLocalStorageDb", "app"]);
  assert.equal(read.databases[1].version, 40);
  // What goes in the session file is JSON, as written.
  assert.deepEqual(JSON.parse(JSON.stringify(read)), read);

  const fresh = world();
  assert.deepEqual(await call(fresh, { op: "write", databases: read.databases }), { skipped: [] });
  // Two invalid dates are never equal to each other: asserted apart.
  const after = contents(fresh);
  const bad = (c: ReturnType<typeof contents>) => c.app.stores.kinds.records.splice(1, 1)[0];
  assert.ok(Number.isNaN((bad(after).value as Date).getTime()));
  bad(before);
  assert.deepEqual(after, before);
});

test("a database it can't carry is named with why, the rest carried, and nothing of it kept", async () => {
  const w = world({
    good: [{ name: "s", records: [["k", "v"]] }],
    files: [{ name: "s", records: [["a", "fine"], ["b", { file: new Blob(["x"]) }]] }],
    loop: [{ name: "s", records: [["a", (() => { const o: Record<string, unknown> = {}; o.self = o; return o; })()]] }],
  });
  const read = await call(w, { op: "read", limit: 1 << 20 });
  assert.deepEqual(read.databases.map((d: { name: string }) => d.name), ["good"]);
  assert.deepEqual(read.skipped, [
    { name: "files", why: "holds a Blob, which a session can't carry" },
    { name: "loop", why: "holds a value that contains itself, which a session can't carry" },
  ]);
  assert.ok(!JSON.stringify(read).includes("fine"), "no record of a database left out");
});

test("past the cap a database is left out whole, and a smaller one after it still fits", async () => {
  const w = world({
    big: [{ name: "s", records: [["a", "x".repeat(5000)]] }],
    small: [{ name: "s", records: [["a", "y"]] }],
  });
  const read = await call(w, { op: "read", limit: 2000 });
  assert.deepEqual(read.databases.map((d: { name: string }) => d.name), ["small"]);
  assert.deepEqual(read.skipped, [{ name: "big", why: "too big for a session" }]);
});

test("a load replaces the records of a database the page has open, in place and without a delete", async () => {
  const source = world({
    kept: [{ name: "s", records: [["a", 1]] }],
    other: [{ name: "s", records: [["b", 2]] }],
  });
  const { databases } = await call(source, { op: "read", limit: 1 << 20 });
  const page = world({ kept: [{ name: "s", records: [["z", 0], ["y", 9]] }] });
  page.keepOpen("kept");
  assert.deepEqual(await call(page, { op: "write", databases }), { skipped: [] });
  assert.deepEqual(contents(page), contents(source));
  // A delete the page's connection blocks would wait and run at the reload, with nothing put back.
  assert.deepEqual(page.deletes, []);
});

test("a database the page made in another shape is left as it is, named, and nothing is deleted", async () => {
  const source = world({ app: [{ name: "s", keyPath: "id", records: [["", { id: 1 }]] }], more: [{ name: "s", records: [["a", 1]] }] });
  const { databases } = await call(source, { op: "read", limit: 1 << 20 });
  const page = world({
    app: [{ name: "s", keyPath: "other", records: [["", { other: 5 }]] }],
    more: [{ name: "s", records: [] }, { name: "extra", records: [] }],
  });
  const wrote = await call(page, { op: "write", databases });
  assert.deepEqual(wrote.skipped.map((x: { name: string }) => x.name), ["app", "more"]);
  assert.match(wrote.skipped[0].why, /another shape/);
  assert.match(wrote.skipped[1].why, /other stores/);
  assert.deepEqual(page.deletes, []);
  assert.equal(page.dbs.get("app")!.stores.get("s")!.records.length, 1, "as the page had it");
});

test("a key named __proto__ and text with a lone surrogate come back as they were, and a bad name leaves out its database only", async () => {
  const odd = JSON.parse('{"__proto__": {"x": 1}, "a": 2}');
  const lone = "ok \ud800 broken \udc00 end";
  const w = world({
    values: [{ name: "s", records: [["odd", odd], ["lone", lone], ["pair", "\u{1F600}"], ["map", new Map([[lone, 1]])]] }],
    ["bad\ud800name"]: [{ name: "s", records: [["a", 1]] }],
    badkey: [{ name: "s", records: [["a", JSON.parse('{"k\\ud800": 1}')]] }],
  });
  const read = await call(w, { op: "read", limit: 1 << 20 });
  assert.deepEqual(read.databases.map((d: { name: string }) => d.name), ["values"]);
  assert.deepEqual(read.skipped.map((x: { name: string }) => x.name), ["bad\ufffdname", "badkey"]);
  // The session file is read back by a parser that refuses a lone surrogate's escape.
  assert.ok(!/\\u[dD][89a-fA-F]/.test(JSON.stringify(read)));
  const fresh = world();
  assert.deepEqual(await call(fresh, { op: "write", databases: read.databases }), { skipped: [] });
  const back = fresh.dbs.get("values")!.stores.get("s")!.records.map((r) => r.value) as [Record<string, unknown>, string, string, Map<string, number>];
  assert.deepEqual(Object.keys(back[0]), ["__proto__", "a"]);
  assert.equal(Object.getPrototypeOf(back[0]), Object.prototype);
  assert.equal(back[1], lone);
  assert.equal(back[2], "\u{1F600}");
  assert.equal([...back[3].keys()][0], lone);
});

test("a load replaces a database of the same name instead of merging into it", async () => {
  const source = world({ app: [{ name: "s", records: [["a", "saved"]] }] });
  const { databases } = await call(source, { op: "read", limit: 1 << 20 });
  const page = world({ app: [{ name: "s", records: [["a", "page's own"], ["b", "extra"]] }] });
  await call(page, { op: "write", databases });
  assert.deepEqual(contents(page), contents(source));
});

test("a record the engine refuses leaves its database out, with the engine's word for it", async () => {
  const databases = [{ name: "bad", version: 1, stores: [{ name: "s", keyPath: "id", autoIncrement: false, indexes: [], records: [{ key: 1, value: 1 }] }] }];
  const page = world();
  const wrote = await call(page, { op: "write", databases });
  assert.equal(wrote.skipped.length, 1);
  assert.equal(wrote.skipped[0].name, "bad");
  assert.equal(page.dbs.has("bad"), false, "no half-made database");
});

test("a session file can't make the script build anything but a view of memory", async () => {
  const databases = [{ name: "x", version: 1, stores: [{ name: "s", keyPath: null, autoIncrement: false, indexes: [], records: [{ key: "k", value: { $: "view", t: "Function", v: "" } }] }] }];
  const page = world();
  const wrote = await call(page, { op: "write", databases });
  assert.match(wrote.skipped[0].why, /a view of kind Function/);
});

test("listing names the databases and reads none of their values, so nothing is asked of the keychain", async () => {
  const w = world({ app: [{ name: "s", records: [["a", 1]] }], other: [{ name: "s", records: [] }] });
  let opened = 0;
  const open = w.indexedDB.open;
  w.indexedDB.open = ((...a: Parameters<typeof open>) => (opened++, open(...a))) as typeof open;
  const listed = await call(w, { op: "list" });
  assert.deepEqual(listed, { databases: [], skipped: [{ name: "app", why: "not asked for" }, { name: "other", why: "not asked for" }] });
  assert.equal(opened, 0, "no database was opened");
});

test("an engine that can't list databases says so instead of failing the save", async () => {
  const w = world({ app: [{ name: "s", records: [] }] });
  (w.indexedDB as { databases?: unknown }).databases = undefined;
  const read = await call(w, { op: "read", limit: 100 });
  assert.deepEqual(read, { databases: [], skipped: [{ name: "all", why: "can't be listed by this browser engine" }] });
});
