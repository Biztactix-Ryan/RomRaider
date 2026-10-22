# ROM checksums: legacy algorithms and the save flow

What the Java RomRaider ROM editor does to keep a ROM image's checksums
right: how a checksum is chosen for a ROM, every algorithm it can run (the
`<checksum type=…>` managers for Nissan, Motronic, BMW and GM E38 ECUs, and
the Subaru "Checksum Fix" table), which bytes each one covers, where it
stores its result, how validation on open and recalculation on save work,
in what order checksums run and which of them depend on each other. Written
from the source only (the Java was never run, D-024). It is the spec behind
the checksum engine (US-RR-35) and the save pipeline (US-RR-45). Part of the
legacy behaviour catalogue, story US-RR-12, task US-RR-12-7.

How the `<checksum>` element is **parsed and inherited** is in
[`definitions.md`](definitions.md) (section 7, MF-53); how the
"Checksum Fix" switch table and its `<state>`s are read is in
`definitions.md` 4.7. Byte-level reads (`parseByteValue`) are in
[`encoding-scaling.md`](encoding-scaling.md) 1.2. The SSM frame checksum is
in [`ssm.md`](ssm.md) 2.2; the other frame checksums are listed in section 9
here only to place them. The arithmetic of every algorithm is pinned by the
existing reference vectors `fixtures/kat/checksum-*.toml` (US-RR-14-7),
whose coverage against the legacy classes is checked by
`crates/rr-testkit/tests/checksum_kat.rs` (`PLANNED`, `LEGACY_CLASSES`).
This page links to them and does not repeat their working.

## How to read this page

- **References.** Source references are relative to
  `legacy/src/main/java/com/romraider/`, the convention the PROJECT.md
  Must-Fix ledger uses: `maps/RomChecksum.java:110` is
  `legacy/src/main/java/com/romraider/maps/RomChecksum.java`, line 110.
  Message texts are in `legacy/i18n/…` and other files are written from the
  repository root (`legacy/...`, `fixtures/...`, `crates/...`). Every
  reference is `file:line` or `file:first-last`.
- **Addresses.** "Image offset" is a byte index into the ROM file. The
  `<checksum>` managers use their attributes as image offsets directly; the
  Subaru table subtracts the ROM's RAM offset (`definitions.md` 2.3).
  Ranges are written `[a, b)` (end excluded) or `[a, b]` (end included),
  exactly as each loop runs.
- **Bugs.** Where legacy behaviour is wrong or fragile, the page says so and
  cites the Must-Fix ledger entry (`MF-nn`) or edge case (`EC-nn`) of
  PROJECT.md. Do not port it as intended. A sentence that calls legacy
  behaviour a bug always carries its ledger ID (`cargo xtask
  check-legacy-docs`, rule 7). [Ledgered issues](#ledgered-issues) maps the
  problems first found on this page (`UL-CHK-nn`, IDs that tasks and
  fixtures still cite) to their ledger entries.
- **Numbers.** Bytes are hex without `0x`. Every worked example was computed
  by hand for this page, with the working shown; none came from running
  code. Java `int` arithmetic wraps mod 2^32 and `short` arithmetic mod
  2^16, which is how every sum below wraps.

### Source files

| Alias | Path under `legacy/src/main/java/com/romraider/` | Role |
|-------|---------------------------------------------------|------|
| `maps/checksum/ChecksumManager.java` | same | The interface: `configure`, `getNumberOfChecksums`, `validate`, `update` |
| `maps/checksum/ChecksumFactory.java` | same | `type` attribute to class, by reflection (MF-11) |
| `maps/checksum/NissanChecksum.java`, `ChecksumSTD.java`, `ChecksumALT.java`, `CalculateSTD.java`, `Calculator.java` | `maps/checksum/` | Nissan `std` and `alt`: 32-bit sum and XOR |
| `maps/checksum/ChecksumALT2.java`, `CalculateALT2.java` | `maps/checksum/` | Nissan `alt2`: the above plus two 16-bit sums |
| `maps/checksum/ChecksumMOTRONICSINGLE.java`, `ChecksumMOTRONICDOUBLE.java` | `maps/checksum/` | Bosch Motronic 16-bit byte sum over one or two ranges |
| `maps/checksum/ChecksumBYTEXOR.java` | `maps/checksum/` | BMW one-byte XOR |
| `maps/checksum/ChecksumCOPY.java` | `maps/checksum/` | Block copy ("copy checksum") |
| `maps/checksum/ChecksumE38PCM.java` | `maps/checksum/` | GM E38: six segments, each a 16-bit sum and a CRC-16 CVN |
| `maps/checksum/NcsCoDec.java` | `maps/checksum/` | Nissan NCS codec and block CRC, used only by a reflash test tool |
| `maps/RomChecksum.java` | same | The Subaru table checksum (`0x5AA5A55A`) |
| `maps/Rom.java` | same | Manager list, validate on open, `saveFile` (Subaru fix, edit stamp, managers) |
| `maps/TableSwitch.java`, `maps/DataCell.java`, `maps/Table1D.java` | `maps/` | "Checksum Fix" special cases by table name |
| `xml/DOMRomUnmarshaller.java` | same | `<checksum>` to manager, order of levels |
| `editor/ecu/OpenImageWorker.java`, `swing/ECUEditorMenuBar.java` | same | Open and save entry points |
| `util/HexUtil.java`, `xml/RomAttributeParser.java`, `Settings.java` | same | Hex attribute parsing, byte reads and writes, `CHECK_TOTAL` |

There are no legacy unit tests for any ROM checksum; the only checksum test
is `legacy/src/test/java/com/romraider/maps/checksum/NcsCoDecTest.java`
(already transcribed, section 9).

## 1. Two mechanisms

Legacy has two unrelated ways to keep checksums:

1. **Checksum managers**, one per `<checksum type="…" …/>` element of the
   ROM's definition chain (sections 2 to 7). Each is a `ChecksumManager`
   with `validate(image)` returning how many of its checksums are right
   and `update(image)` rewriting them
   (`maps/checksum/ChecksumManager.java:29-58`).
2. **The Subaru "Checksum Fix" table**: a table whose *name* is "Checksum
   Fix" holds a list of ranges and stored values inside the ROM image; it
   is validated when the table is populated and recalculated on save
   (section 8, **MF-19**).

A ROM can have both; the Subaru fix always runs before the managers on
save (section 10.2). The pinned definition corpus (D-028) has no
`<checksum>` element at all: its Subaru definitions rely entirely on the
"Checksum Fix" table (the bases define it as a `Switch` of `sizey` 168 or
204, i.e. 14 or 17 entries, and some bases add a second `Switch` named
"Checksum Fix 2" of `sizey` 24, two entries). Its
`<romid><checksummodule>` element is ECUFlash's and legacy never reads it
(`definitions.md` 2.1).

## 2. Selecting a manager

### 2.1 From `<checksum>` to a class

`<checksum>` is read in document order as a child of `<rom>`; the base
`<rom>` is unmarshalled first, so a chain's managers are listed root first,
then each child's in document order
(`xml/DOMRomUnmarshaller.java:149-154,156,185-188`). Every attribute, name
lower-cased, goes into a map passed to the factory
(`xml/DOMRomUnmarshaller.java:300-308`). Managers of every level are
**added**; a child cannot replace or remove its base's (MF-53). The
last `type` read is shown as the ROM's "checksum" property
(`xml/DOMRomUnmarshaller.java:186`; `maps/RomID.java:280-286`).

`ChecksumFactory.getManager` builds the class name
`com.romraider.maps.checksum.Checksum` + `type.toUpperCase()` and
instantiates it by reflection, then calls `configure(attrs)`
(`maps/checksum/ChecksumFactory.java:60-89`). With a `path` attribute it
would instead load `type` from a jar or directory next to the definition
with a `URLClassLoader` (**MF-11**, `maps/checksum/ChecksumFactory.java:73-81`;
unreachable at load because the definition path is set later,
`definitions.md` 7).

| `type` (any case) | Class | Checksums (`getNumberOfChecksums`) | Section |
|-------------------|-------|-------------------------------------|---------|
| `std` | `ChecksumSTD` | 2 | 3 |
| `alt` | `ChecksumALT` | 2 | 3 |
| `alt2` | `ChecksumALT2` | 4 | 4 |
| `motronicsingle` | `ChecksumMOTRONICSINGLE` | 1 | 5 |
| `motronicdouble` | `ChecksumMOTRONICDOUBLE` | 1 | 5 |
| `bytexor` | `ChecksumBYTEXOR` | 1 | 6.1 |
| `copy` | `ChecksumCOPY` | 1 | 6.2 |
| `e38pcm` | `ChecksumE38PCM` | 12 | 7 |

Sources: `maps/checksum/NissanChecksum.java:54-57`;
`maps/checksum/ChecksumALT2.java:52-55`;
`maps/checksum/ChecksumMOTRONICSINGLE.java:53-56`;
`maps/checksum/ChecksumMOTRONICDOUBLE.java:46-49`;
`maps/checksum/ChecksumBYTEXOR.java:49-52`;
`maps/checksum/ChecksumCOPY.java:47-50`;
`maps/checksum/ChecksumE38PCM.java:48-51`.

`type="manager"` and `type="factory"` name `ChecksumManager` and
`ChecksumFactory`, an interface and a class with a private constructor, so
they fail to instantiate like an unknown type (`maps/checksum/ChecksumManager.java:29`;
`maps/checksum/ChecksumFactory.java:48-49,84-88`).

### 2.2 Attribute values

Every address attribute is parsed by `HexUtil.hexToInt`:
`Integer.parseInt(value.replace("0x", ""), 16)` (`util/HexUtil.java:161-163`).
So `0x` is optional, an upper-case `0X` or a value of `80000000` or more
throws `NumberFormatException`, and a **missing** attribute throws
`NullPointerException` (`null.replace`; MF-55, and 2.3 for what follows,
MF-45). These are image offsets, never
shifted by the RAM offset (each manager indexes the image array directly).

### 2.3 Errors

Any exception while instantiating **or configuring** is caught: the factory
prints the stack trace and pops up "Error in checksum definition, 'type'
attribute missing." when there is no `type`, else "Error loading Checksum
Manager of type: TYPE" (`maps/checksum/ChecksumFactory.java:90-103`;
`legacy/i18n/com/romraider/maps/checksum/ChecksumFactory.properties:1-2`).
It then returns `cm` (`maps/checksum/ChecksumFactory.java:104`), which is:

