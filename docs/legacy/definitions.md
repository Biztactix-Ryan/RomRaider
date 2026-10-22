# ECU definitions: legacy parsing and inheritance

What the Java RomRaider does with an ECU definition file (the editor's
`<roms>` XML): which elements and attributes it reads, what each defaults to,
how a ROM image is matched to a definition, how `base` inheritance and `omit`
resolve, what user levels mean, which forms are parsed but never used, and
what happens to malformed input. Written from the source only (the Java was
never run, D-024). It is the spec behind the RomRaider definition reader and
resolver (`rr-defs`, US-RR-29, US-RR-34) and its known-answer fixtures. Part
of the legacy behaviour catalogue, story US-RR-12, task US-RR-12-5.

Storage widths, byte order, MOVI20, masks, the Bosch-subtract layout and the
JEP expression dialect are only touched here where the definition reader
decides them; their decoding semantics belong to `encoding-scaling.md`
(US-RR-12-6). Checksum algorithms are in `checksums.md` (US-RR-12-7).

## How to read this page

- **References.** Source references are relative to
  `legacy/src/main/java/com/romraider/`, the convention the PROJECT.md
  Must-Fix ledger uses: `xml/TableScaleUnmarshaller.java:150` is
  `legacy/src/main/java/com/romraider/xml/TableScaleUnmarshaller.java`,
  line 150. Files elsewhere in `legacy/` are written with the `legacy/`
  prefix (`legacy/definitions/ecu_defs.dtd:3`). Every reference is
  `file:line` or `file:first-last`.
- **Bugs.** Where legacy behaviour is wrong or fragile, the page says so and
  cites the Must-Fix ledger entry (`MF-nn`) or edge case (`EC-nn`) of
  PROJECT.md. Do not port it as intended. A sentence that calls legacy
  behaviour a bug always carries its ledger ID (`cargo xtask
  check-legacy-docs`, rule 7). [Ledgered issues](#ledgered-issues) maps the
  problems first found on this page (`UL-DEF-nn`, IDs that tasks and
  fixtures still cite) to their ledger entries.
- **Names.** "Element names" compare ignoring case everywhere legacy reads
  them (`equalsIgnoreCase`); attribute names are case-sensitive, because
  attributes are read through `getNamedItem` (`xml/DOMHelper.java:48-51`).
  The exceptions are the attribute loop of `<scaling>` (section 6.2) and
  `<checksum>`, whose attribute names are lower-cased (section 7).

### Source files

| Alias | Path under `legacy/src/main/java/com/romraider/` | Role |
|-------|---------------------------------------------------|------|
| `xml/DOMRomUnmarshaller.java` | same | `<rom>`, `<romid>`, `base` lookup, `<checksum>`, ROM matching |
| `xml/TableScaleUnmarshaller.java` | same | `<table>`, axes, `<state>`, `<bit>`, `<scaling>`, `<scalingbase>`, table `base`, `omit`, the address filter |
| `xml/RomAttributeParser.java` | same | Value parsers: hex, storage type, signedness, endian, axis type, file size |
| `xml/DOMHelper.java` | same | Text and attribute readers, their defaults and the `NaN` rule |
| `xml/TableIsOmittedException.java`, `xml/RomNotFoundException.java` | same | The two control-flow exceptions |
| `maps/RomID.java` | same | ROM identity fields, the image match, RAM offset |
| `maps/Rom.java` | same | The table map, populate, tree visibility, error popups |
| `maps/Table.java`, `Table1D.java`, `Table2D.java`, `Table3D.java`, `TableSwitch.java`, `TableBitwiseSwitch.java` | `maps/` | Table kinds and their defaults |
| `maps/Scale.java`, `maps/PresetManager.java`, `maps/DataCell.java` | `maps/` | Scaling defaults; switch states; where user level and lock are enforced |
| `util/HexUtil.java`, `util/ObjectCloner.java` | `util/` | Hex parsing; the deep copy behind table `base` |
| `editor/ecu/OpenImageWorker.java` | same | Opening an image: which files, in which order, and every error popup |
| `Settings.java`, `xml/DOMSettingsUnmarshaller.java` | same | The definition file list, user level, `displayhightables`, `savedebugtables`, `defaultscale` |
| `legacy/definitions/ecu_defs.dtd` | (outside `src/`) | A DTD that does **not** describe what the parser reads (section 9) |
| `legacy/src/test/definitions/scalingbase_test.xml`, `legacy/src/test/java/com/romraider/xml/TableScaleUnmarshallerTest.java` | (outside `src/main/`) | Legacy's own definition test: a usable source of expected values |

## 1. Loading: which file, which `<rom>`

### 1.1 The definition file list

The user keeps an ordered list of definition files (the Definition Manager).
It is saved in `settings.xml` as `<ecudefinitionfile name="...">`, one per
file, default name `ecu_defs.xml` (`xml/DOMSettingsUnmarshaller.java:109-110`;
`Settings.java:178,308-318`). Opening an image
(`editor/ecu/OpenImageWorker.java:200-258`):

1. Reads the whole image into memory (`OpenImageWorker.java:211`).
2. For each listed file, **in list order**: a missing file gives a "missing
   or moved" popup and is skipped (`OpenImageWorker.java:223-233`); the file
   is parsed into a DOM (every time, for every image: MF-40) and its `<rom>`s
   are tried against the image (section 2). A parse error gives a popup and
   the next file is tried (`OpenImageWorker.java:238-244`).
3. The **first** file with a matching `<rom>` wins: it is loaded and the
   search stops, even if loading it then fails (`OpenImageWorker.java:246-250`;
   MF-56). Two definitions matching one image are never noticed (EC-03).
4. No match anywhere: the user is asked to pick a file. If that file has no
   matching `<rom>` either, the user can force-load it, which takes the
   file's **first** `<rom>` element (`OpenImageWorker.java:260-312`;
   `xml/DOMRomUnmarshaller.java:81-93`), usually the abstract base, not a
   real ROM (MF-56).

### 1.2 XML parsing

`DocumentBuilderFactory` with namespaces on and **XInclude on**, nothing
else configured: DTDs, external entities and entity expansion are the
platform defaults (`editor/ecu/OpenImageWorker.java:108-111`). XInclude is
the only way legacy could split one set of `<rom>`s over several files
(section 3.4). Hostile XML is EC-09. Files that need a conversion layer (XDF,
VDF, BMW) are converted to the same DOM shape first
(`OpenImageWorker.java:115-123`; MF-20, MF-21; out of scope here).

The root element's name is never checked: the code takes whatever
`getDocumentElement()` returns (`OpenImageWorker.java:172,240`) and looks
for `rom` and `scalingbase` children (`xml/DOMRomUnmarshaller.java:104-108`;
`xml/TableScaleUnmarshaller.java:65-77`). The shipped convention is `<roms>`.

### 1.3 Reading text and attributes

- **Text** is the concatenation of an element's direct text-node children,
  **untrimmed**; CDATA sections and nested elements are skipped
  (`xml/DOMHelper.java:30-46`). So `<xmlid> X </xmlid>` is `" X "`.
- **Attributes.** A missing attribute **or one whose value is exactly
  `NaN`** gives the default (`xml/DOMHelper.java:48-51`). Typed overloads
  parse the string, or the default turned into a string, with
  `Double.parseDouble`, `Integer.parseInt` (decimal) or
  `Boolean.parseBoolean` (`xml/DOMHelper.java:53-63`). So a boolean is true
  only for `true` in any case: `1` and `yes` are false; and a bad number
  throws `NumberFormatException` (section 8).

## 2. `<romid>` and matching an image

### 2.1 Elements read

`unmarshallRomID` walks the `<romid>`'s child elements; anything else is
skipped without a word (`xml/DOMRomUnmarshaller.java:216-292`).

