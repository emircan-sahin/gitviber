// A page's IndexedDB for a saved session (browser/agent.rs): the tail of an async function body in
// GitViber's world, with `input` a JSON string. {op: "list"} gives {databases: [], skipped} with a
// skipped entry for each database there is and nothing of its values: reading those can make macOS
// ask for the keychain's password (a CryptoKey in one), so it is for {op: "read"} to do, when asked.
// {op: "read", limit} gives {databases, skipped} of
// this origin, bytes within `limit`; {op: "write", databases} gives {skipped} of those it couldn't
// put back. A database is carried whole or named in `skipped` with why, never in part.
// Values are JSON with a tag for what JSON lacks: {"$": "date", "v": ms}, "buf", "view", "map",
// "set", "num", "big", "undef", "re", "utf16" for text with a lone surrogate (JSON text can't say
// it so the session file could be read back), and "obj" for an object with a "$" key of its own.
const ask = JSON.parse(input);

const unsupported = (what) => Object.assign(new Error(`holds ${what}, which a session can't carry`), { unsupported: true });
const tooBig = () => Object.assign(new Error("too big for a session"), { big: true });
const VIEWS = ["Int8Array", "Uint8Array", "Uint8ClampedArray", "Int16Array", "Uint16Array", "Int32Array", "Uint32Array", "Float32Array", "Float64Array", "BigInt64Array", "BigUint64Array", "DataView"];

const wellFormed = (text) => (text.isWellFormed ? text.isWellFormed() : !/[\ud800-\udbff](?![\udc00-\udfff])|(?<![\ud800-\udbff])[\udc00-\udfff]/.test(text));
// A name as shown in a reply: a broken character becomes the replacement character.
const shown = (text) => (wellFormed(text) ? text : text.replace(/[\ud800-\udbff](?![\udc00-\udfff])|(?<![\ud800-\udbff])[\udc00-\udfff]/g, "\ufffd"));
// Names (of databases, stores, indexes and paths) are text or the database is left out.
const named = (...names) => {
  for (const name of names.flat(Infinity)) if (typeof name === "string" && !wellFormed(name)) throw unsupported("a name that isn't text");
};
const units = (text) => Array.from({ length: text.length }, (_, i) => text.charCodeAt(i));
const fromUnits = (list) => {
  let text = "";
  for (let i = 0; i < list.length; i += 0x8000) text += String.fromCharCode(...list.slice(i, i + 0x8000));
  return text;
};

const base64 = (bytes) => {
  let text = "";
  for (let i = 0; i < bytes.length; i += 0x8000) text += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(text);
};
const bytesOf = (text) => Uint8Array.from(atob(text), (c) => c.charCodeAt(0));

const charge = (spent, bytes) => {
  spent.bytes += bytes;
  if (spent.bytes > spent.limit) throw tooBig();
};

const encode = (value, spent, seen = new Set()) => {
  charge(spent, 8);
  if (value === null || typeof value === "boolean") return value;
  if (typeof value === "string") {
    charge(spent, value.length);
    return wellFormed(value) ? value : { $: "utf16", v: units(value) };
  }
  if (typeof value === "number") return Number.isFinite(value) && !Object.is(value, -0) ? value : { $: "num", v: Object.is(value, -0) ? "-0" : String(value) };
  if (value === undefined) return { $: "undef" };
  if (typeof value === "bigint") return { $: "big", v: String(value) };
  if (typeof value !== "object") throw unsupported(`a ${typeof value}`);
  const kind = Object.prototype.toString.call(value).slice(8, -1);
  if (kind === "Date") return { $: "date", v: Number.isNaN(value.getTime()) ? null : value.getTime() };
  if (kind === "ArrayBuffer" || ArrayBuffer.isView(value)) {
    const bytes = kind === "ArrayBuffer" ? new Uint8Array(value) : new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
    charge(spent, Math.ceil(bytes.length * 1.34));
    return kind === "ArrayBuffer" ? { $: "buf", v: base64(bytes) } : { $: "view", t: kind, v: base64(bytes) };
  }
  if (kind === "RegExp") return { $: "re", s: value.source, f: value.flags };
  if (!["Array", "Object", "Map", "Set"].includes(kind)) throw unsupported(`a ${kind}`);
  if (seen.has(value)) throw unsupported("a value that contains itself");
  seen.add(value);
  try {
    if (kind === "Array") return Array.from(value, (v) => encode(v, spent, seen));
    if (kind === "Map") return { $: "map", v: Array.from(value, ([k, v]) => [encode(k, spent, seen), encode(v, spent, seen)]) };
    if (kind === "Set") return { $: "set", v: Array.from(value, (v) => encode(v, spent, seen)) };
    // No prototype: a key named __proto__ is a key, not the object's parent.
    const out = Object.create(null);
    for (const k of Object.keys(value)) {
      named(k);
      charge(spent, k.length);
      out[k] = encode(value[k], spent, seen);
    }
    return "$" in out ? { $: "obj", v: out } : out;
  } finally {
    seen.delete(value);
  }
};