- `null` when the class did not load: the null manager is added and
  `Rom.validateChecksum` calls `cm.validate` before its own null check, so
  opening the ROM ends in a `NullPointerException` (MF-53,
  `maps/Rom.java:570-573`);
- **the half-configured manager** when `configure` threw (a missing or
  malformed attribute): it is added and used. Fields not yet assigned keep
  their defaults: a Motronic manager with a bad `end` has `end` = `loc` = 0
  and on save writes its sum into image bytes 0–1; a `bytexor` without
  `xorloc` writes into byte 0; a Nissan manager without `end` throws
  `NullPointerException` on open (unboxing a missing map entry) (MF-45;
  `maps/checksum/ChecksumMOTRONICSINGLE.java:44-51`;
  `maps/checksum/ChecksumBYTEXOR.java:43-47`;
  `maps/checksum/NissanChecksum.java:46-52`).

`toUpperCase()` uses the JVM's default locale, so under a Turkish locale
`motronicsingle` becomes `MOTRONİCSİNGLE` and fails to load (MF-45;
`maps/checksum/ChecksumFactory.java:84`).

## 3. Nissan `std` and `alt`

`ChecksumALT` is **identical** to `ChecksumSTD`: both use `CalculateSTD`
(`maps/checksum/ChecksumSTD.java:26-30`; `maps/checksum/ChecksumALT.java:26-30`).
Whatever the name suggests, legacy's `alt` is an alias of `std`.

**Parameters.** `start`, `end`, `sumloc`, `xorloc`
(`maps/checksum/NissanChecksum.java:36-39,46-52`).

**Algorithm** (`maps/checksum/CalculateSTD.java:39-55`). For
`i = start, start+4, …` while `i < end`, skipping an `i` equal to `sumloc`
or `xorloc`, read the big-endian 32-bit word at `i` and accumulate
`sumt += word` (mod 2^32) and `xort ^= word`.

- The range is `[start, end)` in 4-byte steps from `start`. A storage
  location is skipped only if it falls exactly on that stride; a
  misaligned `sumloc` or `xorloc` is summed like data, so the checksum
  covers its own storage (EC-11; `maps/checksum/CalculateSTD.java:47-48`).
- A range whose length is not a multiple of 4 reads the last word across
  `end`; past the image it throws (`xml/RomAttributeParser.java:175-177`).
  The vectors' `pad = "zero"` convention in
  `fixtures/kat/checksum-sum-words.toml` is the port's proposal, not
  legacy's.

**Storage.** `sumt` as a big-endian 32-bit word at `sumloc`, `xort` at
`xorloc` (`maps/checksum/NissanChecksum.java:76-81`).

**Validate** compares each with the stored word read signed
(`maps/checksum/NissanChecksum.java:59-73`): 0, 1 or 2 correct.
**Update** always writes both and reports 2, changed or not
(`maps/checksum/NissanChecksum.java:76-81`). The two results do not depend
on each other while both locations are on the stride.

*Worked example (hand-computed).* `start=0`, `end=10`, `sumloc=8`,
`xorloc=C`; image bytes `11 22 33 44 01 02 03 04` at 0–7.

- Words: `i=0` → `11223344`; `i=4` → `01020304`; `i=8`, `i=C` skipped.
- `sumt` = `11223344 + 01020304`, byte by byte with no carries:
  `11+01=12`, `22+02=24`, `33+03=36`, `44+04=48` → `12243648`.
- `xort` = `11223344 ^ 01020304`: `11^01=10`, `22^02=20`, `33^03=30`,
  `44^04=40` → `10203040`.
- Stored: bytes 8–B `12 24 36 48`, bytes C–F `10 20 30 40`.