| Element | Parsed as | Field / effect | Used for | Source |
|---------|-----------|----------------|----------|--------|
| `xmlid` | text | `xmlid` | `base` lookup (case-insensitive), progress text, display | `xml/DOMRomUnmarshaller.java:226-227,127`; `swing/RomPropertyPanel.java:48` |
| `internalidaddress` | hex (`HexUtil.hexToInt`), default 0 | where the ID is in the image | matching | `xml/DOMRomUnmarshaller.java:229-231`; `maps/RomID.java:37,58-95` |
| `internalidstring` | text, default `""` | the ID: ASCII, or hex bytes if it starts with `0x` | matching | `xml/DOMRomUnmarshaller.java:233-234`; `maps/RomID.java:38,58-95` |
| `author`, `version` | text | display | tree tooltip, properties panel | `xml/DOMRomUnmarshaller.java:236-240`; `swing/RomCellRenderer.java:81-82,108-109` |
| `caseid` | text | display ("ECU version") | | `xml/DOMRomUnmarshaller.java:242-243`; `swing/RomPropertyPanel.java:49` |
| `ecuid` | text, default `""` | stored, **never read** | nothing (dead, section 9) | `xml/DOMRomUnmarshaller.java:245-246`; `maps/RomID.java:40` |
| `make`, `market`, `model`, `submodel`, `transmission`, `year` | text | display | tree text, properties panel | `xml/DOMRomUnmarshaller.java:248-264`; `swing/RomCellRenderer.java:90-103`; `swing/RomPropertyPanel.java:59-64` |
| `noramoffset` | presence | RAM offset forced to 0 | addressing (2.3) | `xml/DOMRomUnmarshaller.java:266-267`; `maps/RomID.java:241-244` |
| `offset` | hex | RAM offset = −offset, fixed | addressing (2.3) | `xml/DOMRomUnmarshaller.java:269-270`; `maps/RomID.java:236-239` |
| `flashmethod` | text | stored, **never read** | nothing (dead) | `xml/DOMRomUnmarshaller.java:272-273` |
| `memmodel` | text, plus attribute `endian` | text stored and **never read**; `endian` overrides every table's byte order (section 5.3) | encoding | `xml/DOMRomUnmarshaller.java:275-278`; `xml/TableScaleUnmarshaller.java:61-63,162-170` |
| `filesize` | size (2.2) | declared image size | RAM offset (2.3) | `xml/DOMRomUnmarshaller.java:279-281` |
| `obsolete` | `Boolean.parseBoolean` | "a newer revision exists" popup on open, if the user has not turned it off | UI only | `xml/DOMRomUnmarshaller.java:283-284`; `editor/ecu/ECUEditor.java:404` |

Not read at all, whatever other tools write: `internalidhex`,
`checksummodule`, `ramoffset` (as an element), and every `<rom>` attribute
other than `base` (section 9).

A `<rom>` with two `<romid>`s is read twice into the same object, the later
value of each element winning (`xml/DOMRomUnmarshaller.java:164-166`); only
the **first** is used for matching (`xml/DOMRomUnmarshaller.java:112-131`,
the `break` after the first `romid`).

### 2.2 `filesize`

`RomAttributeParser.parseFileSize` (`xml/RomAttributeParser.java:238-250`):
the last two characters `kb` (any case) mean ×1024, `mb` ×1024², a last
character `b` means bytes, otherwise the whole string is a decimal integer.
`512kb` = 524288, `1mb` = 1048576, `1024` = 1024. The number is
`Integer.parseInt`, so `512 kb` (space) throws, and a value shorter than two
characters (`"8"`, `""`) throws `StringIndexOutOfBoundsException` from
`substring(-1)` (MF-55).

### 2.3 RAM offset: definition address to image offset

After the whole chain is read, `ramOffset = filesize − image length`
(`xml/DOMRomUnmarshaller.java:73-76`), unless `noramoffset` or `offset` set
it first; `setRamOffset` is then a no-op (`maps/RomID.java:250-254`). A cell's
image offset is `storageaddress + index × width − ramOffset`
(`maps/DataCell.java:151-190`), so:

- `filesize` equal to the image size: offset 0.
- A definition without `filesize` has `fileSize = 0`, so `ramOffset =
  −length` and every table lands past the end of the image (EC-02).
- `<offset>N</offset>`: image offset = address + N.
- A table with `beforeram="true"` keeps `ramOffset = 0` whatever the ROM
  says (`maps/Table.java:157-159`; `maps/TableBitwiseSwitch.java:42-44`).

### 2.4 The image match

`findRomNodeMatch(root, null, image)` walks the root's `<rom>` children in
document order, builds a fresh `RomID` from each one's **own** first
`<romid>` (no inheritance) and returns the first that matches
(`xml/DOMRomUnmarshaller.java:97-138`). `RomID.checkMatch`
(`maps/RomID.java:58-95`):

1. Empty or missing `internalidstring`: no match. So a base `<rom>` with no
   ID never matches an image.
2. `internalidaddress = -1` and `internalidstring = force` (any case): match,
   whatever the image.
3. `internalidstring` longer than 2 characters starting `0x` (any case):
   the rest is hex (`HexUtil.asBytes`: spaces and every `0x` removed, odd
   length left-padded with `0`, `util/HexUtil.java:51-60,136-154`); the image
   bytes at `internalidaddress` must equal them exactly.
4. Otherwise the image bytes at `internalidaddress`, `length(id)` of them,
   decoded with the **platform default charset**, must equal the ID ignoring
   case.

An ID running past the end of the image is no match; any exception is logged
and is no match (`maps/RomID.java:73,83,90-94`). The returned `<rom>` is the
first match in the first matching file: there is no ambiguity check (EC-03;
`xml/DOMRomUnmarshaller.java:104-123`; `editor/ecu/OpenImageWorker.java:246-250`).
A child `<rom>` that leaves out `internalidaddress` is matched at address 0
(`maps/RomID.java:37`; its own `<romid>` only,
`xml/DOMRomUnmarshaller.java:118-119`), although the loaded ROM then
inherits its base's address (MF-56).

## 3. `<rom base>` inheritance

### 3.1 Order of resolution

`unmarshallXMLDefinition` reads the root's `<scalingbase>`s, then calls
`unmarshallRom(matchedRom, new Rom(new RomID()))`
(`xml/DOMRomUnmarshaller.java:61-79`). `unmarshallRom`
(`xml/DOMRomUnmarshaller.java:140-199`) does, in order:

1. `filterFoundRomTables` over this `<rom>`'s children (section 4.1).
2. If the `<rom>` has a `base` other than `none` (compared ignoring case):
   find the `<rom>` whose own `<romid><xmlid>` equals it, ignoring case, in
   the **same document** (`rootNode.getParentNode()`), first match in
   document order, and **recurse** into it with the same `Rom` object
   (`xml/DOMRomUnmarshaller.java:149-154,201-214`). Then set `obsolete =
   false`.
3. Walk this `<rom>`'s children in document order: `<romid>` (read into the
   same `RomID`, so every element it gives overrides the base's and every
   element it leaves out is inherited), `<table>` (section 4), `<checksum>`
   (section 7), `<Simulation>` (section 7); anything else is skipped.

So the root of the chain is read first and the matched `<rom>` last; each
level changes the one `Rom` object (`xml/DOMRomUnmarshaller.java:151-152,208`).
The **matched ROM's values win**, field by field
(`xml/DOMRomUnmarshaller.java:156-166`). `obsolete` is the one `<romid>`
field that is never inherited: it is reset after each base and only the
`<rom>` that says it is obsolete is (`xml/DOMRomUnmarshaller.java:153,209`).

### 3.2 Missing base, cycles, depth

