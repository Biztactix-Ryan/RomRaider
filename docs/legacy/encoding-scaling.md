# Storage, byte order and scaling: legacy semantics

What the Java RomRaider ROM editor does with the bytes of a table cell: how
each storage type is read and written, which byte order applies (including
the float "hack" and the static memory-model endian), how masks, bit flags,
the Bosch-subtract layout and the 3D cell order work, how a raw value becomes
a real one and back (`expression`, `to_byte`, the binary-search fallback,
rounding and clamping), the JEP expression dialect and `BitWise`, and what
display formatting does. Written from the source only (the Java was never
run, D-024). It is the spec behind rr-core's Encoding (C3), Expr (C4a) and
Conversion (C4b) and their known-answer fixtures. Part of the legacy behaviour
catalogue, story US-RR-12, task US-RR-12-6.

How the `storagetype`, `endian`, `mask`, `dataLayout`, `<memmodel endian>`
and `<scaling>` **attributes are parsed and inherited** is in
[`definitions.md`](definitions.md) (sections 4.4, 5.3 and 6); this page
links to it and covers what the parsed values **mean**. Logger parameter
conversion has its own decoder and belongs to `logger.md` (US-RR-12-9); it
is mentioned here only where it shares code with the editor (JEP) or differs
instructively. Checksums are in `checksums.md` (US-RR-12-7).

## How to read this page

- **References.** Source references are relative to
  `legacy/src/main/java/com/romraider/`, the convention the PROJECT.md
  Must-Fix ledger uses: `maps/DataCell.java:261` is
  `legacy/src/main/java/com/romraider/maps/DataCell.java`, line 261. Files
  elsewhere are written from the repository root (`legacy/...`,
  `fixtures/...`, `crates/...`). Every reference is `file:line` or
  `file:first-last`.
- **The JEP library** is not source in `legacy/`: it is the binary
  `legacy/lib/common/jep.jar`. Claims about it cite a class and method with
  bytecode offsets (`JEP.<init>@24-26`), read from the class files as data,
  never run (D-024), the convention of `crates/rr-core/src/expr/eval.rs`,
  which already documents JEP's numeric rules in full; this page summarises
  and links rather than repeating them.