## 4. Nissan `alt2`

**Parameters.** As `std`, plus `skiploc`, default `0x20000` when absent
(`maps/checksum/ChecksumALT2.java:41-50`).

**Algorithm** (`maps/checksum/CalculateALT2.java:40-86`), three parts:

1. **32-bit** `sumt` and `xort` as in `std`, but starting at `start + 4`
   and also skipping `skiploc`: big-endian words at `start+4, start+8, …`
   while `< end`, except `sumloc`, `xorloc`, `skiploc`
   (`maps/checksum/CalculateALT2.java:45-58`).
2. **16-bit calibration sum** (`results[start]`): big-endian 16-bit words
   at `start+2, start+4, …` while `< skiploc`, accumulated in a Java
   `short` (mod 2^16). At `i == sumloc` the **computed** `sumt` is added as
   its high and low halves instead of the stored bytes, and `i` advances
   past the 4-byte word; the same for `xorloc` and `xort`
   (`maps/checksum/CalculateALT2.java:60-79`).
3. **16-bit code sum** (`results[skiploc]`): big-endian 16-bit words at
   `skiploc+2, …` while `< end`, from the image as it is
   (`maps/checksum/CalculateALT2.java:80-85`).

Coverage quirks that the port must reproduce exactly: bytes `start+2..+3`
are in the calibration sum but not in `sumt`/`xort` (part 1 starts at
`start+4`), and bytes `skiploc+2..+3` are in the code sum but not in
`sumt`/`xort` (part 1 skips the whole word at `skiploc`). With the default
`skiploc` of `0x20000` and `end` below it, part 2 runs to `0x20000`
regardless of `end` and part 3 is empty
(`maps/checksum/CalculateALT2.java:49-52,62,82`).

**Storage.** `sumt` and `xort` as in `std`; the calibration sum as a
big-endian 16-bit word at `start`, the code sum at `skiploc`
(`maps/checksum/ChecksumALT2.java:81-88`).

**Validate** counts up to 4, comparing the 16-bit results sign-extended
both sides (`maps/checksum/ChecksumALT2.java:57-79`). **Update** calls the
`std` update, which computes all four results once on the image as it is,
writes `sumt` and `xort`, then writes the two 16-bit sums from the same
results; it reports 4 (`maps/checksum/ChecksumALT2.java:81-88`).

**Ordering (EC-12).** The calibration sum depends on `sumt` and `xort`,
but legacy sidesteps the order by adding the computed values. The code sum
does not: if `sumloc` or `xorloc` lies at or after `skiploc + 2`, the code
sum is computed from the **stale** stored `sumt`/`xort` and the saved file
fails its own code-sum check until it is saved a second time (MF-46;
`maps/checksum/CalculateALT2.java:63-76,82-85`;
`maps/checksum/ChecksumALT2.java:84-86`). Neither 16-bit sum is inside
part 1, so part 1 does not depend on them.

*Worked example (hand-computed).* `start=0`, `end=20`, `sumloc=8`,
`xorloc=C`, `skiploc=10`. Data bytes: 02–03 `00 01`; 04–07 `00 02 00 03`;
12–13 `00 04`; 14–1F `00 05 00 06 00 07 00 08 00 09 00 0A`.

- Part 1 visits `i` = 4, 14, 18, 1C (8, C, 10 skipped): words `00020003`,
  `00050006`, `00070008`, `0009000A`.
  `sumt`: high halves 2+5+7+9 = 23 = `17`, low halves 3+6+8+10 = 27 = `1B`,
  no carry out of the low half → `0017001B`.
  `xort`: high 2^5 = 7, 7^7 = 0, 0^9 = 9; low 3^6 = 5, 5^8 = D, D^A = 7 →
  `00090007`.
- Part 2, `i` from 2 below 10: `0001` (1), `0002` (3), `0003` (6); at
  `i=8` add `0017` (29) and `001B` (56); at `i=C` add `0009` (65) and
  `0007` (72) → 72 = `0048`.
- Part 3, `i` from 12 below 20: 4+5+6+7+8+9+10 = 49 = `0031`.
- Stored: 00–01 `00 48`; 08–0B `00 17 00 1B`; 0C–0F `00 09 00 07`;
  10–11 `00 31`.

## 5. Motronic single and double

**Parameters.** `start`, `end`, `loc`, optional `initial` (hex, cast to a
16-bit `short`, default 0); `motronicdouble` adds `startsecond` and
`endsecond` (`maps/checksum/ChecksumMOTRONICSINGLE.java:33-51`;
`maps/checksum/ChecksumMOTRONICDOUBLE.java:33-44`).

**Algorithm.** `value = initial`; for every byte in `[start, end]` (end
**included**) add the unsigned byte; the `short` wraps mod 2^16
(`maps/checksum/ChecksumMOTRONICSINGLE.java:86-93`). The double variant
continues with the first result as the initial value over
`[startsecond, endsecond]` (`maps/checksum/ChecksumMOTRONICDOUBLE.java:52-54`).
`loc` is **not** excluded: if it lies inside a range the checksum covers its
own storage and cannot settle (EC-11).

**Storage.** Big-endian 16-bit word at `loc`
(`maps/checksum/ChecksumMOTRONICSINGLE.java:79-80`;
`maps/checksum/ChecksumMOTRONICDOUBLE.java:76-77`). **Validate** returns 1
or 0. **Update** writes only when the stored value differs and returns 1 if
it wrote (`maps/checksum/ChecksumMOTRONICSINGLE.java:58-84`;
`maps/checksum/ChecksumMOTRONICDOUBLE.java:51-81`). Both variants count as
**one** checksum.

*Worked example (hand-computed).* `start=0`, `end=3`, `loc=4`,
`initial=1234`; bytes `01 02 03 FF`.

- Bytes: 1 + 2 + 3 + 255 = 261 = `0105`.
- `1234 + 0105` = `1339` → bytes 4–5 `13 39`.
- Double, with `startsecond=6`, `endsecond=7` and bytes 6–7 `80 80`:
  `1339 + 80 + 80` = `1339 + 0100` = `1439` → `14 39`.
- Wrap: `initial=FFF0` over bytes `FF FF`: `FFF0 + 01FE` = `101EE` →
  `01EE` (as Java `short`s: −16 + 510 = 494 = `01EE`).

## 6. BMW byte XOR and the copy "checksum"

### 6.1 `bytexor`

**Parameters.** `start`, `end`, `xorloc`
(`maps/checksum/ChecksumBYTEXOR.java:34-47`). **Algorithm.** XOR of every
byte in `[start, end)` except `xorloc` (`maps/checksum/ChecksumBYTEXOR.java:77-85`).
**Storage.** One byte at `xorloc`. **Validate** 1 or 0; **update** always
writes and returns 1 only if the byte changed
(`maps/checksum/ChecksumBYTEXOR.java:54-75`).

*Worked example (hand-computed).* `start=0`, `end=4`, `xorloc=3`; bytes
`12 34 56 ??`. `12 ^ 34`: `0001 0010 ^ 0011 0100 = 0010 0110` = `26`;
`26 ^ 56`: `0010 0110 ^ 0101 0110 = 0111 0000` = `70`. Byte 3 becomes `70`.

