# Vendored scripts

Reader mode's article extraction (`scripts/reader.js`), copied unchanged from the npm package
`@mozilla/readability` 0.6.0 (Apache-2.0, published 2025-03-03), source
<https://github.com/mozilla/readability>, tarball
<https://registry.npmjs.org/@mozilla/readability/-/readability-0.6.0.tgz> (integrity
`sha512-juG5VWh4qAivzTAeMzvY9xs9HY5rAcr2E4I7tiSSCokRFi7XIZCAu92ZkSTsIj1OPceCifL3cpfteP3pDT9/QQ==`,
checked against the registry).

| File | sha256 |
| --- | --- |
| `Readability.js` | `34dcab3d0832d0019f02990eed6b6124e029e8c32b9f0c6f2550544ff8dff174` |
| `Readability-readerable.js` | `a98d28805804c1986ceed470678a3f409f150ee7f1d227f8c8239c005d21de65` |

The JS tests parse their fixtures with `JSDOMParser.js` from the same tarball (MPL-2.0,
`crates/browser/tests/js/vendor/`, sha256
`6b8eae46939f187ef4565c67f452a9098211daea80e50c9aea74d72015b73680`); it doesn't ship.

To update: download the new tarball, check its integrity against the registry, copy the three
files over unchanged, and update the version and hashes here and in the app's license list
(`crates/app/src/dialogs/licenses.rs`).