const decode = (value) => {
  if (value === null || typeof value !== "object") return value;
  if (Array.isArray(value)) return value.map(decode);
  const mapped = (o) => Object.fromEntries(Object.entries(o).map(([k, v]) => [k, decode(v)]));
  if (typeof value.$ !== "string") return mapped(value);
  switch (value.$) {
    case "utf16": return fromUnits(value.v);
    case "num": return Number(value.v);
    case "big": return BigInt(value.v);
    case "undef": return undefined;
    case "date": return new Date(value.v ?? NaN);
    case "buf": return bytesOf(value.v).buffer;
    case "view": {
      const bytes = bytesOf(value.v);
      if (!VIEWS.includes(value.t)) throw new Error(`a view of kind ${value.t}`);
      return new globalThis[value.t](bytes.buffer);
    }
    case "re": return new RegExp(value.s, value.f);
    case "map": return new Map(value.v.map(([k, v]) => [decode(k), decode(v)]));
    case "set": return new Set(value.v.map(decode));
    case "obj": return mapped(value.v);
  }
  throw new Error(`a value of kind ${value.$}`);
};

const settle = (request) =>
  new Promise((resolve, reject) => {
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });

const readStore = (tx, name, spent) =>
  new Promise((resolve, reject) => {
    const store = tx.objectStore(name);
    const indexes = Array.from(store.indexNames, (n) => {
      const i = store.index(n);
      return { name: n, keyPath: i.keyPath, unique: i.unique, multiEntry: i.multiEntry };
    });
    named(name, store.keyPath, indexes.map((i) => [i.name, i.keyPath]));
    const out = { name, keyPath: store.keyPath, autoIncrement: store.autoIncrement, indexes, records: [] };
    const cursor = store.openCursor();
    cursor.onerror = () => reject(cursor.error);
    cursor.onsuccess = () => {
      const at = cursor.result;
      if (!at) return resolve(out);
      try {
        out.records.push({ key: encode(at.primaryKey, spent), value: encode(at.value, spent) });
      } catch (e) {
        tx.abort();
        return reject(e);
      }
      at.continue();
    };
  });

const readDatabase = async (name, spent) => {
  named(name);
  const opened = indexedDB.open(name);
  // One that was dropped since it was listed would be made empty: not made.
  opened.onupgradeneeded = () => opened.transaction.abort();
  const db = await settle(opened);
  try {
    const names = Array.from(db.objectStoreNames);
    if (!names.length) return { name, version: db.version, stores: [] };
    const tx = db.transaction(names, "readonly");
    const stores = await Promise.all(names.map((n) => readStore(tx, n, spent)));
    return { name, version: db.version, stores };
  } finally {
    db.close();
  }
};