### 6.2 `copy`

**Parameters.** `start`, `end` (included), `loc`
(`maps/checksum/ChecksumCOPY.java:31-45`; the `start >= end` guard is
always true on a fresh instance, so it always configures).
**Validate** returns 1 if `[start, end]` equals the same number of bytes at
`loc`, else 0 (`maps/checksum/ChecksumCOPY.java:52-60`). **Update** copies
`[start, end]` to `loc` when they differ (`System.arraycopy`, which handles
overlap as if through a temporary) and returns 1 if it copied
(`maps/checksum/ChecksumCOPY.java:62-70`). An edit made to the copy at
`loc` is replaced from the source on every save, which is what the
algorithm is for. No reference
value: it is US-RR-35's `Copy`, out of scope for the arithmetic vectors
(`crates/rr-testkit/tests/checksum_kat.rs`, `LEGACY_CLASSES`).

## 7. GM E38 (`e38pcm`)

Ported from the external E38-Sum-Tool (`maps/checksum/ChecksumE38PCM.java:27-30`).
It reads **no** attributes (`maps/checksum/ChecksumE38PCM.java:44-46`):
everything comes from fixed offsets in the image.

### 7.1 Segment table

Six segments, numbered 1–6 (`seg[0]` is unused). Each start and end is a
big-endian unsigned 32-bit word read at `10000` plus an offset, then cast
to `int` (`maps/checksum/ChecksumE38PCM.java:99-117`):

| Segment | start at | end at |
|---------|----------|--------|
| 1 | `10024` | `10028` |
| 2 | `10048` | `1004C` |
| 3 | `1006B` | `1006F` |
| 4 | `1008E` | `10092` |
| 5 | `100B1` | `100B5` |
| 6 | `100D4` | `100D8` |

The stride is `24` for the first step and `23` after it; that is legacy's
table as written (`maps/checksum/ChecksumE38PCM.java:106-117`), unconfirmed
against a real E38 image (none is in the corpus; LEG-CHK-12). An image
shorter than `100DC` bytes throws here.

**Checks** (`maps/checksum/ChecksumE38PCM.java:119-142`), for each segment
in order: start ≤ `200000`, end ≤ `200000`, start even, end odd,
`end − start ≥ 24`. The first failure **returns from `calculate`
immediately**, before any segment is loaded, so every segment is left
"not calculated" with all four values zero. A start or end word of
`80000000` or more is negative after the cast and passes the `≤ 200000`
tests; with an even negative start the read throws (MF-01).

### 7.2 Sum and CVN of one segment `[s, e]`

Loaded per segment when the checks pass (`maps/checksum/ChecksumE38PCM.java:144-151`):

- **Stored sum** `lsum`: big-endian 16 bits at `s`.
- **Stored CVN** `lcvn`: big-endian 16 bits at `s + 1E`.
- **Computed sum** `csum` (`maps/checksum/ChecksumE38PCM.java:213-221`):
  the big-endian 16-bit words at `s+2, s+4, …, e−1` (`i ≤ e` with `e` odd,
  so the last word is `e−1..e`), summed, then two's-complemented:
  `((sum & FFFF) ^ FFFF) + 1`, masked to 16 bits. It **includes the CVN
  word** at `s+1E` and excludes only the sum's own word at `s`.
- **Computed CVN** `ccvn` (`maps/checksum/ChecksumE38PCM.java:154-197,223-228`):
  the reflected CRC-16 of poly `8005` (the table at
  `maps/checksum/ChecksumE38PCM.java:162-181`, entry 1 = `C0C1`), register
  0, no final XOR, i.e. CRC-16/ARC, over `[s+2, s+1D]` and then, continuing
  from that register, over `[s+20, e]`. That is CRC-16/ARC of the
  concatenation, skipping both the sum (`s..s+1`) and the CVN
  (`s+1E..s+1F`). The result is byte-swapped (`swapab`,
  `maps/checksum/ChecksumE38PCM.java:199-211`) and stored big-endian, so
  the CRC lands **little-endian** at `s+1E`.

The step per byte is `crc = table[(crc ^ byte) & FF] ^ (crc >> 8)`
(`maps/checksum/ChecksumE38PCM.java:186-192`).

*Worked examples (hand-computed).*

- Sum: words `s+2..e` totalling `10234` → low 16 bits `0234` →
  `0234 ^ FFFF` = `FDCB` → `+1` = `FDCC`, stored `FD CC` at `s`. Check:
  `0234 + FDCC` = `10000` ≡ 0 mod 2^16, i.e. a correct segment's words
  `s..e` sum to zero.
- CRC step: register 0, byte `01`: index `(0000 ^ 01) & FF` = 1, `table[1]`
  = `C0C1`, `^ (0000 >> 8)` = `C0C1`. `swapab(C0C1)` = `C1C0`, stored
  `C1 C0`. For the catalogue check input `"123456789"`, CRC-16/ARC is
  `BB3D`, stored `3D BB` (`fixtures/kat/checksum-crc.toml`, the
  little-endian ARC case).

### 7.3 Validate and update

**Validate** (`maps/checksum/ChecksumE38PCM.java:53-69`) counts, for each
calculated segment, CVN correct and sum correct: 0–12. A failed segment
table gives 0 of 12, which only shows as the generic "checksum invalid"
popup on open (section 10.1).

**Update** (`maps/checksum/ChecksumE38PCM.java:71-97`) runs **twice**
("Seems like some checksums depend on each other? Need to do it twice").
Each pass recomputes all segments from the image, then for each segment
writes the CVN if it differs and the sum if it differs, counting each
write. Why two passes: the sum covers the CVN word (7.2), so pass 1 writes
a sum computed over the **old** CVN; pass 2 recomputes the sum over the
new CVN and rewrites it. The CVN does not cover the sum, so two passes
settle one segment; a segment whose range covers another segment's sum or
CVN could need more, and nothing checks.

- **Count.** Corrections are counted per pass, so a segment with stale CVN
  and sum reports 3 (CVN and sum in pass 1, sum again in pass 2) and the
  status bar can say "18 of 12 Checksums updated" (**MF-01**;
  `maps/checksum/ChecksumE38PCM.java:77-95`; `maps/Rom.java:590-598`;
  `legacy/i18n/com/romraider/maps/Rom.properties:7`).
- **Failed segment table.** `update` never looks at `calculated`
  (**MF-01**; `maps/checksum/ChecksumE38PCM.java:80-93`). Reading the
  code, though, a failed table makes every value zero
  (`maps/checksum/ChecksumE38PCM.java:102-104,119-142`), so
  `ccvn != lcvn` and `lsum != csum` are both false and update
  **writes nothing** and reports 0: the ROM is saved with whatever sums it
  had, silently (**MF-01**, whose text was corrected to this by
  US-RR-12-3; it had said "writes zeroed sums").
- **Bounds.** A segment that passes the checks but reaches past the image
  (a 1 MiB image with `end` near `200000`, or a negative start) throws out
  of `validate` on open, which aborts the open with an exception popup
  (`editor/ecu/OpenImageWorker.java:171-176,192-194`), and out of `update`
  on save, which aborts the save (section 10.2; MF-01).