| Case | Legacy | Source |
|------|--------|--------|
| `base` names no `<rom>` of this file | `RomNotFoundException` (no message) aborts the whole load: a popup with the generic "load exception" text | `xml/DOMRomUnmarshaller.java:213`; `editor/ecu/OpenImageWorker.java:144-163,192-194` |
| `base` names a `<rom>` in another file | Same as missing: lookup is per document (**MF-17**) | `xml/DOMRomUnmarshaller.java:151` |
| `base` names its own `xmlid`, or `A → B → A` | Unbounded recursion until `StackOverflowError`, caught as "Looped base attribute in XML definitions" (**MF-17**) | `editor/ecu/OpenImageWorker.java:178-184`; `legacy/i18n/com/romraider/editor/ecu/ECUEditor.properties:38` |
| Two `<rom>`s with the same `xmlid` in one file | The first in document order is the base; the later one is unreachable as a base | `xml/DOMRomUnmarshaller.java:104-131` |
| Diamond | Not expressible: one `base` per `<rom>` | `xml/DOMRomUnmarshaller.java:149-152` |

EC-04 lists the cases the port must test.

### 3.3 Side effects of looking a base up

Finding a base by `xmlid` reads the `<romid>` of every `<rom>` before it in
the file with `unmarshallRomID`, which also sets the shared memory-model
endian of the table reader (`xml/DOMRomUnmarshaller.java:118-119,275-278`).
A base without a `<memmodel>` therefore takes the endian of the last earlier
`<rom>` in the file that had one (MF-52), and a malformed `<romid>` of any
earlier `<rom>` aborts the load (MF-55).

### 3.4 Across files

Only XInclude (section 1.2) can make another file's `<rom>`s visible, by
splicing them into one document before any lookup. Without it a `base` is
always same-file (**MF-17**).

## 4. `<table>`

### 4.1 Which tables exist: the address filter

Before reading tables, `filterFoundRomTables` records, for every top-level
`<table>` of the `<rom>` being read, `name → storageaddress`
(`xml/TableScaleUnmarshaller.java:317-345`):

- `storageaddress` absent is `-1` (parsed as hex); only addresses `≥ 0` are
  recorded.
- The first level to give a name an address keeps it, except that an
  address below 1 (that is, 0) may be replaced by a later level's. Since the
  matched ROM is filtered before its bases, **the matched ROM's address wins**.
- The map is keyed by the exact, case-sensitive name.
- It **returns** at the first `<table omit="true">`, so tables after it in
  that `<rom>` are never recorded (**MF-07**).

When a `<table>` element is read and no table of that name exists yet
(neither inherited nor made by a table `base`), it is created only if its
name is in that map or its `type` contains the substring `xis` (an axis
type); otherwise it is skipped
(`xml/TableScaleUnmarshaller.java:100-139`). This is how an abstract base
lists every table it knows and each ROM keeps only the ones it places. A
table with no address in any level never exists.

At populate time a table whose address is negative is removed from the map
while iterating over it (**MF-23**, `maps/Rom.java:319-322`).

### 4.2 Reading one `<table>`

`unmarshallTable(element, existing, rom)`
(`xml/TableScaleUnmarshaller.java:79-307`), where `existing` is the table of
that name already in the `Rom` (inherited or made earlier in this `<rom>`),
looked up ignoring case (`xml/DOMRomUnmarshaller.java:170`;
`maps/Rom.java:228-243`):

1. `omit="true"` (any case): throw `TableIsOmittedException`; the caller
   removes the existing table of that name, if any
   (`xml/TableScaleUnmarshaller.java:82-85`;
   `xml/DOMRomUnmarshaller.java:177-181`).
2. `base` other than `none`: the table becomes a **deep copy** of the ROM's
   table of that name, by Java serialization (`xml/TableScaleUnmarshaller.java:87-98`;
   `util/ObjectCloner.java:33-94`). The copy serializes the table's `Rom`
   back-reference too, so it copies the whole ROM graph (**MF-18**). A name
   that finds no table copies `null`, which succeeds silently (MF-51).
3. No table yet: create one by `type` (4.3), if the filter allows.
4. Apply the attributes (4.4) over the existing values, then the children
   (4.5) in document order.

The result is stored under its lower-cased name, replacing any earlier entry
(`maps/Rom.java:208-211`). A second `<table>` of the same name in one `<rom>`
therefore changes the first, exactly as a child level does. Table order is
insertion order (`maps/Rom.java:90`), base tables first.

### 4.3 `type`

Read only when the table is created; a level that changes an existing table
cannot change its kind (`xml/TableScaleUnmarshaller.java:100-136`). Compared
ignoring case:

| `type` | Kind | Notes |
|--------|------|-------|
| `1D` | `Table1D` | |
| `2D` | `Table2D` | one axis |
| `3D` | `Table3D` | X and Y axes |
| `X Axis`, `Static X Axis`, `Y Axis`, `Static Y Axis` | `Table1D` | as a top-level table; passes the address filter even when unaddressed (the `xis` test is case-sensitive, so `X AXIS` does not) |
| `Switch` | `TableSwitch` (a `Table1D`) | storage type defaults to 1 (`maps/TableSwitch.java:37-40`) |
| `BitwiseSwitch` | `TableBitwiseSwitch` (a `TableSwitch`) | storage type defaults to 1 (`maps/TableBitwiseSwitch.java:29-31`) |
| absent (`none`) | — | `XMLParseException` "Table type unspecified", logged; table skipped |
| anything else (`3`, `2d table`) | — | `XMLParseException` "Table type … unknown", logged; table skipped |

The skip is `LOGGER.error` only (`xml/DOMRomUnmarshaller.java:182-184`).

### 4.4 Attributes

Every attribute defaults to the value the table already has (inherited, or
the class default for a new table), except where noted. Order of application
is `xml/TableScaleUnmarshaller.java:142-211`.