const read = async () => {
  if (typeof indexedDB?.databases !== "function") return { databases: [], skipped: [{ name: "all", why: "can't be listed by this browser engine" }] };
  const spent = { bytes: 0, limit: ask.limit };
  const databases = [];
  const skipped = [];
  for (const { name } of await indexedDB.databases()) {
    const before = spent.bytes;
    try {
      databases.push(await readDatabase(name, spent));
    } catch (e) {
      spent.bytes = before;
      skipped.push({ name: shown(name), why: e?.unsupported || e?.big ? e.message : `couldn't be read (${e?.name ?? e})` });
    }
  }
  return { databases, skipped };
};

// A database the page hasn't made: made as it was saved, at its version.
const create = (saved) =>
  new Promise((resolve, reject) => {
    const request = indexedDB.open(saved.name, saved.version);
    let failure = null;
    request.onblocked = () => reject(new Error("kept open by the page"));
    request.onerror = () => reject(failure ?? request.error);
    request.onsuccess = () => {
      request.result.close();
      resolve();
    };
    request.onupgradeneeded = (event) => {
      try {
        // One the page made since: never taken apart from here.
        if (event.oldVersion !== 0) throw new Error("made by the page meanwhile");
        for (const s of saved.stores) {
          const store = request.result.createObjectStore(s.name, { keyPath: s.keyPath, autoIncrement: s.autoIncrement });
          for (const i of s.indexes) store.createIndex(i.name, i.keyPath, { unique: i.unique, multiEntry: i.multiEntry });
          for (const r of s.records) {
            if (s.keyPath === null) store.put(decode(r.value), decode(r.key));
            else store.put(decode(r.value));
          }
        }
      } catch (e) {
        failure = e;
        request.transaction.abort();
      }
    };
  });

const shape = (s) => JSON.stringify([s.keyPath, s.autoIncrement, s.indexes.map((i) => [i.name, i.keyPath, i.unique, i.multiEntry]).sort()]);

// A database the page has made already: its records replaced, in the stores it has, as saved. Not
// deleted and made again: a delete the page's open connection blocks waits to run, and would run
// later, at the reload, with nothing put back.
const replace = async (saved) => {
  const opened = indexedDB.open(saved.name);
  opened.onupgradeneeded = () => opened.transaction.abort();
  let db;
  try {
    db = await settle(opened);
  } catch {
    // It went since: nothing there to replace.
    return create(saved);
  }
  try {
    const names = Array.from(db.objectStoreNames);
    if (names.length !== saved.stores.length || !saved.stores.every((s) => names.includes(s.name))) {
      throw new Error("in the page already with other stores: left as it is");
    }
    if (!names.length) return;
    const tx = db.transaction(names, "readwrite");
    const done = new Promise((resolve, reject) => {
      tx.oncomplete = () => resolve();
      tx.onabort = () => reject(tx.error ?? new Error("aborted"));
    });
    done.catch(() => {});
    try {
      for (const s of saved.stores) {
        const store = tx.objectStore(s.name);
        const here = { keyPath: store.keyPath, autoIncrement: store.autoIncrement, indexes: Array.from(store.indexNames, (n) => ({ name: n, ...pick(store.index(n)) })) };
        if (shape(here) !== shape(s)) throw new Error(`in the page already with another shape (${s.name}): left as it is`);
      }
      for (const s of saved.stores) {
        const store = tx.objectStore(s.name);
        store.clear();
        for (const r of s.records) {
          if (s.keyPath === null) store.put(decode(r.value), decode(r.key));
          else store.put(decode(r.value));
        }
      }
    } catch (e) {
      tx.abort();
      throw e;
    }
    await done;
  } finally {
    db.close();
  }
};
const pick = (i) => ({ keyPath: i.keyPath, unique: i.unique, multiEntry: i.multiEntry });

const write = async () => {
  const skipped = [];
  for (const saved of ask.databases) {
    try {
      await replace(saved);
    } catch (e) {
      skipped.push({ name: shown(saved.name), why: shown(e?.message ?? String(e)) });
    }
  }
  return { skipped };
};

const list = async () => ({
  databases: [],
  skipped: typeof indexedDB?.databases === "function" ? (await indexedDB.databases()).map(({ name }) => ({ name: shown(name), why: "not asked for" })) : [],
});

return JSON.stringify(await { read, write, list }[ask.op]());