## 8. The Subaru "Checksum Fix" table

### 8.1 Finding it (MF-19)

There is no checksum element: the checksum is triggered by a **table
name**, and four places test the name in three different ways (MF-19):

| Where | Test | Effect | Source |
|-------|------|--------|--------|
| `TableSwitch.populateTable` | `getName().contains("Checksum Fix")`, case-sensitive, `Switch` tables only | validate on open (8.4) | `maps/TableSwitch.java:56` |
| `DataCell.setBinValue`, `saveBinValueInFile` | `contains("Checksum Fix")`, any table type | the cells can never be edited or written | `maps/DataCell.java:211,482` |
| `Rom.populateTables` | `equalsIgnoreCase("Checksum Fix")` | read the edit stamp (8.6) | `maps/Rom.java:308-310` |
| `Rom.saveFile` | lower-cased key `startsWith("checksum fix")`, any table type | recalculate on save (8.5), write the stamp | `maps/Rom.java:443-449` |

So "Checksum Fix 2" (which the pinned corpus uses, section 1) is validated
and recalculated, and its presence means no stamp is written at all (8.6);
"checksum fix" (lower case) is recalculated but never validated and its
cells are editable; "Main Checksum Fix" is validated but never
recalculated (MF-19).

The table's own definition gives only where the entry list is
(`storageaddress`, minus the RAM offset) and how long it is (`sizey` =
`dataSize`, one byte per cell, `maps/TableSwitch.java:37-40`). The ranges
themselves are **read from the ROM image**, so the image decides which
bytes are summed (8.7).

### 8.2 Entry layout

`dataSize / 12` entries of 12 bytes, all big-endian 32-bit
(`maps/RomChecksum.java:49-64,66-99`):

| Bytes | Field | Meaning |
|-------|-------|---------|
| 0–3 | `start` | first address, in definition address space |
| 4–7 | `end` | end address, **excluded** |
| 8–11 | `diff` | stored value: `5AA5A55A − sum` |

Both addresses have the table's RAM offset subtracted, except that an entry
with `start = end = 0` keeps them at 0 ("0 means checksum is disabled, keep
it", `maps/RomChecksum.java:54-58,75-81`).

### 8.3 Algorithm

`sum` = the 32-bit sum (mod 2^32) of the big-endian words at
`start, start+4, …` while `< end` (`maps/RomChecksum.java:101-116`). The
entry is correct when `CHECK_TOTAL − diff − sum = 0`, i.e. `sum + diff =
5AA5A55A` (`Settings.java:131`). Recalculation stores `diff = 5AA5A55A −
sum`. A disabled entry (`0`, `0`) sums nothing, so its correct `diff` is
`5AA5A55A` itself. The vectors are in `fixtures/kat/checksum-sum-words.toml`
(the `target=0x5AA5A55A` cases).

*Worked example (hand-computed).* RAM offset 0; two entries.

- Entry 0: `start=00000100`, `end=00000108`; image words at 100 and 104:
  `00010002`, `00030004`. `sum` = `00040006` (no carries). `diff` =
  `5AA5A55A − 00040006`: low half `A55A − 0006 = A554`, high half
  `5AA5 − 0004 = 5AA1` → `5AA1A554`. Check: `5AA5A55A − 5AA1A554` =
  `00040006`, minus `sum` = 0.
- Entry 1: `00000000 00000000 5AA5A55A`, disabled: `5AA5A55A − 5AA5A55A −
  0` = 0.
- Wrap: words `FFFFFFFF`, `00000002` sum to `1 00000001` → `00000001`, so
  `diff` = `5AA5A559`.

### 8.4 Validate on open

When a `Switch` table whose name contains "Checksum Fix" is populated,
`validateRomChecksum` runs over the entries in order
(`maps/TableSwitch.java:49-81`; `maps/RomChecksum.java:66-99`):

- If **entry 0** is disabled (`start = end = 0` after the offset rule) with
  `diff = 5AA5A55A`, it returns −1 at once: "all checksums disabled",
  without looking at the other entries.
- Otherwise it returns the 1-based position of the first entry that is not
  correct, or 0 if all are.

Then: > 0 pops up "Checksum No. {n} is invalid in table: {name} … The
checksum can be corrected when the ROM is saved." (warning); −1 pops up
"All Checksums are disabled." (information); 0 sets `locked = false`
(`maps/TableSwitch.java:59-79`;
`legacy/i18n/com/romraider/maps/TableSwitch.properties:1-4`). In the first
two cases `locked` keeps its definition value, which is `false` unless the
definition says `locked="true"` (`maps/Table.java:66`;
`xml/TableScaleUnmarshaller.java:191-192`). The pinned corpus never sets
it.

### 8.5 The "fix" UI and save

**The switch does nothing.** The table's `<state>`s become preset check
boxes, and choosing one calls `setBinValue` on each cell, which returns at
once for a "Checksum Fix" table (`maps/PresetPanel.java:164-168`;
`maps/PresetManager.java:70-80`; `maps/DataCell.java:481-484`). A check box
shows as ticked when the cells match its state (`maps/PresetManager.java:57-67`).
The pinned corpus's base definitions say in the table description that the
checksum is fixed on save regardless of the check box, which is what the
code does.

**On save** (`maps/Rom.java:441-490`), for every table whose lower-cased
name starts with "checksum fix", in the hash map's iteration order:

- **not locked** (the normal case): `calculateRomChecksum` rewrites every
  entry's `diff`, disabled entries included (they get `5AA5A55A`, so a
  disabled entry with any other `diff` is changed), in entry order
  (`maps/RomChecksum.java:49-64`). This happens even when the open-time
  validation failed and the popup warned the image may be corrupt: there
  is no confirmation (MF-46);
- **locked**: it would ask "One or more ROM image Checksums is invalid.
  Calculate new Checksums?" and recalculate on Yes
  (`maps/Rom.java:417-437`; `legacy/i18n/com/romraider/maps/Rom.properties:8-9`),
  but only if `isButtonSelected()` is false, and for every `Table1D`
  (`TableSwitch` included) it is always true (`maps/Table1D.java:79-82`). So
  a locked table is **never** recalculated and the ROM is saved with the
  failing checksums, silently (MF-46).

**Ordering within the table.** Entries are rewritten in order on an image
that is being changed. If entry k's range covers entry j's `diff` (or the
edit stamp), entry k must be computed after j; legacy computes k first when
k < j, leaving k stale (EC-11, EC-12; `maps/RomChecksum.java:51-63`).
Nothing re-validates after the fix (`maps/Rom.java:474-489`).

### 8.6 The edit stamp (MF-19)

With exactly one "checksum fix" table, `saveFile` first stamps the image at
table offset `+204..+207` (decimal), **before** the checksums are computed,
so a stamp inside a summed range is covered (`maps/Rom.java:451-472`):

- `count` = byte `+207`; `FF` → `01`, else `count + 1` (a Java `byte`, so
  `7F` → `80` and `FE` → `FF`, and `FF` → `01` on the next save).