- **Bugs.** Where legacy behaviour is wrong or fragile, the page says so and
  cites the Must-Fix ledger entry (`MF-nn`) or edge case (`EC-nn`) of
  PROJECT.md. Do not port it as intended. A sentence that calls legacy
  behaviour a bug always carries its ledger ID (`cargo xtask
  check-legacy-docs`, rule 7). [Ledgered issues](#ledgered-issues) maps the
  problems first found on this page (`UL-ENC-nn`, IDs that tasks and
  fixtures still cite) to their ledger entries.
- **Numbers.** Bytes are hex without `0x`. Every worked example was computed
  by hand for this page, with the working shown; none came from running code.
  "Raw" (legacy: `binValue`) is the integer or float stored in the cell;
  "real" is the scaled value shown to the user.

### Source files

| Alias | Path under `legacy/src/main/java/com/romraider/` | Role |
|-------|---------------------------------------------------|------|
| `xml/RomAttributeParser.java` | same | Byte-level codec: integer and `MOVI20` read/write, masked read, float read/write and the float hack |
| `maps/DataCell.java` | same | One cell: address arithmetic, which codec path, the Bosch-subtract transform, raw ranges and clamping, `setRealValue` (real to raw), save to the image |
| `maps/Table.java` | same | The static `memModelEndian`, mask clamp, data layout, scale selection, `validateScaling`, populate, interpolation |
| `maps/Table3D.java` | same | 3D cell order: `swapxy`, `flipx`, `flipy`, `skipCells` |
| `maps/TableBitwiseSwitch.java`, `maps/TableSwitch.java` | `maps/` | Bit-flag cells, default storage of switches |
| `maps/Scale.java` | same | Scaling fields and defaults, `validate`, the binary-search inverse |
| `xml/TableScaleUnmarshaller.java` | same | Where the parsed endian, memory-model endian, storage and mask reach the table |
| `util/JEPUtil.java`, `util/BitWise.java` | `util/` | The two JEP entry points, the global parser cache, the `BitWise` function |
| `util/ByteUtil.java`, `util/NumberUtil.java` | `util/` | Mask helpers; locale number parsing and formatting |
| `maps/DataCellView.java` | same | Display formatting of cells (`format`) |
| `Settings.java` | same | `Endian` codes, storage-type and `MOVI20` range constants |
| `logger/ecu/definition/EcuParameterConvertorImpl.java`, `EcuDerivedParameterConvertorImpl.java` | same | The logger's decoder and JEP use (contrast only) |
| `legacy/lib/common/jep.jar` | (binary) | JEP 2.x, class files dated 2006-03-14 |

## 1. Storage types

### 1.1 Codes

A table's storage is one `int`, `storageType`, plus a separate boolean
`signed` (`maps/Table.java:54-55`). Parsing is in `definitions.md` 4.4; the
codes that reach the codec are:

| `storageType` | From `storagetype=` | Bytes per cell (stride) | Signed | Read by | Source |
|---------------|---------------------|-------------------------|--------|---------|--------|
| 1 | `uint8`, `int8`, `1` | 1 | `int8` only | `ByteBuffer.get` | `xml/RomAttributeParser.java:62-69,146-148` |
| 2 | `uint16`, `int16`, `2` | 2 | `int16` only | `getShort` | `xml/RomAttributeParser.java:149-151` |
| 4 | `uint32`, `int32`, `4` | 4 | `int32` only | `getInt` | `xml/RomAttributeParser.java:152-154` |
| 3, 8 | `uint24`, `int24`, `uint64`, `int64`, `3`, `8` | 3, 8 | as spelled | **nothing: reads 0** (**MF-02**) | `xml/RomAttributeParser.java:145-160` |
| 0 | no `storagetype` anywhere | 0 | no | **nothing: reads 0** (MF-02) | `xml/RomAttributeParser.java:145-160` |
| 20 | `movi20` (any case), `20` | **3** | `movi20`: yes; `20`: no, but always sign-extended (1.4) | `MOVI20` decoder | `Settings.java:122`; `xml/RomAttributeParser.java:53-54,155-158` |
| 28 | `movi20s` (any case), `28` | **3** | as 20 | `MOVI20` decoder | `Settings.java:123`; `xml/RomAttributeParser.java:56-57` |
| 99 | `float` (any case), `99` | **4** | no (the flag only affects the edit range, 1.6) | IEEE 754 binary32 | `Settings.java:124`; `xml/RomAttributeParser.java:59-60,271-286` |

Switches and bitwise switches default to storage 1 (`maps/TableSwitch.java:37-40`;
`maps/TableBitwiseSwitch.java:29-31`). `signed` comes from the spelling: a
`storagetype` starting `int` or `movi20` (any case) is signed
(`xml/RomAttributeParser.java:73-81`), and only the first level that gives a
width sets it (**MF-06**).

Cell `i` of a table is at image offset `storageaddress + i × stride −
ramOffset` (ram offset: `definitions.md` 2.3), with stride `storageType` for
integers (`maps/DataCell.java:177-187`), 4 for float
(`maps/DataCell.java:161-167`) and 3 for `MOVI20`
(`maps/DataCell.java:169-175`). The editor's "which cells share these bytes"
map is keyed by `storageaddress + i × storageType − ramOffset`
(`maps/DataCell.java:343-346`), which is wrong for 20, 28 and 99 when `i > 0`
(**MF-03**). Edits are written into the in-memory image at once
(`maps/DataCell.java:505-506`); the cells registered at the same key are then
re-read (`maps/DataCell.java:333-341`).

### 1.2 Integer read

`parseByteValue(image, endian, offset, length, signed)`
(`xml/RomAttributeParser.java:133-178`) wraps the image at `offset` in a
`ByteBuffer` of `length` bytes, little-endian if the table says so (big
otherwise), and reads one `byte`, `short` or `int` by width. Java reads these
**signed**, so the result is sign-extended into a `long`; then, unless the
table is signed, it is masked back to `0xFF`, `0xFFFF` or `0xFFFFFFFF`
(`xml/RomAttributeParser.java:161-173`). A cell past either end of the image
throws `IndexOutOfBoundsException` (`xml/RomAttributeParser.java:175-177`),
which the populate loop turns into an "out of bounds" popup (EC-02,
`definitions.md` 8).

*Worked example (hand-computed).* Bytes `FF 38`:

- `uint16`, big: `FF·256 + 38` = 255·256 + 56 = 65 280 + 56 = **65 336**.
- `int16`, big: 65 336 has bit 15 set, so 65 336 − 65 536 = **−200**.
- `uint16`, little: `38·256 + FF` = 56·256 + 255 = 14 336 + 255 = **14 591**.
- `int8` of the first byte `FF`: **−1**; `uint8`: **255**.

### 1.3 Integer write

`saveBinValueInFile` (`maps/DataCell.java:210-316`), for every non-float
storage:

1. Skipped entirely for a table whose name contains `Checksum Fix`
   (**MF-19**, `maps/DataCell.java:211`) and for a cell the user's level may
   not save (`maps/DataCell.java:236`; `definitions.md` 5.1).
2. `finalValue = (int) binValue` (`maps/DataCell.java:261`). Java's
   `double`-to-`int` cast truncates toward zero, maps NaN to 0 and
   **saturates** at `2^31 − 1`. So a `uint32` raw value of `2^31` or more is
   written as `7F FF FF FF` (MF-61).
3. A mask shifts it left (section 3).
4. `parseIntegerValue(finalValue, endian, storageType)`
   (`xml/RomAttributeParser.java:193-224`) puts it into a 1-, 2- or 4-byte
   buffer with a narrowing cast (`(byte)`, `(short)`): no range check, the
   high bits are dropped. Widths 3 and 8 fall through the `switch` and write a
   zeroed buffer of that length (**MF-02**). Width 0 writes nothing.
5. The bytes are copied into the image (`maps/DataCell.java:296-300`).

Raw values reach step 2 already clamped to the cell's range (1.6), so the
narrowing cast only bites through the paths that skip the clamp or saturate
first (MF-61, MF-62).

### 1.4 `MOVI20` and `MOVI20S`

An SH-2A `MOVI20 #imm20, Rn` instruction is 4 bytes, `0000 nnnn | iiii 000s
| iiii iiii | iiii iiii`, where `s = 0` is `MOVI20` and `s = 1` is `MOVI20S`
(the CPU loads `imm20 << 8`). A definition's `storageaddress` points at the
**second** byte (the one holding `imm20[19:16]` and `s`), and the cell is
that byte and the two after it: the codec reads from one byte before the
cell to take in the opcode (`xml/RomAttributeParser.java:137-141,155-158`;
`fixtures/kat/u20-movi20.toml` notes the same convention).

**Read** (`xml/RomAttributeParser.java:137-158,181-191`):

1. Wrap 3 bytes at the cell's offset, then move the position **back one
   byte** and read a 4-byte `int` from `offset − 1`, so the opcode byte is
   read too (`xml/RomAttributeParser.java:137-141,157-158`). In big-endian
   order the int is `opcode·2^24 + b0·2^16 + b1·2^8 + b2`.
2. `getMovi20` (`xml/RomAttributeParser.java:181-191`):
   `shift = int & 0x00010000` (bit 0 of `b0`, the instruction's `s` bit);
   `value = ((int & 0x00F00000) >>> 4) + (int & 0xFFFF)`, i.e.
   `(b0 >> 4)·2^16 + b1·2^8 + b2`; if bit 19 is set,
   `value |= 0xFFF00000` (sign-extend); if `shift` is set, return
   `value << 8`.
3. The unsigned masking of 1.2 has no case for 20 or 28
   (`xml/RomAttributeParser.java:161-173`), so the value is **always**
   sign-extended, whatever `signed` says.

So **the ROM's `s` bit, not the storage type, decides the `<< 8`**
(`getMovi20` tests the bit, never `length`:
`xml/RomAttributeParser.java:182,187-189`): a `movi20` table over a `MOVI20S`
instruction reads the shifted value, and a `movi20s` table over a `MOVI20`
instruction reads the unshifted one.

**Write** (`xml/RomAttributeParser.java:193-236`; `maps/DataCell.java:271-275`):
the value (for 28 first shifted right by 8, arithmetic:
`xml/RomAttributeParser.java:216-217`) is put into a 4-byte buffer and
`parseMovi20` builds 3 bytes: `out0 = (byte)(buf[1] << 4)`, then `| 0x01` for
storage 28; `out1 = buf[2]`; `out2 = buf[3]`
(`xml/RomAttributeParser.java:227-236`). So:

- the low nibble of the cell's first byte is **overwritten** with `0000`
  (storage 20) or `0001` (storage 28): the instruction type follows the
  **storage type**, not what was in the ROM (MF-64);
- a value above the 20-bit range loses its high bits silently; for storage
  28 the low 8 bits of the raw value are dropped (`>> 8`) silently (MF-64);
- the opcode byte at `offset − 1` is never written.

Edit ranges: `movi20` −524 288..524 287 (`0xFFF80000..0x0007FFFF`),
`movi20s` −134 217 728..134 217 472 (`0xF8000000..0x07FFFF00`)
(`Settings.java:118-121`; `maps/DataCell.java:121-128`), applied only when
`signed` is set, i.e. only for the `movi20`/`movi20s` spellings; a numeric
`20` gets the unsigned range `0..256^20 − 1` (`maps/DataCell.java:131-139`).

Other quirks: the stride is 3 (`maps/DataCell.java:173`), but consecutive
instructions are 4 bytes apart, so a multi-cell `MOVI20` table cannot address
consecutive instructions; a little-endian `MOVI20` table reads the four bytes
reversed (garbage); a cell at image offset 0 makes `position(−1)` throw
`IllegalArgumentException`, which neither the codec
(`xml/RomAttributeParser.java:175-177`) nor the populate loop
(`maps/Rom.java:312-327`) catches. All MF-64.

*Worked example (hand-computed).* Instruction bytes `01 80 FE 0C`
(`MOVI20 #imm, R1`), cell at the `80`:

- int = `0180FE0C`; `shift = 0180FE0C & 00010000 = 0` (`80` has low bit 0);
- `(0180FE0C & 00F00000) >>> 4 = 00800000 >>> 4 = 00080000`;
  `0180FE0C & FFFF = FE0C`; value = `0008FE0C`;
- bit 19 (`00080000`) is set: value = `0008FE0C | FFF00000 = FFF8FE0C`;
  as a signed int that is −(`100000000 − FFF8FE0C`) = −`000701F4` =
  −(7·65 536 + 500) = −(458 752 + 500) = **−459 252**. Cross-check: imm20 =
  `8FE0C` = 589 324, and 589 324 − 2^20 = 589 324 − 1 048 576 = −459 252.
- The same bytes with `81` (a `MOVI20S`): −459 252 · 256 = **−117 568 512**.
- Writing −459 252 back as storage 20: `putInt` gives `FF F8 FE 0C`;
  `out0 = (byte)(F8 << 4) = 80`, `out1 = FE`, `out2 = 0C`: the cell becomes
  `80 FE 0C` again. As storage 28 the value −117 568 512 `>> 8` is −459 252,
  the same bytes, then `out0 = 80 | 01 = 81`.

### 1.5 Float

Storage 99 reads 4 bytes at `storageaddress + i·4 − ramOffset`
(`maps/DataCell.java:161-167`) and `byteToFloat`
(`xml/RomAttributeParser.java:271-286`) decodes them in the byte order of
section 2.2. Writing is `floatToByte((float) binValue, ...)`
(`maps/DataCell.java:302-309`; `xml/RomAttributeParser.java:252-269`): the
`double` is rounded to the nearest `float`, no other check. A mask is never
applied to floats on read (`maps/DataCell.java:161-167`). NaN, ±Inf and
subnormals are read and shown as they are; nothing stops an edit producing
one except the range clamp of 1.6 (EC-07).

### 1.6 Raw ranges and clamping

Each cell computes `minAllowedBin`/`maxAllowedBin` once, lazily, the first
time it is read while both are still 0 (`maps/DataCell.java:368-375`), by
`calcValueRange` (`maps/DataCell.java:105-149`):

| Storage | Signed | Range |
|---------|--------|-------|
| 1, 2, 4 | yes | `Byte`/`Short`/`Integer` `MIN..MAX` (`maps/DataCell.java:107-120`) |
| 20, 28 | yes | 1.4 (`maps/DataCell.java:121-128`) |
| 3, 8, 0 | yes | no case: stays `0..0`, so every edit clamps to 0 and the range is recomputed on every read (**MF-02**) |
| any integer | no, no mask | `0..256^width − 1` (`maps/DataCell.java:132-134`) |
| any integer | no, mask | `0..2^popcount(mask) − 1` (`maps/DataCell.java:135-137`) |
| 99 | no (the normal case) | `−Float.MAX..Float.MAX` |
| 99 | yes (`int…`-spelled float is impossible, but a base can set it) | `0..Float.MAX`: signed and unsigned are swapped (**MF-15**, `maps/DataCell.java:140-147`) |

`setBinValue` clamps into that range silently (no message), then saves
(`maps/DataCell.java:481-508`; the port rejects instead, EC-08). Comparisons with NaN are false, so a NaN raw
value passes the clamp. The scaling's `min`/`max` attributes are never used
for clamping (`definitions.md` 9).

## 2. Byte order

### 2.1 Which order a table gets

`Endian` is `LITTLE(1)` or `BIG(2)` (`Settings.java:93-106`); a table defaults
to big (`maps/Table.java:56`). When the ROM's `<memmodel>` has an `endian`
attribute, every table read after it takes that order instead of its own
(`xml/TableScaleUnmarshaller.java:162-170`); the parsing rules and the
leak between ROMs of one file are in `definitions.md` 5.3 (MF-52,
MF-60). Integers and `MOVI20` use the table's order directly
(1.2, 1.4).

### 2.2 The float hack

Floats do **not** use the table's order. `byteToFloat` and `floatToByte`
take both the table's endian and the static memory-model endian
(`xml/RomAttributeParser.java:252-286`):

| `Table.memModelEndian` (static) | Table `endian` | Float byte order |
|---------------------------------|----------------|------------------|
| `LITTLE` | any | little |
| `BIG` | any | big |
| `null` (no `<memmodel endian>` seen yet) | `LITTLE` | **big** ("this case corrects improperly defined float table endian in legacy definition files", `xml/RomAttributeParser.java:262-265,280-283`) |
| `null` | `BIG` | big (`ByteBuffer`'s default) |

So without a memory-model endian, a float table is **always big-endian**,
whatever its `endian` says (`xml/RomAttributeParser.java:261-266,279-284`). The `endian` attribute of a float table never
matters.

*Worked example (hand-computed).* Cell bytes `3F 80 00 00`, table
`endian="little"`, no `<memmodel endian>` anywhere: legacy reads big-endian
`3F800000` = sign 0, exponent `7F` = 127, fraction 0: 2^(127−127) = **1.0**.
A plain little-endian read would take `0000803F`: exponent 0, fraction
`803F` = 32 831, a subnormal 32 831 × 2^−149 ≈ 4.60 × 10^−41.

### 2.3 Static state (MF-04)

`memModelEndian` is a **static** field of `Table` (`maps/Table.java:40`),
written through an instance setter (`maps/Table.java:761-767`) each time a
table is read under a `<memmodel endian>`
(`xml/TableScaleUnmarshaller.java:166-169`), and **never reset**: a ROM
without `<memmodel endian>` does not clear it
(`xml/TableScaleUnmarshaller.java:162-165` only sets the table's own order).
Consequences (**MF-04**):

- the float hack of 2.2 applies only until the first ROM with a
  `<memmodel endian>` is opened in the session; after that every float table
  of every ROM, opened before or after, decodes and saves in that ROM's
  order;
- two open ROMs with different memory-model endians share one value: the
  last one loaded wins for both;
- integers and `MOVI20` are not affected (they read the per-table field).

### 2.4 Contrast: the logger

The logger decodes with its own code: the parameter's endian applies to
floats directly, with no hack and no static
(`logger/ecu/definition/EcuParameterConvertorImpl.java:72-84`). Its storage
spelling and endian rules belong to `logger.md`.

## 3. Masks and bit flags

### 3.1 `mask`: read

A table `mask` (hex, clamped to the storage width at parse time:
`maps/Table.java:537-548`; `definitions.md` 4.4) is copied into every cell
(`maps/DataCell.java:63-67,98-103`). A masked integer cell reads
`(parseByteValue(...) & mask) >> firstOneOfMask(mask)`
(`xml/RomAttributeParser.java:124-131`; `maps/DataCell.java:178-187`;
`util/ByteUtil.java:122-133`):

- the mask applies to the **decoded value**, after byte order, so a 16-bit
  little-endian cell with mask `00F0` takes bits 4..7 of the decoded number,
  not of the first byte (as `fixtures/kat/bitfield-u16-le.toml` pins);
- the field is never sign-extended: a signed table's field reads unsigned
  (EC-06 "signed bitfields" has no legacy form);
- the code assumes contiguous masks ("OK = 11110000, Not OK = 11001100",
  `xml/RomAttributeParser.java:124`); a split mask reads the bits with the
  gap left in, while the edit range is `2^popcount − 1` (1.6).

### 3.2 `mask`: write

`saveBinValueInFile` shifts the raw value left by `firstOneOfMask(mask)`,
encodes it with `parseIntegerValue` (1.3), then for each byte `z` of the cell
clears `(mask >> 8·(len−1−z)) & FF` in the image and **ORs in the unmasked
output byte** (`maps/DataCell.java:264-294`). Two problems:

- the per-byte mask is cut **most significant byte first**, but the output
  bytes are in the table's byte order, so a **little-endian** multi-byte
  masked cell clears the wrong byte and ORs the field into a byte it did not
  clear (MF-62);
- the output byte is not masked, so a value wider than the field spills into
  neighbouring bits. The unsigned range (1.6) keeps contiguous unsigned
  fields inside the mask, but a signed table's range is the full width, and
  a split mask's is too wide (MF-62).

*Worked example (hand-computed): big-endian works.* `uint16`, mask `00F0`,
bytes `A7 35`. Read: `A735 & 00F0 = 0030`, `>> 4` = **3**. Write 5: `5 << 4 =
0050`, big-endian bytes `00 50`. `z = 0`: mask byte `(00F0 >> 8) & FF = 00`:
`A7 & ~00 = A7`, `| 00` = `A7`. `z = 1`: mask byte `F0`: `35 & 0F = 05`,
`| 50` = `55`. Image `A7 55`; read back `A755 & 00F0 = 0050 >> 4` = **5**.

*Worked example (hand-computed): little-endian corrupts (MF-62).* Same
field, little-endian, bytes `35 A7` (the value is again `A735`, field 3).
Write 5: `0050`, little-endian bytes `50 00`. `z = 0`: mask byte `00`:
`35 & ~00 = 35`, `| 50` = **`75`**. `z = 1`: mask byte `F0`: `A7 & 0F` =
**`07`**, `| 00` = `07`. Image `75 07`; read back `0775 & 00F0 = 0070 >> 4` =
**7**, not 5, and the high nibble `A` of the second byte, outside the field,
is destroyed.

### 3.3 Bitwise switches

A `BitwiseSwitch` has one cell per `<bit position>`, every cell at index 0
of the table (offset `storageaddress`), with mask `1 << position`
(`maps/TableBitwiseSwitch.java:46-53`; `util/ByteUtil.java:147-149`); the
`<bit>` parsing is `definitions.md` 4.8. Each cell is constructed with the
**table's** mask (normally none), which reads the whole storage word and
fixes the cell's range and `originalValue` from it, before
`setBitMask(1 << position)` and a second read
(`maps/DataCell.java:77-84,368-371`; `maps/TableBitwiseSwitch.java:49-51`).
So a bit cell's range is `0..255` (for storage 1) and its `originalValue`,
used by undo (`maps/DataCell.java:547-549`), is the whole byte (MF-63).

*Worked example (hand-computed).* Byte `2A` (`0010 1010`), bit 3. The cell
reads `(2A & 08) >> 3` = 1, `originalValue` = `2A` = 42. Undo before any
revert point: `setBinValue(42)`, inside `0..255`, saved as `42 << 3` = 336 =
`150`, `(byte)` = `50`; then `2A & ~08 = 22`, `| 50` = **`72`**
(`0111 0010`): bit 3 is cleared and bits 4 and 6, two other flags, are set.

## 4. Data layouts

### 4.1 Bosch subtract

`dataLayout="bosch_subtract"` (`maps/Table.java:520-527`; not inherited,
MF-50) makes each cell's value depend on the cells after it. With `s[j]`
the ordinary decoded raw value of cell `j` (mask and sign included),
`W` the storage width in bytes and `N` the cell count
(`maps/DataCell.java:193-208`):

```text
value[i] = 2^(8W) − (s[i] + s[i+1] + … + s[N−1])
```

Writing cell `i` (`maps/DataCell.java:219-234`) computes the new `s[i]` as
`2^(8W) − s[N−1] − … − s[i+1] − value[i]`, through the integer write of 1.3,
then rewrites cell `i−1` from its unchanged value, which cascades to cell 0
(`maps/DataCell.java:312-313`). For the **last** cell the loop starts from 0
instead of `2^(8W)`, so it stores `−value[N−1]`; the narrowing cast of 1.3
makes that `2^(8W) − value[N−1]` modulo `2^(8W)`, which is the right byte
pattern (by accident). Limits (MF-65): the values are clamped to the
storage range of 1.6 although a valid value can be `2^(8W)` (all `s` zero) or
negative; for `W = 4` the `(int)` cast saturates (MF-61); a 3D table
cannot use the layout because `table.data` is the hidden one-element array
(**MF-12**, `maps/Table3D.java:36`).

*Worked example (hand-computed).* `uint8` axis, `N = 3`, bytes `10 20 30`
(`s` = 16, 32, 48), `2^(8·1)` = 256:

- `value[2]` = 256 − 48 = **208**; `value[1]` = 256 − 48 − 32 = **176**;
  `value[0]` = 256 − 48 − 32 − 16 = **160**.
- Set `value[1]` = 180: new `s[1]` = 256 − 48 − 180 = **28** (`1C`). Cascade
  to cell 0 (value still 160): `s[0]` = 256 − 48 − 28 − 160 = **20** (`14`).
  Image `14 1C 30`; read back 256 − 48 − 28 = 180 and 256 − 48 − 28 − 20 =
  160.
- Set `value[2]` = 200: the loop gives `0 − 200` = −200, `(byte)(−200)` =
  −200 + 256 = 56 = `38`; read back 256 − 56 = 200.

Equivalently, from the sum of `maps/DataCell.java:197-201`,
`s[N−1] = 2^(8W) − value[N−1]` and `s[i] = value[i+1] − value[i]`:
208 − 176 = 32, 176 − 160 = 16, 256 − 208 = 48.

### 4.2 3D cell order: `swapxy`, `flipx`, `flipy`, `skipCells`

`Table3D.populateTable` (`maps/Table3D.java:166-221`) walks the image cells
in order, `offset` counting **cells** from `storageaddress`
(`maps/Table3D.java:186-213`):

- outer loop `i` over `iMax`, inner `j` over `jMax`, where `iMax` is the Y
  axis length and `jMax` the X axis length, or the other way round when
  `swapxy` (`maps/Table3D.java:187-188`);
- `x = flipy ? jMax−1−j : j`, `y = flipx ? iMax−1−i : i`, then `swapxy` swaps
  `x` and `y` (`maps/Table3D.java:193-199`);
- the cell at `offset` goes to `data[x][y]`; after the last cell of each inner
  run `offset` advances by `1 + skipCells`, otherwise by 1
  (`maps/Table3D.java:200-211`).

So the default is row-major (X fastest), `swapxy` is column-major (Y
fastest), and `skipCells` is in cells, not bytes
(`maps/Table3D.java:187-211`). **Without** `swapxy` the
flip names are crossed: `flipy` reverses the X direction and `flipx` the Y
direction; with `swapxy` they match. This is the data contract existing
definitions are written against, so the port reproduces it, it does not fix
it.

*Worked example (hand-computed).* X axis 3, Y axis 2, `uint8`, image from
`storageaddress`: `0A 0B 0C 0D 0E 0F 10`. `data[x][y]`:

| Attributes | (0,0) | (1,0) | (2,0) | (0,1) | (1,1) | (2,1) |
|------------|-------|-------|-------|-------|-------|-------|
| none | `0A` | `0B` | `0C` | `0D` | `0E` | `0F` |
| `flipy` | `0C` | `0B` | `0A` | `0F` | `0E` | `0D` |
| `flipx` | `0D` | `0E` | `0F` | `0A` | `0B` | `0C` |
| `swapxy` | `0A` | `0C` | `0E` | `0B` | `0D` | `0F` |
| `skipCells="1"` | `0A` | `0B` | `0C` | `0E` | `0F` | `10` |

(By `maps/Table3D.java:187-211`. `swapxy`: `i` = 0..2 over X, `j` = 0..1
over Y, so offsets 0, 1 are (0,0),
(0,1); 2, 3 are (1,0), (1,1); 4, 5 are (2,0), (2,1). `skipCells="1"`: row 0
at offsets 0, 1, 2, then `offset` = 2 + 1 + 1 = 4, row 1 at 4, 5, 6.)

### 4.3 Static cells

A table with `<data>` children (`definitions.md` 4.5) is never read from the
image (`maps/Table.java:150`). Each `<data>` text is kept as a string and
shown by parsing it as a double and putting it through the current
`expression` (`maps/DataCellView.java:383-393`): static data are **raw**
values. Text that does not parse is shown as it is. Static cells cannot be
selected (`maps/DataCell.java:353-354`), so the static branch of
`saveBinValueInFile` (`maps/DataCell.java:245-259`) is not reached by an
edit.

## 5. Raw to real

`getRealValue` evaluates the current scale's `expression` with `x` = the raw
value through JEP (`maps/DataCell.java:422-426`; section 7); with no current
scale it is the raw value. The scale is the one `addScale` selected
(`definitions.md` 6.4); defaults are `expression="x"`, `to_byte="x"`,
`format="#.##"` (`maps/Scale.java:36-38`). Nothing checks the result: NaN and
±Inf (division by zero at some raw value, `log` of 0, a parse error, section
7.3) are displayed (EC-07, EC-08). Compare views use the same expression on
both values (`maps/DataCell.java:464-479`). `getMaxReal`/`getMinReal` apply
the expression to the largest/smallest raw value **present in the table**,
which is the wrong way round for a decreasing scaling (**MF-13**,
`maps/Table.java:444-450`; `maps/Table.java:402-434`).

## 6. Real to raw

### 6.1 Which path

`setRealValue(text)` (`maps/DataCell.java:428-458`):

1. Spaces and non-breaking spaces are removed (`maps/DataCell.java:430`;
   `maps/DataCellView.java:49`); the literal input `x` (any case) does
   nothing (`maps/DataCell.java:433`).
2. The text is parsed in the **default locale** with `NumberFormat.parse`
   (`util/NumberUtil.java:31-35,55-57`), which accepts a valid prefix and
   locale grouping separators; an unparsable text is ignored without a
   message (`maps/DataCell.java:455-457`; MF-86).
3. If the scale's `expression`, trimmed, is `x` in any case: raw = the
   number (`maps/DataCell.java:436-439`). So `expression="X"` is treated as
   identity here, while JEP sees an undeclared `X` and shows NaN (7.3,
   MF-66).
4. Else if the scale has no `to_byte`: the binary search of 6.3
   (`maps/DataCell.java:440-442`).
5. Else: evaluate **`to_byte`** with `x` = the number
   (`maps/DataCell.java:443-445`). `to_byte` is trusted: it is never checked
   against `expression` at edit time (only once, at load, 6.5; MF-66).

A scaling with `to_byte` uses it; one without falls back to the search
(`maps/DataCell.java:440-445`).
`to_byte` is easily lost through inheritance (MF-50), which silently
switches a table to the search.

### 6.2 Rounding and clamping

For every non-float storage the result becomes `(int) Math.round(result)`
(`maps/DataCell.java:447-449`): round half **up** (toward +∞), as a `long`,
then a **wrapping** `long`-to-`int` cast (MF-61). Then `setBinValue` clamps
into the raw range (1.6) silently and saves (`maps/DataCell.java:481-508`;
EC-08). Floats are
not rounded; the clamp is `±Float.MAX` and the `(float)` cast of 1.5.

*Worked examples (hand-computed),* `expression="x*0.5"`, `to_byte="x/0.5"`,
`int16`:

- real 10.25: 10.25 / 0.5 = 20.5, `Math.round` = floor(20.5 + 0.5) = **21**.
- real −10.25: −20.5, floor(−20.5 + 0.5) = floor(−20) = **−20** (half up, not
  away from zero).
- real 20 000 on `int8` (`−128..127`): 40 000, clamped silently to **127**,
  which displays as 63.5. EC-08 wants this refused.
- `uint32`, `expression="x"`, real 3 000 000 000: identity path, `Math.round`
  = 3 000 000 000 as a `long`; `(int)` keeps the low 32 bits: 3 000 000 000
  − 4 294 967 296 = −1 294 967 296; clamped to the unsigned minimum: **0**
  is written (MF-61).

### 6.3 Without `to_byte`: the binary search (MF-05)

`approximateToByteFunction(input, storageType, signed)`
(`maps/Scale.java:79-140`):

1. A per-scale cache (`HashMap`, up to 100 entries) keyed by the input
   returns an earlier answer (`maps/Scale.java:44-45,82-85`).
2. `max = (int) 2^(8·storageType)`, saturating at `2^31 − 1` for storage 4,
   20, 28 and 99; unsigned: `max − 1`, signed: `−max/2..max/2 − 1`
   (`maps/Scale.java:87-96`).
3. Start at `current = (max − min)/2`, `step = (int) 2^(8·storageType)/2`
   (`maps/Scale.java:101-102`). For a signed table the start is
   `(127 + 128)/2` = 127 for `int8`, not 0, and nothing keeps candidates
   inside `min..max`. For storage 4 `current + step` overflows `int`
   (**MF-05**).
4. Loop while `step > 0` and `error > 0.00001`: evaluate `expression` at
   `current − step` and `current + step`, move `current` to whichever is
   closer to the input (the minus side on a tie), and record `output =
   current` only when the error beats the **previous iteration's** error,
   not the best so far; halve `step` (`maps/Scale.java:106-131`).
5. Cache `output`, but **return `current`** (`maps/Scale.java:133-139`), so a
   first call and a repeated call can disagree (**MF-05**). It assumes a
   monotonic `expression`, and searches integers only, even for a float
   table (MF-05).

*Worked example (hand-computed).* `expression="x*0.5"`, no `to_byte`,
`uint8`, input 10.6 (nearest raw 21, 21·0.5 = 10.5). `max` = 255, `min` = 0,
`current` = 127, `step` = 128, `lastError` = 9 999 999:

| Step | Candidates (real) | Errors | `current` | Error < last? | `output` |
|------|-------------------|--------|-----------|---------------|----------|
| 128 | −1 (−0.5), 255 (127.5) | 11.1, 116.9 | −1 | yes | −1 |
| 64 | −65 (−32.5), 63 (31.5) | 43.1, 20.9 | 63 | no | −1 |
| 32 | 31 (15.5), 95 (47.5) | 4.9, 36.9 | 31 | yes | 31 |
| 16 | 15 (7.5), 47 (23.5) | 3.1, 12.9 | 15 | yes | 15 |
| 8 | 7 (3.5), 23 (11.5) | 7.1, 0.9 | 23 | yes | 23 |
| 4 | 19 (9.5), 27 (13.5) | 1.1, 2.9 | 19 | no | 23 |
| 2 | 17 (8.5), 21 (10.5) | 2.1, 0.1 | 21 | yes | 21 |
| 1 | 20 (10.0), 22 (11.0) | 0.6, 0.4 | 22 | no | 21 |

`step` becomes 0 and the loop ends. The call **returns 22** (real 11.0) and
caches **21** (real 10.5) (`maps/Scale.java:129,133-139`); typing 10.6 again
gives 21 (`maps/Scale.java:82-85`). The right answer is 21. For 1- and
2-byte storage `current` starts odd (127, 32 767) and moves
by even steps until the last one, so a search that does not hit an exact
match early always ends on an even raw value.

### 6.4 Non-finite results

The rounding of section 6.2 turns non-finite `to_byte` results into numbers
without a word (EC-08, `maps/DataCell.java:447-449`): `Math.round(NaN)` = 0; `Math.round(+∞)` =
`Long.MAX_VALUE`, whose low 32 bits are −1; `Math.round(−∞)` =
`Long.MIN_VALUE`, low 32 bits 0. Then the clamp. Example: `to_byte="1/(x-3)"`
at real 3 gives 1/0 = +∞, raw −1, which an unsigned table clamps to **0**
and an `int8` table stores as **−1** (`FF`). For a float table a NaN
`to_byte` result passes the clamp and is written as a NaN pattern. EC-08
requires a refusal.

### 6.5 The scaling check at load

At populate, every non-switch table runs `Scale.validate()` on each scale
(`maps/Table.java:151,554-562`): skipped without `to_byte` or when both are
exactly `x`; else `|to_byte(expression(5)) − 5| > 0.001` raises a "bad
scaling" popup from model code (**MF-40**), and the table still loads
(`maps/Scale.java:63-77`). It checks one raw value, 5; a NaN difference
compares false, so a `to_byte` that is NaN at that point passes (MF-66).

### 6.6 Other edits

- **Increment/decrement** (`maps/DataCell.java:510-545`): real + step through
  `to_byte` or the search, round as 6.2; if the real value did not change,
  retry with **double** the step (**MF-39**). A negative `coarseincrement`
  flips the direction.
- **Multiply** formats the product as a string, swaps `.` for `,` in a comma
  locale, and re-parses it (**MF-39**, `maps/DataCell.java:581-589`).
- **Interpolate** works on raw values (**MF-14**, `maps/Table.java:612-638`)
  and passes the unrounded result straight to `setBinValue`, so the
  `(int)` cast of 1.3 truncates it toward zero on save.

## 7. JEP

### 7.1 Library

`legacy/lib/common/jep.jar` is JEP 2.x (`org.nfunk.jep`, class files dated
2006-03-14), the version with `OperatorSet`, the vector operators `.` and
`^^` and `[...]` lists. Every value is a `Double` (`DoubleNumberFactory`,
`JEP.<init>@29-37`) unless a function returns a complex number.

### 7.2 The two entry points and the cache (MF-25)

`JEPUtil` (`util/JEPUtil.java:28-83`) has:

- `evaluate(expression, x)`, used by every ROM-editor conversion
  (`maps/DataCell.java:425,444,522`; `maps/Scale.java:71-72,107-108`;
  `maps/Table.java:445,449`; `maps/DataCellView.java:388`) and the logger's
  single-value conversions: a new `JEP`, `addStandardFunctions()`,
  `addFunction("BitWise", ...)`, `initSymTab()` (which empties the symbol
  table, so no constants), `addVariable("x", value)`, `parseExpression`
  (`util/JEPUtil.java:46-55`);
- `evaluate(expression, map)`, used by logger derived parameters and the
  dataflow simulation: **no** standard functions and **no** `BitWise`, one
  variable per map key (`util/JEPUtil.java:62-71`).

Both share one cache: a `synchronized` LRU map from the exact expression
string to the parsed `JEP` (`util/JEPUtil.java:30-44`), and both methods are
`static synchronized`, so every evaluation in the program is serialised. On
a hit only the variable values are set (`util/JEPUtil.java:56-58,72-79`). So
**the dialect of an expression is whichever entry point parsed it first**:
a string first parsed by the map overload has no functions and no `x`, so
later `x`-overload calls on it return NaN if it uses either; a string first
parsed by the `x` overload knows no other variable, so a later map call
whose formula needs one gets the cached parse error, NaN. Either way every
caller gets the first caller's parse. The
`removeEldestEntry` test is `size() >= cacheSize`, so it holds at most 31
parsers (`util/JEPUtil.java:39-41`), and an editor with more distinct
formulas than that re-parses on every evaluation. Derived parameters also
rewrite their formula with `String.replace` on every sample
(`logger/ecu/definition/EcuDerivedParameterConvertorImpl.java:53-68`). All
**MF-25**.

### 7.3 Syntax and numeric rules

From the jar (read, never run) and `JEPUtil`'s set-up:

- **Variables.** Only `x` (case-sensitive), in the `x` overload. Undeclared
  names are a parse error, since `allowUndeclared` is false
  (`JEP.<init>@14-16`); so `X`, `pi` and `e` are errors (no
  `addStandardConstants()`, `util/JEPUtil.java:49-53`).
- **Implicit multiplication is off** (`JEP.<init>@24-26`): `2x` is an error.
- **Errors give NaN.** `parseExpression` records a syntax error instead of
  throwing (`JEP.parseExpression@33-68`); `getValueAsObject` returns null
  when there is an error or evaluation throws (`JEP.getValueAsObject@0-15,35-108`);
  `getValue` returns NaN for null and for a complex result with a non-zero
  imaginary part (`JEP.getValue@0-42`). Nothing in `JEPUtil` checks
  `hasError()`, so a bad formula is not reported: every cell shows NaN, and
  `validate` (6.5) passes it.
- **Literals** are decimal only: integer, decimal and floating-point with an
  exponent; no hexadecimal (the `BitWise` Javadoc warns that `0x18` "will
  cause a parsing error", `util/BitWise.java:53-54`).
- **Operators** (`ParserConstants` token images,
  `ParserConstants.<clinit>@110-248`): `+ - * / % ^`, unary `-`
  and `!`, `== != < > <= >=` (1 or 0), `&& ||`, plus `=`, `;`, `.`, `^^`,
  `[ ]` that no definition uses. `^` is **power**, not exclusive-or.
- **Functions** (`JEP.addStandardFunctions@0-425`): `sin cos tan asin acos
  atan atan2 sinh cosh tanh asinh acosh atanh log ln exp pow sqrt abs mod sum
  rand if str binom`, plus `BitWise` from `JEPUtil`
  (`util/JEPUtil.java:50`). The complex functions (`re`, `im`, `arg`, `cmod`,
  `complex`, `polar`, `conj`) come from `addComplex()` (`JEP.addComplex@0-138`),
  which `JEPUtil` never calls. There is **no** `round`, `floor`, `ceil`,
  `min` or `max`.

Precedence (`-2^2` is −4, `2^3^2` is 512), `%` (Java `drem`,
`Modulus.run@47-52`), `log` (base 10 via `ln`, `log(0)` NaN,
`Logarithm.log@36-57`), `if` with three and four arguments
(`If.evaluate@139-199`), `sum`'s order (`Sum.run@23-58`), division by zero (IEEE: ±∞ or NaN, never an exception) and the always-NaN
`binom` are documented with bytecode offsets in
`crates/rr-core/src/expr.rs` (*Precedence*) and
`crates/rr-core/src/expr/eval.rs` (*Numeric semantics*), and pinned by
`fixtures/kat/expr-jep-precedence.toml` and
`fixtures/kat/expr-bitwise-functions.toml`.

### 7.4 `BitWise(mask, value, op)`

`util/BitWise.java:64-110`. Exactly three arguments
(`util/BitWise.java:66-68`); popped in reverse, so the first is the "mask",
the second the value, the third the operation (`util/BitWise.java:81-83`).
If the mask is a `Double` (always, unless complex), each argument becomes an
`int` with `Double.intValue()` (truncation toward zero, NaN → 0, saturation
at `±2^31`) and (`util/BitWise.java:85-105`):

| `op` | Result | Note |
|------|--------|------|
| 1 | `value & mask` | |
| 2 | `value \| mask` | |
| 3 | `value ^ mask` | exclusive-or |
| 4 | `value << mask` | the **first** argument is the shift distance; Java uses only its low 5 bits |
| 5 | `value >> mask` | arithmetic |
| 6 | `value >>> mask` | logical |
| 7 | `~value` | `mask` required but ignored |
| other | 0 | `util/BitWise.java:102-103` |

The `int` result is pushed back as a `Double` (`util/BitWise.java:105`), so
every result is in the signed 32-bit range. A non-`Double` mask throws
"Invalid parameter type" (`util/BitWise.java:107-108`), which JEP turns into
NaN (7.3). `BitWise` exists only in the `x` overload (7.2, MF-25).

*Worked examples (hand-computed).* `BitWise(24, 60, 1)`: `0011 1100 & 0001
1000` = `0001 1000` = **24**. `BitWise(4, 240, 5)`: 240 `>> 4` = 240 / 16 =
**15**. `BitWise(1, 2147483648, 1)`: 2 147 483 648 = 2^31 saturates to
2 147 483 647 = `7FFFFFFF`, `& 1` = **1**, although bit 0 of 2^31 is 0: a
`uint32` raw value of `2^31` or more is masked wrongly.

### 7.5 What the definitions use

The pinned definition corpus (730 distinct formulas) uses linear scalings
and their `to_byte` inverses, offsets, division by `x`, unary minus, exponent
literals, `+-` and `*-` operand prefixes, `%` and `if(...)`, and derived
logger parameters; it has **no `^`, no `BitWise` and no function other than
`if`** (`fixtures/kat/expr-corpus.toml`, notes "THE SPREAD"; formula text is
not committed, D-028). Legacy's own test definitions use only linear forms
(`legacy/src/test/definitions/scalingbase_test.xml:4-8`).

## 8. Display formatting

A cell's text is `DecimalFormat.format(realValue)` with the scale's `format`
pattern (`maps/DataCellView.java:279-306`; pattern applied at `:284`), in the
default locale's symbols. One `static` `DecimalFormat` is shared by every cell
of every table (`maps/DataCellView.java:45`). `DecimalFormat` rounds
`HALF_EVEN` by default, so with the default `#.##` (`maps/Scale.java:38`) the
exact binary values 2.125 and 2.375 show as **2.12** and **2.38** (ties go
to the even last digit). Tooltips use `#,##0.##########`, compare-percent
views `#,##0.0%` (`maps/DataCellView.java:46-48,308-325`). Formatting changes
only the text: edits, increments and the clipboard work on the real value,
and copying a table writes each real value with `0.0#################` in the
default locale (`maps/Table.java:329-341`; `util/NumberUtil.java:31-35,64-66`),
so a comma-decimal locale copies `1,5`. A NaN shows as the locale's NaN
symbol in a cell and as `NaN` in the clipboard.

## 9. Mapping to the port

How the behaviour above lands in rr-core and rr-defs. Pointers, not
decisions; checked against the crates' own module docs
(`crates/rr-core/src/encoding.rs`, `encoding/legacy.rs`, `encoding/codec.rs`,
`encoding/table.rs`, `encoding/float.rs`, `expr.rs`, `expr/eval.rs`,
`conversion.rs`; `crates/rr-defs/src/romraider/scaling.rs`), not re-derived.

- **Storage.** `Storage` covers `U8..U64`, `I8..I64`, `U24`/`I24`, `U20`,
  `F32`/`F64`; 24- and 64-bit widths decode (MF-02 fixed); a missing
  `storagetype` on a non-switch table is `MISSING_STORAGE_TYPE`. Refused
  where legacy accepted: `movi20s`, bare numeric codes (`2`, `20`, `99`),
  `UINT8` (`encoding/legacy.rs` docs). Deliberate.
- **`MOVI20`.** `U20` reads the same three bytes and sign-extends bit 19
  (`U20_BIT19_SIGN_EXTENDS`, decided in US-RR-19 to match legacy), and
  writes read-modify-write, keeping the low nibble that legacy overwrote
  (MF-64 fixed). **Divergence to decide:** the port ignores the `s` bit,
  so a `movi20` table over a `MOVI20S` instruction decodes `imm20` where
  legacy showed `imm20 << 8` (LEG-ENC-03).
- **Byte order.** Per encoding, never static (MF-04, MF-52 fixed); the
  rule is the memory model's, else the table's, else big (rr-defs test
  `byte_order_is_the_memmodel_s_else_the_table_s_else_big`). **Gap:** the
  float hack of 2.2 is not reproduced, so a float table marked `little`
  under no `<memmodel endian>` decodes little-endian in the port and
  big-endian in legacy. Definitions written against legacy may depend on
  it ("corrects improperly defined float table endian in legacy definition
  files"); LEG-ENC-04 must settle it before float tables are trusted.
- **Masks.** `BitField` applies to the decoded value as legacy did, can
  sign-extend (EC-06), refuses a mask wider than the storage (legacy
  clamped), and writes read-modify-write within the mask in the right byte
  order (MF-62 fixed); a value outside the field is an error.
- **Bosch subtract.** The table codec implements 4.1 exactly, in `i128`,
  and refuses a table whose derived cells do not fit instead of truncating
  (MF-65); float and `U20` cannot use the layout.
- **3D order.** rr-defs carries `swap_xy`, `flip_x`, `flip_y`, `skip_cells`
  from the definition (`crates/rr-defs/src/resolve.rs`); the placement rules
  of 4.2, crossed flip names included, are for the table model to apply
  (LEG-ENC-14).
- **Ranges.** `Encoding::raw_range` comes from the encoding alone, float
  `−MAX..MAX` both ways (MF-15 fixed); `Conversion::real_range` takes the
  extrema over the raw range (MF-13 fixed).
- **to_real.** `Conversion::to_real` returns an error for a NaN or infinite
  result instead of displaying it (EC-07, EC-08); a formula that does not
  parse is a load error, not NaN everywhere.
- **to_byte. Deliberately different.** rr-defs checks a given `to_byte`
  against `expression` over the raw range (`CNV0005` warning,
  `to_raw_mismatch`) and edits always use the inverse of `expression`:
  exact for linear forms, a bounded solve that refuses non-monotonic forms
  otherwise (MF-05 fixed, MF-50 moot). Rounding of the given `to_byte`
  in that check is half up, as legacy (`conversion/inverse.rs`,
  `round_half_up`). Out-of-range and non-finite results are refused, never
  clamped or wrapped (EC-08; MF-61 fixed).
- **JEP.** One dialect (the superset of the two), parsed once at load, no
  cache, no lock (MF-25 fixed). Precedence, `%`, `log`, `if`, `BitWise` and
  its `int` saturation follow legacy on purpose; `pi`, `e` and other names
  parse as parameter references and a ROM scaling naming anything but `x`
  is `CONVERSION_PARAM_REF`.
- **Display.** `DisplayFormat` keeps the pattern verbatim; rendering it
  (`HALF_EVEN`, locale) is the editor's job (LEG-ENC-17).
- **Not yet ported.** Increment, multiply and interpolation (MF-14, MF-39),
  the user-level and `Checksum Fix` save rules (MF-19) and overlap refresh
  (MF-03, EC-01) belong to the editor stories.

## Ledgered issues

Problems first found while cataloguing this page, each since entered in
PROJECT.md's Must-Fix ledger (`MF-nn`) or matched to an existing entry or
edge case (US-RR-12-3). The `UL-ENC-nn` IDs are kept here only so the
tasks and fixtures that cite them still resolve; new text cites the ledger
ID.

| ID | Ledger | Problem | Source |
|----|--------|---------|--------|
| UL-ENC-01 | MF-61 | **`uint32` values of `2^31` and above.** Saving casts the raw `double` to `int`, which saturates: a `uint32` (or 4-byte Bosch-subtract) raw value of `2^31` or more is written as `7FFFFFFF`, reachable through undo, interpolation and the Bosch cascade. Editing goes through `(int) Math.round`, which wraps instead: 3 000 000 000 becomes −1 294 967 296 and is clamped to 0. Either way a `uint32` cell cannot be set above `2^31 − 1` | `maps/DataCell.java:261,447-449,530`; `maps/Table.java:635` |
| UL-ENC-02 | EC-08 | **Non-finite `to_byte` results written as numbers.** NaN rounds to 0, +∞ to −1 (low bits of `Long.MAX_VALUE`), −∞ to 0, each then clamped silently; a float table writes the NaN. Related to EC-08 | `maps/DataCell.java:443-452,491-498` |
| UL-ENC-03 | MF-62 | **Little-endian masked write corrupts.** The mask is cut into bytes most-significant first while the value bytes are in table order, so a multi-byte little-endian masked cell clears the wrong byte and ORs the field into an uncleared one (worked example in 3.2) | `maps/DataCell.java:264-294`; `xml/RomAttributeParser.java:193-224` |
| UL-ENC-04 | MF-62 | **Masked write is not masked.** The shifted value is ORed in whole: a signed table's field (range: the full width), a split mask (range `2^popcount − 1`) or any path past the clamp spills into neighbouring bits; reading never sign-extends a field | `maps/DataCell.java:105-139,264-294`; `xml/RomAttributeParser.java:124-131` |
| UL-ENC-05 | MF-63 | **Bitwise-switch undo flips other flags.** A bit cell's range and `originalValue` come from a first read of the whole storage word, before its bit mask is set, so undo writes the whole byte shifted into the bit's position (byte `2A`, bit 3 becomes `72`, worked example in 3.3) | `maps/DataCell.java:77-84,368-371,547-549`; `maps/TableBitwiseSwitch.java:49-51` |
| UL-ENC-06 | MF-64 | **`MOVI20` round trip.** Read takes the `<< 8` from the instruction's `s` bit, write sets the `s` bit from the storage type, so a mismatched table changes the instruction's type when saved; `movi20s` drops the low 8 bits of a written value and both drop bits above 20 silently; a little-endian table reads garbage; the 3-byte stride cannot address consecutive 4-byte instructions; a cell at image offset 0 throws an uncaught `IllegalArgumentException` | `xml/RomAttributeParser.java:137-158,181-191,193-236`; `maps/DataCell.java:169-175,271-275`; `maps/Rom.java:312-327` |
| UL-ENC-07 | MF-05, MF-65 | **Search and Bosch limits.** The binary search (MF-05) also starts a signed table at 127 instead of 0, never keeps candidates in range, and returns only integers, even for a float table; Bosch-subtract values are clamped to the storage range though `2^(8W)` and negative values are valid | `maps/Scale.java:87-139`; `maps/DataCell.java:105-139,193-234` |
| UL-ENC-08 | MF-66 | **Scaling check is one point and NaN-blind.** `validate` compares `to_byte(expression(5))` with 5 only, and a NaN difference passes | `maps/Scale.java:63-77` |
| UL-ENC-09 | MF-66 | **`X` is identity for editing, NaN for display.** `setRealValue` treats an expression that trims to `x` in any case as identity, but JEP's variable is lower-case `x` only, so `expression="X"` displays NaN while editing writes the typed number | `maps/DataCell.java:436-439`; `util/JEPUtil.java:53` |
| UL-ENC-10 | MF-66 | **Errors in formulas are silent.** A formula that does not parse, or names anything but `x`, evaluates to NaN in every cell; `JEPUtil` never checks `hasError()` and `validate` lets NaN pass | `util/JEPUtil.java:46-60`; `maps/Scale.java:63-77` |

Ledger entries this page cross-references: **MF-02** (unsupported widths
read and write as zero), **MF-03** (overlap key `index × storageType`),
**MF-04** (static memory-model endian), **MF-05** (binary-search inverse),
**MF-06** (signedness inheritance), **MF-12** (3D data field hidden),
**MF-13** (`getMaxReal` from the largest raw value), **MF-14** (raw-space
interpolation), **MF-15** (float range swapped), **MF-19** (`Checksum Fix`
by name), **MF-25** (two JEP dialects, global cache), **MF-39** (multiply
through a string, doubling increment), **MF-40** (popup from model code);
edge cases **EC-01**, **EC-02**, **EC-06**, **EC-07**, **EC-08**. The table
above adds **MF-61** to **MF-66** and files non-finite `to_byte` results
under **EC-08**; section 6.1 adds **MF-86** (edit text parsed leniently).

## Fixtures to write

Stable IDs for US-RR-12-2. Each becomes a KAT file under `fixtures/kat/`
(format: `fixtures/README.md`), a definition-reader KAT under
`fixtures/defs-kat/romraider/`, or a test where noted (the KAT format has no
encode shape yet, so write-side cases are rr-core tests until it does).
Expected values are hand-computed from this page (D-024), never from running
Java. Where a fixture pins a legacy bug, the expected result is the port's
fixed behaviour and the legacy result goes in its `notes`.

| ID | Kind | What it pins down | Section |
|----|------|-------------------|---------|
| LEG-ENC-01 | KATs (exist) `fixtures/kat/{u,i}{8,16,24,32,64}-{be,le}.toml` | Integer read per width, order and sign, including `FF 38` of 1.2; 24/64-bit decode where legacy read 0 (MF-02) | 1.1, 1.2 |
| LEG-ENC-02 | KAT (exists) `fixtures/kat/u20-movi20.toml` + rr-core test | `MOVI20` decode incl. the `01 80 FE 0C` example (−459 252); encode keeps the low nibble and the opcode byte (MF-64) | 1.4 |
| LEG-ENC-03 | Test (rr-defs / rr-core) + decision | `movi20` table over a `MOVI20S` instruction (`81` byte): legacy −117 568 512, port today −459 252; decide, then pin. `movi20s` storage refused | 1.4, 9 |
| LEG-ENC-04 | KAT `fixtures/defs-kat/romraider/float-endian.toml` + decision | Float table `endian="little"` with no `<memmodel endian>` (legacy: big, `3F800000` = 1.0); with `<memmodel endian="little">`; two ROMs open at once decode independently (MF-04) | 2.2, 2.3 |
| LEG-ENC-05 | KATs (exist) `fixtures/kat/f32-{be,le}.toml`, `f64-{be,le}.toml` + rr-core test | Bit-exact float round trip incl. NaN payload, ±Inf, subnormal (`0000803F`); edit of a special float refused unless allowed (EC-07) | 1.5 |
| LEG-ENC-06 | rr-core test (`encoding`) | Masked write, `uint16` mask `00F0`, big (`A7 35` → `A7 55`) and little (`35 A7` → `55 A7`, legacy `75 07`, MF-62); value wider than the field refused (MF-62); signed field sign-extends | 3.1, 3.2 |
| LEG-ENC-07 | KAT (exists) `fixtures/kat/bitfield-u16-le.toml` | Masked read applies to the decoded value | 3.1 |
| LEG-ENC-08 | rr-core / editor test | Bitwise switch: byte `2A`, bit 3 reads 1; setting and undoing it changes only bit 3 (legacy undo: `72`, MF-63) | 3.3 |
| LEG-ENC-09 | KAT `fixtures/kat/bosch-subtract-u8.toml` + rr-core test | Decode `10 20 30` → 160, 176, 208; encode `value[1]` = 180 → `14 1C 30`; last cell; all-zero cells = 256; an underivable table refused (MF-65) | 4.1 |
| LEG-ENC-10 | rr-core test (`conversion`) | Rounding: `x/0.5` at 10.25 → 21, at −10.25 → −20 (half up); 20 000 on `int8` refused (legacy clamps to 127) | 6.2 |
| LEG-ENC-11 | rr-core test (`conversion`) | No `to_byte`, `x*0.5` on `uint8`, input 10.6 → 21 on every call (legacy 22 then 21, MF-05); decreasing and non-monotonic forms; `uint32` and float storage (MF-05) | 6.3 |
| LEG-ENC-12 | rr-core test (`encoding` / `conversion`) | `uint32` raw 3 000 000 000 and `C0000000` round trip (legacy 0 and `7FFFFFFF`, MF-61); `to_byte="1/(x-3)"` at 3 refused (legacy 0 / −1, EC-08) | 1.3, 6.2, 6.4 |
| LEG-ENC-13 | rr-defs test (`to_raw_mismatch`) | A `to_byte` that agrees with `expression` only at raw 5, and one that is NaN there: `CNV0005` (legacy passes both, MF-66) | 6.5 |
| LEG-ENC-14 | KAT `fixtures/defs-kat/romraider/table-3d-order.toml` + table-model test | The five 3D placements of 4.2 (none, `flipy`, `flipx`, `swapxy`, `skipCells="1"`) over `0A..10`; the crossed flip names without `swapxy` | 4.2 |
| LEG-ENC-15 | KATs (exist) `fixtures/kat/expr-jep-precedence.toml`, `expr-bitwise-functions.toml`; add cases | `BitWise(24, 60, 1)` = 24, `BitWise(4, 240, 5)` = 15, `BitWise(1, 2147483648, 1)` = 1; `X`, `2x` (implicit multiplication), `0x18` refused at parse | 7.3, 7.4 |
| LEG-ENC-16 | rr-core test (`expr` / `conversion`) | A ROM scaling naming `pi`, `X` or a parameter: a load error (`CONVERSION_PARAM_REF`), not NaN in every cell (MF-66); a NaN/∞ `to_real` result is a `ToRealError` | 5, 7.3 |
| LEG-ENC-17 | Editor test (later) | `format="#.##"`: 2.125 → `2.12`, 2.375 → `2.38` (`HALF_EVEN`); default format; the format never changes the stored value | 8 |
| LEG-ENC-18 | Editor test (later) | Two tables over the same bytes with different widths (one float, one `MOVI20` at `i > 0`): editing one refreshes the other (MF-03, EC-01) | 1.1 |
| LEG-ENC-19 | KAT `fixtures/defs-kat/romraider/static-data.toml` | Static `<data>` cells are raw values shown through `expression`; non-numeric `<data>` text kept as text | 4.3 |