| Attribute | Parse | Default (new table) | Inheritance quirk | Source |
|-----------|-------|---------------------|-------------------|--------|
| `name` | text | `unknown` for the filter; `null` once created | A top-level table must have one: a nameless one throws `NullPointerException` (MF-55) | `xml/TableScaleUnmarshaller.java:102,143-144`; `xml/DOMRomUnmarshaller.java:170` |
| `beforeram` | `true` (any case) sets it | false | Can be set, never cleared: `beforeram="false"` keeps an inherited `true` (MF-50) | `xml/TableScaleUnmarshaller.java:145-148` |
| `dataLayout` | `bosch_subtract` (trimmed, any case) or default | `DEFAULT` | **Not inherited**: reset to `DEFAULT` on every `<table>` without it (MF-50). Attribute name is case-sensitive camel case | `xml/TableScaleUnmarshaller.java:150`; `maps/Table.java:520-527` |
| `category` | text; `" //"` and `"// "` normalised to `//`, trimmed | `Other` | inherited | `xml/TableScaleUnmarshaller.java:151-152`; `maps/Table.java:46,183-186` |
| `storagetype` | `movi20` → 20, `movi20s` → 28, `float` → 99, `uintN` → N/8, `intN` → N/8, else a decimal byte count | `0` (1 for switches) | Signedness is set only while the width is still below 1, i.e. by the first level that gives a width (**MF-06**). `uint24` gives 3 and `int64` 8, which the decoder does not handle (**MF-02**); no `storagetype` anywhere leaves width 0, which reads every cell as 0 (MF-02). `UINT8` throws: the `uint`/`int` prefix test is case-sensitive while the sign test lower-cases (MF-60) | `xml/TableScaleUnmarshaller.java:153-161`; `xml/RomAttributeParser.java:52-81`; `Settings.java:122-124` |
| `endian` | `big` or `2` → big; `little` or `1` → little; **anything else → little** | big | inherited; ignored entirely when the ROM's `<memmodel endian>` is set (5.3) | `xml/TableScaleUnmarshaller.java:162-170`; `xml/RomAttributeParser.java:36-46`; `maps/Table.java:56`; `Settings.java:93-106` |
| `storageaddress` | hex (`0x` optional, lower case only; up to `7FFFFFFF`) | 0 | Taken from the address filter when the name is there (the matched ROM's address); otherwise the attribute, whose default is the inherited address **printed in decimal and re-parsed as hex** (MF-49). Also applies to axes, by axis name (MF-49) | `xml/TableScaleUnmarshaller.java:171-179`; `util/HexUtil.java:161-163` |
| `description` | text; a `<description>` child overrides it | `""` | inherited | `xml/TableScaleUnmarshaller.java:181-182,288-289` |
| `sizey`, else `sizex` | decimal int | inherited, 1 for a new table | The 1D/2D/switch cell count. Setting it allocates a fresh, empty cell array | `xml/TableScaleUnmarshaller.java:185-186`; `maps/Table.java:274-276` |
| `flipy`, else `flipx` | boolean | false | Used only by the 2D view, which shows the cells in reverse; no effect on a 1D | `xml/TableScaleUnmarshaller.java:187-188`; `maps/Table2DView.java:88-96` |
| `userlevel` | decimal int | **0** | Stored unclamped (**MF-37**); section 5 | `xml/TableScaleUnmarshaller.java:189-190`; `maps/Table.java:65,677-684` |
| `locked` | boolean | false | A locked table cannot be edited: edits are ignored | `xml/TableScaleUnmarshaller.java:191-192`; `maps/DataCell.java:481-484` |
| `logparam` | text | `""` | Logger parameter ID for the live-data overlay | `xml/TableScaleUnmarshaller.java:193-194`; `logger/ecu/ui/handler/table/TableUpdateHandler.java:83-84` |
| `mask` | hex via `parseUnsignedInt`; empty keeps the inherited mask | none | Clamped to the storage width at parse time (`2^(8·width) − 1`), so with width 0 it becomes 0, i.e. no mask | `xml/TableScaleUnmarshaller.java:195-196`; `maps/Table.java:537-548` |
| 3D only: `swapxy`, `flipx`, `flipy` | boolean | false | | `xml/TableScaleUnmarshaller.java:198-204` |
| 3D only: `sizex`, `sizey` | decimal int | 1 × 1 | Each allocates a fresh `[x][y]` array. The 3D's own cells are a separate array that hides the base class's (**MF-12**), which `sizey`/`sizex` above also resize | `xml/TableScaleUnmarshaller.java:205-208`; `maps/Table3D.java:36,95-109` |
| 3D only: `skipCells` | decimal int | 0 | Cells skipped after each stored row; case-sensitive camel case | `xml/TableScaleUnmarshaller.java:209-210`; `maps/Table3D.java:41-42,203-210` |

How swap, flip and skip place the 3D cells is in
`maps/Table3D.java:186-211` (for `encoding-scaling.md`).

### 4.5 Child elements

| Child | Effect | Source |
|-------|--------|--------|
| `<table>` in a **2D** table | If its `type` is an X or Y axis (`X Axis`, `Static X Axis`, `4`, `Y Axis`, `Static Y Axis`, `5`, any case): read into the 2D's one axis (4.6). A second axis child replaces the first's result. Any other type is ignored | `xml/TableScaleUnmarshaller.java:223-241`; `xml/RomAttributeParser.java:92-103` |
| `<table>` in a **3D** table | X types into the X axis, Y types into the Y axis; others ignored | `xml/TableScaleUnmarshaller.java:242-270` |
| `<table>` in any other kind | ignored | `xml/TableScaleUnmarshaller.java:220-271` |
| `<scaling>` | section 6 | `xml/TableScaleUnmarshaller.java:273-277` |
| `<data>` | On a `Table1D` (axes, 1D, switches): the text's first token fills the first empty cell, and the table becomes **static** (never read from the image). Extra `<data>` beyond the cell count is dropped. On a 2D/3D: logged as an error | `xml/TableScaleUnmarshaller.java:279-286`; `maps/Table.java:105-115`; `maps/DataCell.java:69-75` |
| `<description>` | text replaces the description | `xml/TableScaleUnmarshaller.java:288-289` |
| `<state name data>` | Switch state (4.7) | `xml/TableScaleUnmarshaller.java:291-295` |
| `<bit name position>` | Bitwise flag (4.8) | `xml/TableScaleUnmarshaller.java:296-299` |
| anything else | skipped | `xml/TableScaleUnmarshaller.java:301-304` |

### 4.6 Axes

- An axis child is read with `unmarshallTable` into the **existing** axis
  object (`Table2D.getAxis()`, `Table3D.getXAxis()/getYAxis()`), so it is
  never filtered and never created by type; it is matched to the parent's
  axis by its **type letter, not its name**. Its `type` is otherwise unused
  (`maps/Table2D.java:29`; `maps/Table3D.java:33-34`).
- All table attributes apply to the axis (address, storage, endian, scaling,
  `userlevel`, `logparam`, `mask` …), with the same inheritance quirks: the
  axis goes through the same `unmarshallTable`
  (`xml/TableScaleUnmarshaller.java:233,248,262`, then `:142-211`).
  An axis's `userlevel` is its own, not its parent's (MF-57).
- After reading, the axis is resized to the parent's count: the 2D's cell
  count, the 3D's `sizex` for X, `sizey` for Y (`xml/TableScaleUnmarshaller.java:235-240,250-256,264-269`).
  Resizing allocates a fresh array, so static `<data>` cells are lost when
  the axis's own size differs from the parent's (MF-58).
- An axis with `omit="true"` throws out of the parent: the **parent** table
  is removed (`xml/TableScaleUnmarshaller.java:82-85`;
  `xml/DOMRomUnmarshaller.java:177-181`; the port's `omit` removes only what
  it names, MF-07).
- **Static axes.** What makes an axis static is having `<data>`, not the
  word `Static` in its type: a `Static X Axis` with no `<data>` is read from
  the image at its address (default 0), and an `X Axis` with `<data>` is
  static (`maps/Table.java:105-115,150`).
- Axes are deep-copied with a table `base`, never shared: they are fields of
  the table object that `ObjectCloner.deepCopy` copies
  (`xml/TableScaleUnmarshaller.java:87-98`; `maps/Table2D.java:29`;
  `maps/Table3D.java:33-34`). EC-05 (one axis shared by several tables) has
  no legacy form.

### 4.7 Switch states

`<state name="on" data="01 02"/>` adds a named preset; a later state of the
same name (ignoring case) replaces it, including across levels
(`maps/Table.java:500-504`; `maps/PresetManager.java:120-134`). `data`
(default `"0"`) is trimmed and split on `,` if it has one, else on single
spaces; each token is unsigned hex, and for a little-endian table of width 2
or 4 its bytes are reversed (`maps/PresetManager.java:42-55,94-110`). A state
is "active" when cell `i` equals value `i` for every value; selecting it
writes the values (`maps/PresetManager.java:57-80`). The cell count comes
from `sizey`/`sizex`, not from the state data. States work on any table
kind: every view shows a preset panel when the table has presets
(`maps/TableView.java:595-597`).

A switch whose name contains `Checksum Fix` is the Subaru checksum switch:
populating it validates the ROM checksum and pops up the result, and its
cells are never written by the normal save path (**MF-19**,
`maps/TableSwitch.java:56-81`; `maps/DataCell.java:211`; `maps/Rom.java:441-490`).

### 4.8 Bitwise switch flags

On a `BitwiseSwitch`, `<bit name="x" position="3"/>` appends `position`
(decimal) to the table's bit list and adds a preset named `name` with value
`1` at the next cell (`maps/TableBitwiseSwitch.java:69-75`). At populate time
the table has one cell per bit, every cell at offset 0 of the table's one
storage word, each with mask `1 << position`
(`maps/TableBitwiseSwitch.java:33-61`; `util/ByteUtil.java:147-149`). A bit
without `position` is ignored. Bits are **appended**, never replaced, so a
child level re-listing bits duplicates them (MF-54). A position beyond
the storage width is clamped into a wrong mask silently
(`maps/DataCell.java:98-103`; MF-54).

A bitwise switch is a `TableSwitch` subclass, so the editor opens it with the
plain switch view (**MF-38**, `editor/ecu/ECUEditor.java:458-473`).

On any other kind, `<bit>` calls the base `setPresetValues(name, position)`,
which makes a preset whose value is the position parsed **as hex** (dead
form, section 9; `maps/Table.java:488-492`).

## 5. User levels and byte-order overrides

### 5.1 User levels

The levels and their menu labels
(`legacy/i18n/com/romraider/swing/ECUEditorMenuBar.properties:32-36`):

| Level | Menu label |
|-------|------------|
| 1 | Beginner |
| 2 | Intermediate |
| 3 | Advanced |
| 4 | Highest |
| 5 | Debug Mode |

The user's level is a setting, default 1, clamped to 1–5
(`Settings.java:185,484-492`; `xml/DOMSettingsUnmarshaller.java:140-141`). A
table's `userlevel` defaults to **0** and is stored as written, unclamped
(**MF-37**). What it does:

| Where | Rule | Source |
|-------|------|--------|
| Tree | Shown if `displayhightables` (default true) **or** user level ≥ table level | `maps/Rom.java:169`; `Settings.java:187` |
| Tree styling | Level 5: pink italic; above the user's level: italic | `swing/RomCellRenderer.java:170-177` |
| Edit | A cell edit throws `UserLevelException` (popup) if table level > user level | `maps/DataCell.java:486-487`; `maps/PresetManager.java:73-77` |
| Save | A cell is written only if table level ≤ user level **and** (table level < 5 **or** `savedebugtables`, default true) | `maps/DataCell.java:236`; `Settings.java:186` |

Consequences: level 0 (or negative, or absent) is visible and editable at
every level; a level above 5 is never editable or saved; a level-5 table is
saved only at user level 5 with `savedebugtables` on
(`maps/DataCell.java:236,486-487`; `Settings.java:484-492`). Axes have their
own level, default 0 (MF-57; `maps/Table.java:65`).

### 5.2 Default scaling preference

The setting `defaultscale` (default `Metric`) names the scaling category a
table shows first (`Settings.java:256`; `xml/DOMSettingsUnmarshaller.java:176-177`;
section 6.4).

### 5.3 `<memmodel endian>`

When the ROM's `<memmodel>` has an `endian` attribute, every table read
**after** that `<romid>` takes `little` if it says `little` (any case) and
**big for any other value**, ignoring the table's own `endian`
(`xml/TableScaleUnmarshaller.java:162-170`). The value is also stored in the
**static** `Table.memModelEndian`, shared by every open ROM, which the float
decoder consults (**MF-04**, `maps/Table.java:40,761-767`;
`xml/RomAttributeParser.java:252-286`). A later `<memmodel>` without
`endian` (for example in the matched ROM's `<romid>`) sets the override back
to none for the tables after it. Tables read before the `<romid>` element are
not affected. Base lookups leak it between ROMs (MF-52). The two endian
parsers disagree on unknown values (MF-60).

## 6. Scalings

### 6.1 `<scalingbase>`

Read once per load from the **root's** children, in document order, before
any `<rom>` (`xml/TableScaleUnmarshaller.java:65-77`;
`xml/DOMRomUnmarshaller.java:67-68`). Each is read like a `<scaling>` (6.2)
into a fresh `Scale` and registered by lower-cased name in a map of the
reader. A `<scalingbase>` inside a `<rom>` is ignored (unexpected element).
A `<scalingbase base="...">` finds only bases **earlier** in the file
(MF-51).

### 6.2 `<scaling>` attributes

`unmarshallScale(element, existing)` (`xml/TableScaleUnmarshaller.java:412-463`),
where `existing` is the table's scale of category `category` (default
`Default`), or a new default `Scale` (`maps/Table.java:207-214`):

1. `base` other than `none` and found (lower-cased) among registered scales:
   start from a deep copy of it (`xml/TableScaleUnmarshaller.java:415-426`).
   Not found: keep `existing` (MF-51).
2. Only when there is **no** `base`: `category` = attribute or `Default`. With
   a `base`, the `category` attribute is ignored and the base's category kept
   (`xml/TableScaleUnmarshaller.java:428-431`).
3. If the category is not `Raw Value` and the name is still the default
   `Raw Value`: name = `name` attribute, else `units`, else `none`
   (`xml/TableScaleUnmarshaller.java:436-440`).
4. `to_byte` is **always** set: the attribute, or empty, and empty means none
   (`xml/TableScaleUnmarshaller.java:442`; `maps/Scale.java:196-201`). So any
   `<scaling>` without `to_byte` erases an inherited or based one (MF-50).
5. Every attribute, by name **ignoring case**: `units`, `expression`,
   `format`, `max`, `min` (`Double.parseDouble`), `coarseincrement` or its
   alias `increment`, `fineincrement` (`xml/TableScaleUnmarshaller.java:445-458`).
   These raw values bypass the `NaN` rule, so `min="NaN"` is NaN. Any other
   attribute is ignored, including the `storagetype`, `endian` and
   `logparam` that the DTD of section 9 puts on its `<scale>`.
6. Register the result by lower-cased name in the same global map as the
   `<scalingbase>`s (**MF-16**, `xml/TableScaleUnmarshaller.java:461`): a
   later `base="psi"` can find a table-local scaling named by its units, and
   a table-local override of a base replaces the base for every later user.

### 6.3 Defaults

| Field | Default | Source |
|-------|---------|--------|
| category | `Raw Value` (a new `Scale`); `Default` once read from XML without a base | `maps/Scale.java:33`; `xml/TableScaleUnmarshaller.java:430` |
| name | `Raw Value` | `maps/Scale.java:34` |
| units | `raw value` | `maps/Scale.java:35` |
| expression | `x` | `maps/Scale.java:36` |
| to_byte | `x` for a new `Scale`; none after any XML read without one | `maps/Scale.java:37,196-201` |
| format | `#.##` | `maps/Scale.java:38` |
| coarse / fine increment | 2 / 1 | `maps/Scale.java:39-40` |
| min / max | 0 / 0 | `maps/Scale.java:41-42` |

### 6.4 Several scalings on one table

Scales are kept per category, compared ignoring case; adding one replaces the
scale of that category (`maps/Table.java:220-228`). The current scale is the
first added, then any added scale whose category is the `defaultscale`
setting, else any whose category is `Default` (`maps/Table.java:230-239`),
so a `Default` added after a preferred `Metric` takes over (MF-59). At
populate time a built-in `Raw Value` scale is added
(`maps/Table.java:169-170`).

### 6.5 Consistency check

At populate time, for every non-switch table and every scale with a `to_byte`
other than the pair `x`/`x`: evaluate `to_byte(expression(5))`; a difference
above 0.001 raises a "bad scaling" popup from model code (**MF-40**,
`maps/Table.java:554-562`; `maps/Scale.java:63-77`). The table still loads. A
scale with no `to_byte` is converted back with a binary-search approximation
(**MF-05**, `maps/Scale.java:79-140`). Expressions are JEP (**MF-25**).

## 7. Other `<rom>` children

- **`<checksum>`**: every attribute, name lower-cased, goes to
  `ChecksumFactory.getManager`, which loads the class
  `maps.checksum.Checksum<TYPE>` and calls `configure(attributes)`
  (`xml/DOMRomUnmarshaller.java:185-188,300-308`;
  `maps/checksum/ChecksumFactory.java:60-104`). With a `path` attribute it
  would load a class from the definition's directory (**MF-11**); at load
  time `Rom.getDefinitionPath()` is still null (it is set after parsing,
  `editor/ecu/OpenImageWorker.java:172-175`), so that branch is not reached
  from this path, but the loader exists. An unknown type pops up an error
  and adds a null manager. Checksums of every level are **added**, never
  replaced (MF-53). The last level's `type` is shown as the ROM's
  checksum name (`maps/RomID.java:280-286`). Algorithms: `checksums.md`.
- **`<Simulation name>`** (exact element name, any case): a dataflow
  simulation, `<inputs><input name logparam>`, `<dataflow><table reference
  input_x input_y output>` and `<action expression output>`,
  `<description>` (`xml/TableScaleUnmarshaller.java:347-410`). Editor
  feature, out of scope for the reader.

## 8. Error handling

| Input | Legacy | Source |
|-------|--------|--------|
| Definition file missing | Popup "missing or moved", next file | `editor/ecu/OpenImageWorker.java:223-233` |
| XML not well-formed | Popup with the parser message, next file | `editor/ecu/OpenImageWorker.java:238-244` |
| No match in any file | Ask the user to pick a file; then offer to force the first `<rom>` | `editor/ecu/OpenImageWorker.java:253-312` |
| Bad number in any `<romid>` scanned for the match | `NumberFormatException` out of the match: popup, the **whole file** is skipped (MF-55) | `xml/DOMRomUnmarshaller.java:118-119,229-231,269-270,279-281`; `editor/ecu/OpenImageWorker.java:242-244` |
| Unknown element or attribute | Ignored, no message | `xml/DOMRomUnmarshaller.java:193-196,286-289`; `xml/TableScaleUnmarshaller.java:301-304` |
| Missing or unknown table `type` | `XMLParseException` logged; that table skipped | `xml/TableScaleUnmarshaller.java:128-135`; `xml/DOMRomUnmarshaller.java:182-184` |
| Bad hex (`0X…`, `≥ 80000000`), bad decimal size, bad `storagetype`, bad `min`/`max`/increment, nameless top-level table | Exception out of `unmarshallRom`: popup, **no ROM opened** | `util/HexUtil.java:161-163`; `xml/DOMHelper.java:53-63`; `xml/RomAttributeParser.java:52-71`; `xml/TableScaleUnmarshaller.java:453-457`; `editor/ecu/OpenImageWorker.java:192-194` |
| Missing ROM `base` | `RomNotFoundException`: popup, no ROM | `xml/DOMRomUnmarshaller.java:213` |
| Base cycle | `StackOverflowError`: "Looped base" popup (MF-17) | `editor/ecu/OpenImageWorker.java:178-184` |
| Missing table `base` | Silent (MF-51) | `util/ObjectCloner.java:71-83` |
| Missing scaling `base` | Silent (MF-51) | `xml/TableScaleUnmarshaller.java:415-426` |
| Table past the end of the image | Popup "storage address … out of bounds" per table; table removed (EC-02) | `maps/Rom.java:254-263,312-318,330-332` |
| `NullPointerException` while populating a table | Popup "error loading table"; table removed (MF-23) | `maps/Rom.java:265-270,324-327` |
| `to_byte` disagreeing with `expression` | Popup per bad scale; table kept (MF-40) | `maps/Table.java:554-562` |
| Unknown checksum type | Popup; null manager; later `NullPointerException` (MF-53) | `maps/checksum/ChecksumFactory.java:90-103`; `maps/Rom.java:565-588` |
| Out of memory | "Out of memory" popup | `editor/ecu/OpenImageWorker.java:185-191` |

In test mode the per-table popups are replaced by a stack trace and a list of
faulty tables (`maps/Rom.java:272-288`).

## 9. Dead forms

Parsed but never used, or present in the source but unreachable:

| Form | Status | Source |
|------|--------|--------|
| `legacy/definitions/ecu_defs.dtd` | Describes a different schema (`<rom name idaddress idstring abstract>`, `<scales>/<scale>/<unit to_real>`, `<table3d>`, `<table2d>`, `<parameter>`, `<switch>`, `<switchgroup>`, `<axis>`, `<on>/<off>`) that no code reads. Not referenced from `src/` | `legacy/definitions/ecu_defs.dtd:3-106` |
| `<romid><ecuid>` | stored, never read by the editor (the logger's ECU ID is a different object) | `xml/DOMRomUnmarshaller.java:245-246`; `maps/RomID.java:164-170` |
| `<romid><flashmethod>`, `<memmodel>` text | stored, never read; only `memmodel`'s `endian` attribute matters | `xml/DOMRomUnmarshaller.java:272-278` |
| `<romid>` `make`, `market`, `model`, `submodel`, `transmission`, `year`, `caseid`, `author`, `version` | display only | `swing/RomPropertyPanel.java:48-68`; `swing/RomCellRenderer.java:77-110` |
| `<rom>` attributes other than `base` | ignored | `xml/DOMRomUnmarshaller.java:149-152` |
| `RomAttributeParser.parseTableType`, `parseScaleType` | never called | `xml/RomAttributeParser.java:83-90,105-122` |
| Axis types `4` and `5` | accepted for axes only ("is anybody even using this?"); as a table's own type they are unknown | `xml/RomAttributeParser.java:92-103`; `xml/TableScaleUnmarshaller.java:105-135` |
| `flipx`/`flipy` on a 1D or switch | parsed into `flip`, read only by the 2D view | `maps/Table.java:282-288`; `maps/Table2DView.java:88-96` |
| Scaling `min`, `max` | shown in the scales list only; edit limits come from the storage width | `swing/ScalesTableModel.java:70-72`; `maps/DataCell.java:105-149` |
| `Scale.isReady()` | never called | `maps/Scale.java:178-190` |
| `<bit>` on a non-bitwise table | becomes a preset whose value is the position read as hex | `maps/Table.java:488-492`; `maps/PresetManager.java:113-117` |
| The `NullPointerException` popup around a table `base` copy | unreachable: copying `null` returns `null` | `xml/TableScaleUnmarshaller.java:89-97`; `util/ObjectCloner.java:71-83` |
| `unmarshallXMLDefinition`'s `definition` argument | unused | `xml/DOMRomUnmarshaller.java:61-79` |
| `ChecksumFactory`'s `path` branch | unreachable during load (MF-11 still applies to the code) | `maps/checksum/ChecksumFactory.java:75-81`; `editor/ecu/OpenImageWorker.java:172-175` |
| `Table.TableType.SWITCH` marshalling code, commented `X_AXIS(4)`/`Y_AXIS(5)` | vestigial | `maps/Table.java:769-799` |

## 10. Mapping to the port

How the behaviour above lands in `rr-defs`. Pointers, not decisions; checked
against the reader's own docs (`crates/rr-defs/src/romraider.rs`,
`romraider/set.rs`, `romraider/table.rs`, `romraider/scaling.rs`,
`resolve/index.rs`), not re-derived.

- **Files and matching.** A `DefinitionSet` loads the user's files in order;
  the ROM-ID index (`resolve/index.rs`) refuses an image two definitions
  match instead of taking the first (EC-03) and does not re-parse files per
  image (MF-40). Matching rules of 2.4 (ASCII ignoring case, `0x` hex, past
  the end, `force`) are the index's contract; LEG-DEF-03 pins them.
- **`base` chains.** Resolved with rr-core's layer resolver: own file first,
  then other files in set order (MF-17); a cycle is an
  `INHERITANCE_CYCLE` Error and an unknown base an `UNKNOWN_BASE` Error, not
  a stack overflow or a bare exception. `obsolete` is not inherited, as in
  legacy.
- **Tables.** Merged by lower-cased name, field by field: every field
  inherits, signedness included (MF-06), `omit` removes one table (MF-07),
  `dataLayout` and `to_byte` inherit (MF-50 fixed), a table
  `base` is shared, not deep-copied (MF-18). Addresses are typed fields, so
  the decimal-as-hex re-parse (MF-49) and the axis/table name collision
  (MF-49) cannot happen; LEG-DEF-10 should pin that. Tables with no
  address in any layer are listed as unplaced rather than dropped silently.
- **Bits** are appended across layers as in legacy (`TableDoc::bits`);
  whether duplicates should be an error is open (MF-54).
- **Scalings.** `<scalingbase>` names are scoped per file, with table-local
  scalings not leaking into them (MF-16); the preferred-category choice is
  `select_scaling` (preferred, else `Default`, independent of order:
  MF-59); an unknown scaling base is an `UNKNOWN_SCALING` diagnostic
  (MF-51).
- **Errors.** Every unknown element or attribute is a Warning and every
  malformed value an Error at its `line:col`, scoped to the table it breaks
  where possible (MF-23); nothing pops up from the model (MF-40).
- **Not yet ported.** `<checksum>` and `<Simulation>` are read as
  unsupported (`DEF0006`); the checksum stacking question (MF-53)
  belongs to the ChecksumSpec work (MF-11). User levels are typed (`u8`,
  MF-37); the axis-level rule (MF-57) and the editor's visibility, edit
  and save rules of 5.1 are for the editor stories. The hardened XML reader
  (`crates/rr-defs/src/xml.rs`, EC-09) does not process XInclude: an
  `xi:include` is an ordinary element to it; cross-file `base` replaces it.
- **Encoding hand-off.** The memory-model endian becomes part of each
  table's Encoding, per definition, never static (MF-04, MF-52);
  storage widths without a decoder are load errors (MF-02).

## Ledgered issues

Problems first found while cataloguing this page, each since entered in
PROJECT.md's Must-Fix ledger (`MF-nn`) or matched to an existing entry or
edge case (US-RR-12-3). The `UL-DEF-nn` IDs are kept here only so the
tasks and fixtures that cite them still resolve; new text cites the ledger
ID.

| ID | Ledger | Problem | Source |
|----|--------|---------|--------|
| UL-DEF-01 | MF-49 | **Inherited address re-parsed as hex.** When a `<table>` (or axis) changes an existing table and its name is not in the address filter, the default for `storageaddress` is the inherited address converted with `String.valueOf` (decimal) and then parsed as hex: base `0x2000` (8192) becomes `0x8192`. Hits every axis override without its own `storageaddress` (axes are never in the filter), a child naming a table in different case (the filter is case-sensitive, the table map is not), and tables lost to MF-07 | `xml/TableScaleUnmarshaller.java:171-179,317-345`; `util/HexUtil.java:161-163` |
| UL-DEF-02 | MF-49 | **Axis takes a table's address.** The filter map holds top-level table names, but the address lookup also runs for axes by axis name, so an axis named like a top-level table (an axis `Engine Load` beside a table `Engine Load`) gets that table's address | `xml/TableScaleUnmarshaller.java:171-173,233,248,262` |
| UL-DEF-03 | MF-50 | **`to_byte` erased.** Every `<scaling>` sets `to_byte` from its attribute or to none, so a scaling with `base="..."` or one that only changes `units` on an inherited scaling loses the base's `to_byte` and falls back to the MF-05 approximation | `xml/TableScaleUnmarshaller.java:442`; `maps/Scale.java:196-201` |
| UL-DEF-04 | MF-50 | **`dataLayout` erased.** Reset to `DEFAULT` on every `<table>` without it, so a child changing a Bosch-subtract table decodes it with the wrong layout | `xml/TableScaleUnmarshaller.java:150`; `maps/Table.java:520-527` |
| UL-DEF-05 | MF-51 | **Scaling `base` not found is silent.** An unknown name, or a `<scalingbase>` naming one later in the file, keeps the starting scale: on a table with no scale of that category it is a new `Raw Value`-category scale whose own name is ignored, registered as `raw value`, then replaced by the built-in raw scale at populate (the scaling vanishes); on an inherited scale it silently acts as a plain override | `xml/TableScaleUnmarshaller.java:415-440,461`; `maps/Table.java:169-170,207-214` |
| UL-DEF-06 | MF-52 | **Memory-model endian leaks.** The override lives in the reader and is set as a side effect of reading any `<romid>`, including those `findRomNodeMatch` scans to find a base by name, so a base without `<memmodel>` takes the endian of the last earlier `<rom>` in the file that had one; it applies only to tables after the `<romid>` element; a `<memmodel>` without `endian` clears it. Sibling of MF-04 | `xml/DOMRomUnmarshaller.java:118-119,275-278`; `xml/TableScaleUnmarshaller.java:59-63,162-170` |
| UL-DEF-07 | MF-53 | **Checksums stack along the chain.** Each `<checksum>` of each level adds a manager, so a child cannot replace or remove its base's checksum and both run on save. An unknown checksum type adds a `null` manager, which `validateChecksum` dereferences before its null check, so the ROM fails to open after the popup | `xml/DOMRomUnmarshaller.java:185-188`; `maps/Rom.java:557-559,565-588`; `maps/checksum/ChecksumFactory.java:90-104` |
| UL-DEF-08 | MF-54 | **Bitwise bits.** `<bit>`s append to the inherited list with no duplicate check, so re-listing bits in a child doubles the cells; a position at or beyond the storage width is clamped into a wrong mask with no message | `maps/TableBitwiseSwitch.java:46-53,69-75`; `maps/DataCell.java:98-103` |
| UL-DEF-09 | MF-55 | **One bad `<romid>` hides a file.** Matching parses the `<romid>` of every `<rom>` up to the match; a malformed `internalidaddress`, `offset` or `filesize` anywhere before it throws and the whole file is skipped. `filesize` shorter than two characters throws `StringIndexOutOfBoundsException` | `xml/DOMRomUnmarshaller.java:104-131,229-231,269-270,279-281`; `xml/RomAttributeParser.java:238-250`; `editor/ecu/OpenImageWorker.java:238-244` |
| UL-DEF-10 | MF-51 | **Table `base` not found is silent.** Copying a missing table returns `null` without error, so the element is read as a new table: it replaces an inherited table of the same name if it has a `type` and an address, is logged and skipped if it has no `type`, and is dropped if unaddressed. The popup meant for this case is unreachable | `xml/TableScaleUnmarshaller.java:87-140`; `util/ObjectCloner.java:71-83` |
| UL-DEF-11 | MF-57 | **Axis user level is not the table's.** An axis's level is its own `userlevel` (default 0), and the edit and save checks use the cell's own table, so the axis cells of a level-4 table pass the edit check at level 1 (when only axis cells are selected) and are always saved | `xml/TableScaleUnmarshaller.java:189-190,233,248,262`; `maps/DataCell.java:236,486-487`; `maps/Table3D.java:447-457` |
| UL-DEF-12 | MF-58 | **Static axis data lost.** `<data>` fills only the axis's current cell count; when the axis's own `sizex`/`sizey` differs from the parent's, the parent resizes it with a fresh empty array, discarding every static cell while the axis stays marked static, so its cells stay null | `maps/Table.java:105-115,150,274-276`; `xml/TableScaleUnmarshaller.java:235-237,250-252,264-266` |
| UL-DEF-13 | MF-59 | **Scale choice depends on order.** A `Default`-category scaling added after the preferred (`defaultscale`) category takes over as the current scale | `maps/Table.java:230-239` |
| UL-DEF-14 | MF-55 | **Whole definition fails on one value.** A top-level `<table>` without `name` throws `NullPointerException` in the table lookup; hex with an upper-case `0X` or an address of `0x80000000` or more throws `NumberFormatException`; both abort the whole ROM load rather than the one table | `xml/DOMRomUnmarshaller.java:170`; `maps/Rom.java:236-237`; `util/HexUtil.java:161-163`; `editor/ecu/OpenImageWorker.java:192-194` |
| UL-DEF-15 | MF-56 | **No fall-back, wrong force target.** Once a `<rom>` matches, the search stops even if loading it fails, so a later file that would load is never tried; "load anyway" takes the file's first `<rom>`, usually the abstract base | `editor/ecu/OpenImageWorker.java:246-250,302-305`; `xml/DOMRomUnmarshaller.java:81-93` |
| UL-DEF-16 | MF-56, MF-73 | **Match and loaded ID disagree.** Matching uses only the `<rom>`'s own first `<romid>`: a child without `internalidaddress` is matched at address 0 though the loaded ROM inherits its base's address. The ASCII comparison decodes image bytes with the platform default charset | `xml/DOMRomUnmarshaller.java:112-131`; `maps/RomID.java:37,83-87` |
| UL-DEF-17 | MF-60 | **Inconsistent value parsers.** Table `endian` maps anything unrecognised to little, `<memmodel endian>` maps anything but `little` to big, so a typo flips byte order silently and differently; `storagetype` prefixes are case-sensitive (`UINT8` throws) while signedness lower-cases | `xml/RomAttributeParser.java:36-46,52-81`; `xml/TableScaleUnmarshaller.java:167` |

Ledger entries this page cross-references: **MF-02** (storage widths),
**MF-04** (static memory-model endian), **MF-05** (approximate inverse),
**MF-06** (signedness inheritance), **MF-07** (omit stops the filter),
**MF-11** (checksum class loading), **MF-12** (3D data field hidden),
**MF-16** (local scalings global), **MF-17** (same-file base, cycles),
**MF-18** (deep copy), **MF-19** (`Checksum Fix` by name), **MF-20**,
**MF-21** (conversion layers), **MF-23** (populate removes while iterating,
NPE as control flow), **MF-25** (JEP), **MF-37** (user level unclamped),
**MF-38** (bitwise switch view), **MF-40** (popups from the model, re-parse
per image); edge cases **EC-02**, **EC-03**, **EC-04**, **EC-05**, **EC-09**.
The table above adds **MF-49** to **MF-60**, and **MF-73** (platform
charset, shared with `csv.md` and `settings.md`).

## Fixtures to write

Stable IDs for US-RR-12-2. Each becomes a definition-reader KAT under
`fixtures/defs-kat/romraider/` (format: `fixtures/README.md`, "Definition-reader
fixtures"), or a test in `crates/rr-defs` where noted. Expected values are
written by hand from this page or transcribed as data from legacy's own test
files (D-024), never from running Java. Where a fixture pins a legacy bug,
the expected result is the port's fixed behaviour plus the diagnostic, and the
legacy result is recorded in its `notes`.

| ID | Kind | What it pins down | Section |
|----|------|-------------------|---------|
| LEG-DEF-01 | KAT `romid-elements.toml` | Every `<romid>` element of 2.1 read, element names in any case, text untrimmed; fields inherited from the base, the child's winning; `obsolete` not inherited; unknown elements warned | 2.1, 3.1 |
| LEG-DEF-02 | Test (rr-defs `value`) | `filesize`: `512kb`, `512KB`, `1mb`, `100b`, `1024`; Errors for `512 kb`, `8`, empty (MF-55) | 2.2 |
| LEG-DEF-03 | Test (ROM-ID index) | Matching: ASCII ignoring case, `0x` hex (odd length padded), ID ending exactly at the image end, past the end, `force` with `-1`, empty ID never matches, a base without ID never matches, a child without its own address (MF-56, MF-73) | 2.4 |
| LEG-DEF-04 | Test (ROM-ID index) | Two `<rom>`s matching one image, in one file and in two files: refused as ambiguous (EC-03; legacy took the first) | 1.1, 2.4 |
| LEG-DEF-05 | KAT `ram-offset.toml` + synthetic image | `filesize` = image size; image larger and smaller; `noramoffset`; `<offset>`; a `beforeram` table; no `filesize` at all (EC-02) | 2.3 |
| LEG-DEF-06 | KAT `inheritance-chain.toml` | Three-level chain: root read first, each level overriding single fields, the matched ROM winning; two `<rom>`s of one `xmlid` in a file (first wins, warning) | 3.1, 3.2 |
| LEG-DEF-07 | Test (rr-defs `set`) | `base` errors: unknown, self, `A → B → A` cycle, cross-file base (MF-17, EC-04) | 3.2, 3.4 |
| LEG-DEF-08 | KAT `omit.toml` | `omit` in the middle of the list (MF-07: later tables keep their addresses), omit-then-override in a grandchild (EC-04), omit of a table the chain lacks, `omit` on an axis | 4.1, 4.2, 4.6 |
| LEG-DEF-09 | KAT `table-base.toml` | Table `base` within a ROM and from an inherited table; changing the copy leaves the original; `base` naming a missing table (MF-51) | 4.2 |
| LEG-DEF-10 | KAT `address-inheritance.toml` | A child axis override without `storageaddress` keeps the base axis address (MF-49); an axis named like a top-level table keeps its own address (MF-49); a child naming a table in different case | 4.1, 4.4 |
| LEG-DEF-11 | KAT `storage-types.toml` | Every `storagetype` spelling of 4.4, a child flipping `uint8` to `int8` (MF-06), no `storagetype` anywhere (Error, MF-02), `UINT8` (MF-60) | 4.4 |
| LEG-DEF-12 | KAT `endian.toml` | Table `endian` `big`/`little`/`1`/`2`/garbage; `<memmodel endian>` override, a child `<memmodel>` without `endian`, a base without `<memmodel>` after another ROM that has one (MF-52, MF-60) | 4.4, 5.3 |
| LEG-DEF-13 | Test (two definitions) | A big-endian and a little-endian ROM resolved together decode independently (MF-04) | 5.3 |
| LEG-DEF-14 | KAT `table-types.toml` | Each `type` of 4.3, `X Axis` as a top-level type, missing type (warning, skipped), unknown type, a child changing an existing table's `type` (ignored) | 4.3 |
| LEG-DEF-15 | KAT `axes.toml` | 2D with an `X Axis` and with a `Y Axis` child; 3D axes; axis count forced to the parent's; a non-axis nested table ignored; static axis by `<data>` presence; static data with the axis size differing from the parent's (MF-58); extra `<data>` | 4.5, 4.6 |
| LEG-DEF-16 | KAT `table-3d-layout.toml` | `swapxy`, `flipx`, `flipy`, `skipCells`; `sizey` overridden in a child (from `legacy/src/test/definitions/scalingbase_test.xml:82`) | 4.4 |
| LEG-DEF-17 | KAT `switch-states.toml` | `<state>` data comma- and space-separated, 2-byte little-endian swap, a state replaced by name in a child, cell count from `sizey`; `Checksum Fix` name flagged, not special-cased by name (MF-19) | 4.7 |
| LEG-DEF-18 | KAT `bitwise-bits.toml` | Decimal positions, one cell per bit, bits re-listed in a child (MF-54), position ≥ width (Error), `<bit>` on a non-bitwise table (warning) | 4.8, 9 |
| LEG-DEF-19 | KAT `scalingbase.toml` | Legacy's own scaling test transcribed as data: the five `<scalingbase>`s of `legacy/src/test/definitions/scalingbase_test.xml:4-8` and the expected `Scale` values of `legacy/src/test/java/com/romraider/xml/TableScaleUnmarshallerTest.java:64-124` | 6.1, 6.3 |
| LEG-DEF-20 | KAT `scaling-inheritance.toml` | `base` keeps the base's category, ignoring `category`; name from `name`/`units`/`none`; `increment` alias; a `units`-only override keeps `to_byte` (MF-50); unknown and forward `base` (MF-51); a table-local scaling does not become a base (MF-16) | 6.2 |
| LEG-DEF-21 | Test (rr-defs `select_scaling`) | Current scale: preferred category, else `Default`, regardless of order (MF-59) | 6.4 |
| LEG-DEF-22 | KAT `table-attributes.toml` | `dataLayout` inherited (MF-50); `mask` inherited and empty `mask` ignored; `beforeram` set-only; `category` normalisation; `locked`, `logparam`, `description` attribute vs element | 4.4 |
| LEG-DEF-23 | KAT `user-levels.toml` + editor T0 test later | `userlevel` absent (0), 1–5, 7, −1 (MF-37); axis level follows the parent (MF-57); 5.1's visibility, edit and save rules as an editor test when the editor exists | 5.1 |
| LEG-DEF-24 | KAT `malformed.toml` | Nameless top-level table, `0X` hex, address `0x80000000`, bad `min`, bad size: each an Error at `line:col` with every other table still resolved (MF-23, MF-55) | 8 |
| LEG-DEF-25 | Test (checksum spec, later) | `<checksum>` in base and child: decided stacking or replacement (MF-53); unknown type an Error, no code loading (MF-11) | 7 |
| LEG-DEF-26 | Test (rr-defs `xml`) | `xi:include`, a DOCTYPE with entities, a CDATA `<xmlid>`: refused or read as the hardened reader documents (EC-09) | 1.2, 1.3 |