- The stamp is the date as `yyMMdd` followed by `count` as two hex digits,
  parsed as 4 hex bytes: the date's decimal digits become the bytes.
- On open (and after the stamp) `setEditStamp` reads `+204..+207` back for
  the ROM properties: `FFFFFFFF` → no stamp; otherwise `20yy-MM-dd vN`
  with `N` the last byte in decimal (`maps/Rom.java:335-351`).

*Worked example (hand-computed).* Saved on 2026-10-22, byte `+207` was
`01`: `count` = `02`, stamp text `"26102202"`, bytes `26 10 22 02` at
`+204`. Read back: hex `"26102202"`; replace from index 6 with `"2"` →
`"2610222"`; insert `" v"` at 6 → `"261022 v2"`; `"-"` at 4 →
`"2610-22 v2"`; `"-"` at 2 → `"26-10-22 v2"`; `"20"` at 0 →
`"2026-10-22 v2"`.

The offsets are fixed: for a 204-byte table (17 entries) the stamp is the
4 bytes right after the table; for a 168-byte table (14 entries) it is 36
bytes past its end (`maps/Rom.java:453-455,469`). Nothing in the
definition says the bytes there are a stamp (**MF-19**).

### 8.7 Error paths

- **Entry count.** If `dataSize` is not a multiple of 12, the validate loop
  runs one entry more than its `results` array holds and throws
  `ArrayIndexOutOfBoundsException` (`maps/RomChecksum.java:69-71,89`);
  `populateTables` catches it, pops up the out-of-bounds message and
  **removes the table**, so the save never fixes the Subaru checksum
  (`maps/Rom.java:312-318,330-332`; MF-47).
- **Image-controlled ranges.** A `start` or `end` outside the image makes
  the word read throw (`xml/RomAttributeParser.java:175-177`), with the
  same result on open; `end ≤ start` sums nothing. On save the exception
  aborts `saveFile` part-way (section 10.2).
- **Edit stamp** past the image end throws on open (`maps/Rom.java:337`),
  and the table is removed the same way (MF-47).

## 9. Frame and block checksums outside the ROM editor

Placed here only so every legacy checksum class is accounted for
(`crates/rr-testkit/tests/checksum_kat.rs`, `LEGACY_CLASSES`). None of them
touches a ROM image:

| Checksum | Algorithm | Source | Spec |
|----------|-----------|--------|------|
| SSM frame | sum of every byte but the last, low 8 bits | `io/protocol/ssm/iso9141/SSMChecksumCalculator.java:31-38` | `ssm.md` 2.2 |
| NCS (ISO 14230) frame | the same, `& FF` | `io/protocol/ncs/iso14230/NCSChecksumCalculator.java:31-38` | `logger.md` (US-RR-12-9) |
| DS2 frame | XOR of every byte but the last | `io/protocol/ds2/iso9141/DS2ChecksumCalculator.java:29-35` | `logger.md` |
| Ecotrons ALM frame | sum of every byte but the last, low 8 bits | `logger/external/ecotrons/io/AlmChecksumCalculator.java:31-37` | `logger.md` |
| NCS block CRC | CRC-16/MCRF4XX (register `FFFF`, reflected poly `8408`, no final XOR); the caller complements it and stores it low byte first | `maps/checksum/NcsCoDec.java:118-134`; `io/j2534/api/TestJ2534NCS.java:573-581` | `fixtures/kat/ncs-codec-legacy.toml` |
| MD5 of a file | `MessageDigest` | `util/MD5Checksum.java:28-50` | no caller in `legacy/src/main` |

`NcsCoDec`'s only caller is the J2534 NCS reflash test tool
(`io/j2534/api/TestJ2534NCS.java:524`). `BMWCodingConversionLayer` stores a
`guessChecksums` flag and never reads it
(`xml/ConversionLayer/BMWCodingConversionLayer.java:59,107-109`).

## 10. Open and save

### 10.1 On open

1. Tables are populated, which validates every "Checksum Fix" switch
   (8.4) and reads the edit stamp (8.6), each with its own popups
   (`editor/ecu/OpenImageWorker.java:77`; `maps/Rom.java:290-333`).
2. If the ROM has managers, `Rom.validateChecksum` calls each one's
   `validate` in list order (2.1). If any returns fewer than its
   `getNumberOfChecksums`, one popup: "Checksum is invalid. The ROM image
   may be corrupt or it has been hex edited manually. The checksum can be
   corrected when the ROM is saved if you trust it is not corrupt."
   (`maps/Rom.java:565-588`;
   `legacy/i18n/com/romraider/maps/Rom.properties:10-11`).
3. The status bar shows "N/M Checksums are correct. Checksums will be
   updated when file is saved." (`editor/ecu/OpenImageWorker.java:87-96`;
   `legacy/i18n/com/romraider/editor/ecu/ECUEditor.properties:14`). The
   Subaru table is not in N or M.

The ROM opens either way; editing is not restricted by a failing checksum
(`maps/Rom.java:580-587`; `editor/ecu/OpenImageWorker.java:94-98`).

### 10.2 On save

**Save As** asks for a file and confirms overwriting an existing one;
**Save** writes back to the file the ROM came from with no question
(`swing/ECUEditorMenuBar.java:347-360,501-547`;
`swing/ECUEditorMenuBar.java:70-71` for the menu labels). Both call
`Rom.saveFile` and then write the bytes it returns
(`swing/ECUEditorMenuBar.java:509-511`). `saveFile`, on the **open image
itself** (`maps/Rom.java:441-490`):

1. writes the edit stamp (8.6), if there is exactly one "checksum fix"
   table;
2. fixes every unlocked "checksum fix" table (8.5);
3. calls `update` on every manager in list order (`maps/Rom.java:590-601`);
   the status bar shows "U of M Checksums updated."
   (`legacy/i18n/com/romraider/maps/Rom.properties:7`), where U is the sum of
   the managers' own counts: Nissan always reports all, Motronic, `bytexor`
   and `copy` report what changed, E38 counts per pass (7.3).

There is **no validation after the update**, no refusal and no
confirmation for a failing checksum: every save silently writes whatever
the updates produced (MF-46). An exception in any step propagates to
the menu handler, which shows an exception dialog and writes nothing, but
the steps already done stay applied to the open image
(`swing/ECUEditorMenuBar.java:350-353`; MF-48). The write itself is a
plain `FileOutputStream` over the target, with no temporary file and no
`.bak` (`swing/ECUEditorMenuBar.java:549-560`; EC-13).

"Increase/Decrease ROM size" (16-bit Subaru images, `28000` ↔ `30000`
bytes, `Settings.java:132-137`) also call `saveFile` and then move the
`8000`-byte block between `20000` and `28000`; the checksums are those of the image
**before** the move, and nothing recomputes them for the new layout
(`swing/ECUEditorMenuBar.java:624-664`; MF-48).

### 10.3 Order and dependencies, summarised

