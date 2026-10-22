# Legacy behaviour catalogue

Specs of what the Java RomRaider does, one page per area, written by reading
`legacy/` only. The Java is never built or run
([D-024](../../.project/DECISIONS.md#d-024-no-java-in-the-project-including-tests-and-tooling)).
We port the intent, not the code. Story US-RR-12.

Every page:

- cites `legacy/` `file:line` for each behaviour it describes;
- cross-references the Must-Fix ledger
  ([PROJECT.md, Must-Fix Bugs](../../.project/PROJECT.md)) wherever legacy
  behaviour is a bug, rather than describing it as intended; its "Ledgered
  issues" table maps the problems first found on that page (the retired
  `UL-<AREA>-NN` triage IDs) to their ledger entries;
- ends with a "Fixtures to write" list with stable IDs, each of which becomes
  a tracked task or a KAT file.

`cargo xtask check-legacy-docs` (CI job `clippy`; rules in
`xtask/src/legacy_docs.rs`) enforces the first point and this index: every
reference resolves and is in range, names lines that mention what the text
says, and every described behaviour carries one (US-RR-12-1). It also
enforces the last point (US-RR-12-2): each page's last section is its
"Fixtures to write" table of `LEG-<AREA>-NN` IDs, and
[`fixtures-map.toml`](fixtures-map.toml) maps exactly those IDs, each to the
existing KAT, fixture or test files that pin it (`kat`) and/or the
ProjectMan task that writes it (`task`, which must exist and name the ID).
A new ID cannot be added untracked; when a task's fixture lands, its entry
moves to `kat`. And it enforces the second point (US-RR-12-3): every `MF-NN`
or `EC-NN` a page cites is in PROJECT.md's ledger, `UL-` IDs appear only in
the "Ledgered issues" tables, and every behaviour statement that calls
legacy behaviour a bug (a curated word list: "wrong", "silently",
"corrupt", ...) carries its ledger ID.

## Pages

| Page | Area | Task | Status |
|------|------|------|--------|
| [`definitions.md`](definitions.md) | ECU definition parsing and inheritance: `<romid>` and image matching, `base`/`omit`, tables, axes, switches, scalings, user levels, dead forms, error handling | US-RR-12-5 | written |
| [`encoding-scaling.md`](encoding-scaling.md) | Storage, byte order and scaling semantics: every storage type incl. `MOVI20`/`MOVI20S` and float, the float hack and static memory-model endian, masks and bit flags, Bosch subtract, 3D cell order, `expression`/`to_byte`, the binary-search inverse, rounding and clamping, the JEP dialect and `BitWise`, display formatting | US-RR-12-6 | written |
| [`checksums.md`](checksums.md) | ROM checksums: manager selection (`<checksum type>`, MF-11), Nissan `std`/`alt`/`alt2`, Motronic single/double, BMW byte XOR, `copy`, GM E38 segments and CVN, the Subaru "Checksum Fix" table and edit stamp, validate on open, the save flow and ordering dependencies | US-RR-12-7 | written |
| [`ssm.md`](ssm.md) | SSM / K-line protocol flow: init, read address, read memory, writes, echo, checksum, timing, error paths | US-RR-12-8 | written |
| [`logger.md`](logger.md) | Logger definition semantics: which file and protocol section, every element and attribute, addresses, decoding and value text, capability gating, ECU-specific parameters, derived parameters, the file-logging switch, external sensors, error handling | US-RR-12-9 | written |
| [`csv.md`](csv.md) | Logger CSV output: when a file is open, name and location, encoding and line endings, header, time column, row assembly, number format, locale and delimiter, quoting, mid-log changes, non-readings, the playback stub and dyno import, other CSVs | US-RR-12-9 | written |
| [`settings.md`](settings.md) | `settings.xml` for the importer: location, load and save, reading rules and the two kinds of default, encoding, every element and attribute with type, default and port relevance, what is not in it | US-RR-12-9 | written |