| Order | What | Depends on | Section |
|-------|------|------------|---------|
| 1 | Edit stamp | — | 8.6 |
| 2 | Each unlocked "checksum fix" table, entries in order | the stamp, if a range covers it; earlier entries' `diff`s, if a range covers them (EC-11) | 8.5 |
| 3 | Managers, root level first, document order | anything an earlier step or manager wrote inside its range | 10.2 |
| within `alt2` | `sumt`, `xort` → calibration sum (computed values) and code sum (**stored** values, MF-46) | | 4 |
| within `e38pcm` | CVN → sum, settled by the second pass | | 7.3 |

No step re-validates an earlier one, so a manager that writes inside a
Subaru range, or a later manager that writes inside an earlier manager's
range, leaves the earlier checksum wrong in the saved file (EC-12,
MF-46; `maps/Rom.java:441-490`).

## 11. Mapping to the port

How the behaviour above lands in the port. Pointers, not decisions; checked
against the stories (US-RR-35, US-RR-45) and the code that exists today,
not re-derived.

- **No reflection, no name triggers.** US-RR-35's `ChecksumSpec { algo,
  ranges, excludes, store, order }` with a compiled-in named registry
  replaces both `ChecksumFactory` (**MF-11**) and the "Checksum Fix" name
  tests (**MF-19**). An unknown algorithm is an Error at load (LEG-DEF-25);
  a missing or malformed parameter must be one too, never a half-configured
  checksum (MF-45). Today rr-defs reads `<checksum>` as unsupported
  (`DEF0006`, `crates/rr-defs/src/romraider/rom.rs:191-192`) and keeps
  `<checksummodule>` as text (`crates/rr-defs/src/romraider/romid.rs:96-97`);
  the Subaru table is an ordinary switch to the reader (LEG-DEF-17).
- **Registry.** `alt` is an alias of `std` (section 3), not a separate
  algorithm. `alt2` keeps its coverage quirks (section 4) and becomes an
  ordered spec: `sumt`, `xort`, then the two 16-bit sums over the patched
  image, which also fixes MF-46. Motronic is `Sum{width 16, bytes,
  init}` over one or two inclusive ranges; `bytexor` is `Xor` of bytes;
  `copy` is `Copy`. E38 becomes an explicit order (CVN before sum, per
  segment) instead of two passes, the segment table is validated, and a
  failing segment is an error with zero patches (**MF-01**, US-RR-35
  criterion 3). The Subaru table is `Sum{32, big, target 5AA5A55A}` per
  entry, entries read from the image at a spec-given address (US-RR-35:
  "driven by the ChecksumSpec, not by a table named 'Checksum Fix'").
- **Ranges.** The spec states inclusive or exclusive per algorithm as
  legacy ran it (Nissan, Subaru, `bytexor` exclusive; Motronic, `copy`,
  E38 inclusive). A range overlapping its own storage is excluded
  explicitly or refused (EC-11). A length that is not a whole number of
  words is the open question pinned by `pad = "zero"` in
  `fixtures/kat/checksum-sum-words.toml`.
- **compute() returns patches.** `compute()` yields `Patch`es tagged
  `checksum fix` in the journal (`crates/rr-rom/src/journal.rs:173-174`),
  never writes in place; on an unmodified stock ROM it yields none
  (US-RR-35 criterion 2). The image is unchanged if computing fails
  (MF-48).
- **Save.** US-RR-45: recompute as patches, **validate all**, refuse a
  failing checksum unless the user gave an override, which the sidecar
  journal records; atomic write with `.bak` (EC-13, EC-14). The
  `RomWrite` token already carries the checksum state and override
  (`crates/rr-core/src/capability.rs:265-267,314-341`). This replaces
  MF-46.
- **Edit stamp.** Not specified by US-RR-35; whether the port writes a
  stamp at all, and where, is a decision for the Subaru family profile
  (LEG-CHK-08). It must not hide behind a fixed `+204`.
- **Resize** (10.2) has no story yet; if it is ported, checksums are
  recomputed on the resized image.

## Ledgered issues

Problems first found while cataloguing this page, each since entered in
PROJECT.md's Must-Fix ledger (`MF-nn`) or matched to an existing entry or
edge case (US-RR-12-3). The `UL-CHK-nn` IDs are kept here only so the
tasks and fixtures that cite them still resolve; new text cites the ledger
ID.

| ID | Ledger | Problem | Source |
|----|--------|---------|--------|
| UL-CHK-01 | MF-01 | **E38 fails silently.** A segment table that fails the checks leaves every value zero, so `update` writes **nothing** (not zeroed sums) and the ROM is saved with its old sums, reported as "0 of 12 updated"; the open-time popup is the only sign. A start or end word ≥ `80000000` is negative after the cast and passes the `≤ 200000` checks, and a segment past the image throws, aborting the open. MF-01's text now says so | `maps/checksum/ChecksumE38PCM.java:71-97,106-142`; `editor/ecu/OpenImageWorker.java:192-194` |
| UL-CHK-02 | MF-45 | **Half-configured managers.** When `configure` throws (missing or malformed attribute), the factory pops up "Error loading Checksum Manager" but returns the manager anyway; Motronic or `bytexor` then write their checksum at image offset 0–1 or 0 on save, Nissan throws on open. `type.toUpperCase()` is locale-dependent (Turkish `İ`) | `maps/checksum/ChecksumFactory.java:84-104`; `maps/checksum/ChecksumMOTRONICSINGLE.java:44-51,79-80`; `maps/checksum/ChecksumBYTEXOR.java:43-47,72`; `util/HexUtil.java:161-163` |
| UL-CHK-03 | MF-19 | **Three different "Checksum Fix" name tests in four places** (extends MF-19): validate by case-sensitive `contains` on `Switch` tables, edit block by `contains` on any table, stamp read by `equalsIgnoreCase`, recalculation by lower-cased `startsWith`; a table can be validated but never fixed, or fixed but never validated and editable | `maps/TableSwitch.java:56`; `maps/DataCell.java:211,482`; `maps/Rom.java:308,446` |
| UL-CHK-04 | MF-46 | **No consent on the Subaru fix; the locked path is dead.** An unlocked table is recalculated on every save even after the open-time warning that the image may be corrupt; a locked one is never recalculated because `isButtonSelected()` is always true for a `Table1D`, so its confirmation dialog is unreachable and the failing checksums are saved silently | `maps/Rom.java:417-437,474-486`; `maps/Table1D.java:79-82`; `maps/TableSwitch.java:61-79` |
| UL-CHK-05 | MF-46 | **Save never verifies.** Nothing re-validates after updating, so ordering faults reach the file unnoticed: `alt2`'s code sum reads stale `sumt`/`xort` when they lie past `skiploc`; a Motronic `loc` inside its range or a misaligned Nissan `sumloc` covers its own storage; a Subaru entry covering a later entry's `diff`; a manager writing inside a Subaru range or an earlier manager's range. Status counts do not mean "now correct" (Nissan always reports all). Related to EC-11, EC-12 | `maps/Rom.java:474-488,590-601`; `maps/checksum/CalculateALT2.java:80-85`; `maps/checksum/ChecksumMOTRONICSINGLE.java:86-93`; `maps/checksum/CalculateSTD.java:47-48`; `maps/RomChecksum.java:51-63` |
| UL-CHK-06 | MF-47 | **Subaru table fragility.** `sizey` not a multiple of 12 throws and the table is removed, so the save silently stops fixing the Subaru checksum; image-controlled out-of-range entries do the same; a disabled first entry reports "all disabled" without checking the rest, while the save recalculates the rest; a disabled entry whose `diff` is not `5AA5A55A` is rewritten | `maps/RomChecksum.java:66-99,49-64`; `maps/Rom.java:312-318,330-332` |
| UL-CHK-07 | MF-48 | **Save mutates the open image before writing.** Stamp, Subaru `diff`s and manager updates are applied to the in-memory image; if a later step throws or the write fails, the open ROM keeps them with nothing saved. "Increase/Decrease ROM size" saves checksums computed before the block move | `maps/Rom.java:441-490`; `swing/ECUEditorMenuBar.java:501-514,624-664` |
| UL-CHK-08 | MF-19 | **Edit stamp counter.** The count byte runs `FE` → `FF` and then back to `01`, so a stamp can read `v255`; it is a signed Java `byte` formatted with `%02x`; written only when exactly one table matches, so a ROM whose definition also has "Checksum Fix 2" (as some pinned bases do) never gets a stamp | `maps/Rom.java:451-472` |

Ledger entries this page cross-references: **MF-01** (E38 `calculated`
flag, silent no-op update, per-pass count), **MF-11** (checksum classes
loaded by name and from the definition folder), **MF-19** (`Checksum Fix`
by name, `+204`/`+207`), **MF-45** (half-configured managers), **MF-46**
(save never verifies, no consent), **MF-47** (Subaru table faults are
silent), **MF-48** (save mutates the open image; resize); edge cases
**EC-11** (ranges overlapping checksum storage), **EC-12** (checksums
depending on checksums), **EC-13** (interrupted save), and **MF-53**
(managers stack along the chain, null manager) of `definitions.md`. SECURITY.md's `RomWrite` rule ("Checksums must verify;
otherwise an explicit 'save with failing checksum' confirmation, recorded
in the sidecar journal") and "no runtime code loading" (MF-11) are what
sections 2 and 10 are measured against.

## Fixtures to write

Stable IDs for US-RR-12-2. Each is an existing KAT file under
`fixtures/kat/` (format: `fixtures/README.md`, "Checksum reference
vectors"), a test of the checksum engine (US-RR-35, rr-rom) or the save
pipeline (US-RR-45), or a stock-ROM corpus test. Expected values are
hand-computed from this page (D-024), never from running Java. Where a
fixture pins a legacy bug, the expected result is the port's fixed
behaviour and the legacy result goes in its `notes`.

| ID | Kind | What it pins down | Section |
|----|------|-------------------|---------|
| LEG-CHK-01 | KAT (exists) `fixtures/kat/checksum-sum-words.toml` | 32-bit big-endian word sum (Nissan `sumt`), Subaru `target=0x5AA5A55A` incl. the disabled `[0,0)` entry, 16-bit word sums (`alt2`) and E38's two's complement; odd-length `pad` convention | 3, 4, 7.2, 8.3 |
| LEG-CHK-02 | KAT (exists) `fixtures/kat/checksum-sum-bytes.toml` | Motronic 16-bit byte sum with `initial`, wrap, chained second range; 8-bit frame sums | 5, 9 |
| LEG-CHK-03 | KAT (exists) `fixtures/kat/checksum-xor.toml` | `bytexor` and DS2 byte XOR; Nissan 32-bit `xort` | 3, 6.1, 9 |
| LEG-CHK-04 | KAT (exists) `fixtures/kat/checksum-crc.toml`, `fixtures/kat/ncs-codec-legacy.toml` | E38 CVN = CRC-16/ARC stored little-endian (`BB3D` → `3D BB`); NCS CRC-16/MCRF4XX and its complement | 7.2, 9 |
| LEG-CHK-05 | Engine test (rr-rom, synthetic image) | Nissan `std` worked example (`12 24 36 48`, `10 20 30 40`); `alt` resolves to the same algorithm; a misaligned `sumloc` is refused or excluded, never self-covering (EC-11) | 3 |
| LEG-CHK-06 | Engine test (rr-rom, synthetic image) | `alt2` worked example (`00 48`, `00 17 00 1B`, `00 09 00 07`, `00 31`), incl. bytes 02–03 and 12–13 outside `sumt`; `sumloc` after `skiploc`: one `compute()` gives a file that validates (legacy needed two saves, MF-46; EC-12); default `skiploc` | 4 |
| LEG-CHK-07 | Engine test (rr-rom, synthetic image) | Motronic single `13 39`, double `14 39`, wrap `01EE`, inclusive ends; `loc` inside the range refused (EC-11); `bytexor` `70`; `copy` copies and an edit at the destination is reported, not silently overwritten | 5, 6 |
| LEG-CHK-08 | Engine test + decision (Subaru profile) | Subaru table: the 8.3 example (`5AA1A554`), wrap (`5AA5A559`), disabled first entry (−1 in legacy) and later entries still checked, RAM offset applied except to `0/0`, `sizey` not a multiple of 12 is a load Error (MF-47), out-of-image entry an Error, an entry covering a later entry's `diff` (EC-11/12); decide whether the `+204` stamp is written (MF-19) | 8 |
| LEG-CHK-09 | Corpus test (T2 (c), US-RR-35 criterion 1) | Every non-suspect stock Subaru ROM of the corpus validates under its "Checksum Fix" spec; `compute()` is a no-op; one flipped data byte fails validation and `compute()` patches only `diff` words | 8 |
| LEG-CHK-10 | Engine T6 test (synthetic image) | E38 segment table failing each check (start > `200000`, odd start, even end, short, negative start ≥ `80000000`, past the image, image shorter than `100DC`): error and zero patches (MF-01) | 7.1, 7.3 |
| LEG-CHK-11 | Engine test (synthetic E38-shaped image) | One segment with stale CVN and sum: one `compute()` fixes CVN then sum in declared order and validates (no second pass); count is 2, not 3 (MF-01); order of spec listing does not matter (EC-12) | 7.2, 7.3 |
| LEG-CHK-12 | Corpus test, when an E38 image is available | Real E38 image validates 12/12; confirms the segment-table offsets of 7.1 (stride `24` then `23`) | 7.1 |
| LEG-CHK-13 | rr-defs / spec test | `<checksum>` with a missing or malformed parameter (no `end`, `0X10`, `80000000`) is an Error, not a half-configured checksum (MF-45); unknown `type` and `path` attribute: Error, no code loading (MF-11, with LEG-DEF-25) | 2 |
| LEG-CHK-14 | Save-pipeline T6 test (US-RR-45) | A failing checksum without override: refused, file untouched; with override: saved and journaled; the in-memory image unchanged when any step fails (MF-46, MF-48); every saved file re-validates (MF-46) | 10.2 |
| LEG-CHK-15 | Engine test | Ordering across checksums: a Subaru range covering a manager's storage, and a manager writing inside another's range, both validate after one `compute()` (EC-12, MF-46) | 10.3 |
