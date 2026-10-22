# settings.xml: legacy schema (for the importer)

What the Java RomRaider keeps in `settings.xml`: where the file lives, how it
is loaded and saved, its encoding, every element and attribute it reads and
writes with types and defaults, and what is not in it. Written from the
source only (the Java was never run, D-024). It is the spec for the one-time
importer of the app shell (US-RR-59, ARCHITECTURE.md "Settings"), which reads
this file into the port's typed TOML settings and never modifies it. Part of
the legacy behaviour catalogue, story US-RR-12, task US-RR-12-9.

What individual settings do is described where they act: the definition
file list and user levels in [`definitions.md`](definitions.md) sections 1.1
and 5; the logger definition, protocol and module in
[`logger.md`](logger.md) section 1; the log directory, time mode, number
format and file-logging switch in [`csv.md`](csv.md); the SSM link in
[`ssm.md`](ssm.md).

## How to read this page

- **References.** Source references are relative to
  `legacy/src/main/java/com/romraider/`, the convention the PROJECT.md
  Must-Fix ledger uses. `Unmarshaller:N` is short for
  `xml/DOMSettingsUnmarshaller.java:N` (the reader) and `Builder:N` for
  `xml/DOMSettingsBuilder.java:N` (the writer). Files elsewhere in `legacy/`
  carry the `legacy/` prefix. Every reference is `file:line` or
  `file:first-last`.
- **Bugs.** Where legacy behaviour is wrong or fragile, the page says so and
  cites the Must-Fix ledger entry (`MF-nn`) or edge case (`EC-nn`) of
  PROJECT.md. Do not port it as intended. A sentence that calls legacy
  behaviour a bug always carries its ledger ID (`cargo xtask
  check-legacy-docs`, rule 7). [Ledgered issues](#ledgered-issues) maps the
  problems first found on this page (`UL-SET-nn`, IDs that tasks and
  fixtures still cite) to their ledger entries.
- **Three defaults.** A value can come from three places, and they often
  differ (section 2.2): the `Settings` field initialiser (**field**, used
  when the element is absent), the reader's fallback (**reader**, used when
  the element is present but the attribute is absent or `NaN`), or the file.
  Legacy always writes every attribute, so the reader fallback only matters
  for hand-edited or truncated files.
- **JDK behaviour.** Some rules come from the Java class library rather than
  `legacy/` (XML serialisation, default charset). They are marked *(JDK)*.

### Source files

| Alias | Path under `legacy/src/main/java/com/romraider/` | Role |
|-------|---------------------------------------------------|------|
| `util/SettingsManager.java` | same | Where the file is, load, save |
| `xml/DOMSettingsUnmarshaller.java` | same | Reader: every element and attribute, reader defaults |
| `xml/DOMSettingsBuilder.java` | same | Writer: element order, attribute values, encoding |
| `xml/DOMHelper.java` | same | Attribute parsing (`NaN` rule, number and boolean parsing) |
| `Settings.java` | same | Field defaults, clamps, side effects of setters (locale), static fields |
| `swing/SettingsForm.java`, `editor/ecu/ECUEditor.java`, `logger/ecu/EcuLogger.java` | same | When settings are saved, the reset button |
| `logger/external/phidget/interfacekit/io/IntfKitSensor.java` | same | Phidget sensor entries |

## 1. Where the file lives; load and save

### 1.1 Location (MF-35)

`SettingsManager` looks in two places (`util/SettingsManager.java:48-52,72-84`):

1. `<working directory>/settings.xml`, if it exists. Then that directory is
   also where the file is **saved**.
2. Otherwise `~/.RomRaider/settings.xml` (`user.home` + `/.RomRaider`).

A `settings.xml` in whatever directory RomRaider was started from therefore
silently replaces the user's settings, and later saves go there
(`util/SettingsManager.java:76-79,121-123`; **MF-35**).

### 1.2 Load

On first use of `getSettings()` (`util/SettingsManager.java:57-62,72-110`):

- File missing (in both places) or zero length: an information dialog
  "Settings file not found. Using default settings." and all field defaults
  (`util/SettingsManager.java:87-105`;
  `legacy/i18n/com/romraider/util/SettingsManager.properties:1`).
- Otherwise the file is parsed with a non-validating DOM parser and handed to
  the reader (`util/SettingsManager.java:88-94`). **Any** other exception (not
  well-formed XML, a non-numeric number attribute, an empty `switchid`, a
  `locale` without `_`) is rethrown as `RuntimeException`, so RomRaider does
  not start (`util/SettingsManager.java:106-108`; MF-76).
- The root element's name is not checked; only its child elements are read
  (`Unmarshaller:42-72`).

### 1.3 Save

`save` creates the directory (one level, `mkdir`), writes the whole file in
place with a `FileWriter`, and only afterwards checks that the length is not
zero (`util/SettingsManager.java:116-132`). A crash or full disk mid-write
leaves a truncated or empty file, which the next start treats as missing
(1.2) and so loses every setting (**MF-35**, EC-13). Saves happen:

- when the editor exits (`editor/ecu/ECUEditor.java:302-309`), after copying
  in the window state;
- when the logger exits (`logger/ecu/EcuLogger.java:1940-1962`), after
  copying in its window state, port, tab, gauge style and plugin ports;
- when the settings dialog is confirmed or reset (`swing/SettingsForm.java:916-931,1055`).
  Reset keeps only the definition files, last image and repository
  directories, user level, logger definition, debug level, profile and
  output directory.

Many fields are `static` but set through instance setters
(`Settings.java:212-215,222,234-235,239-240`; **MF-35**), so a second
`Settings` object (the reset button builds one) changes the live values of
the first.

## 2. Reading rules

### 2.1 Names and parsing

- Element names are compared **ignoring case** (`equalsIgnoreCase`
  throughout the reader, e.g. `Unmarshaller:50-69`); attribute names are
  case-sensitive (`getNamedItem`, `xml/DOMHelper.java:48-51`). Unknown
  elements and attributes are ignored. An element that appears twice is read
  twice and the later wins, except `ecudefinitionfile`, which appends.
- An attribute whose value is exactly `NaN` counts as absent
  (`xml/DOMHelper.java:48-51`).
- Integers use `Integer.parseInt`; booleans `Boolean.parseBoolean`, so only
  `true` (any case) is true and anything else is false
  (`xml/DOMHelper.java:53-63`). Some booleans are read as strings and then
  parsed (`Unmarshaller:131-180`); the result is the same.
- Only the `<icons>` scale values catch a bad number and fall back to the
  default (`Unmarshaller:407-417`). Every other bad number aborts start-up
  (1.2).

### 2.2 Field default versus reader default

Where they differ, a file that lacks an **attribute** gets the reader value,
and a file that lacks the **element** gets the field value. The differences:

| Setting | Field default | Reader default | Source |
|---------|---------------|----------------|--------|
| `window/size` | 800 wide × 600 high | 600 wide × 800 high (swap, 4.1) | `Settings.java:171`; `Unmarshaller:86-87` |
| `window/location` | centred on screen | 0, 0 | `Settings.java:285-290`; `Unmarshaller:91-92` |
| `files/image_dir path` | `images` | `ecu_defs.xml` (copy-paste) | `Settings.java:180`; `Unmarshaller:113` |
| `files/def_dir path` | `definitions` | `./` | `Settings.java:179`; `Unmarshaller:115` |
| `options/debug` | false | true | `Settings.java:184`; `Unmarshaller:135` |
| `options/tableclickcount` | 1 | 2 | `Settings.java:205`; `Unmarshaller:144` |
| `options/savedebugtables` | true | false | `Settings.java:186`; `Unmarshaller:156` |
| `options/displayhightables` | true | false | `Settings.java:187`; `Unmarshaller:159` |
| `tabledisplay/font size` | 11 | 12 | `Settings.java:190`; `Unmarshaller:195-197` |
| `tabledisplay/colors/*` | per colour (4.4) | 155, 155, 155 per missing component | `Settings.java:192-204`; `Unmarshaller:365-369` |
| `logger/size` | 1000 × 600 | 600 wide × 1000 high (swap) | `Settings.java:224`; `Unmarshaller:279-280` |
| `logger/size divider` | 400 | 500 | `Settings.java:237`; `Unmarshaller:281` |
| `logger/location` | 0, 0 | 150, 150 | `Settings.java:225`; `Unmarshaller:284-285` |
| `logger/filelogging path` | the user's home directory | `""` (logs then go to the filesystem root, csv.md section 2) | `Settings.java:215`; `Unmarshaller:298` |
| `logger/filelogging active` | false | true | `Settings.java:217`; `Unmarshaller:300` |

## 3. Encoding

The writer asks the XML serialiser for `ISO-8859-1` and indentation of 4
(`Builder:75-80`), so the file starts with an XML declaration naming
ISO-8859-1 *(JDK)*. But it writes through a `FileWriter`
(`Builder:72,82-83`), which encodes characters with the **platform default
charset** *(JDK)*, not ISO-8859-1. The reader parses a byte stream
(`util/SettingsManager.java:78,88-92`) and so decodes it as ISO-8859-1, as
declared *(JDK)*:

- **Windows (windows-1252 default, Java 8 era)**: Latin-1 letters in a path
  (`é`, `ü`) are the same bytes in both, so they survive.
- **Linux and macOS (UTF-8 default), and any Java 18+ (UTF-8 default)**: `é`
  is written as two bytes `C3 A9`, read back as `Ã©`, written next time as
  four bytes. Every save corrupts non-ASCII paths further (MF-73).

The importer must therefore not trust the declaration: decode as declared,
and if the result contains UTF-8 sequences mis-read as Latin-1 (`Ã`, `Â`
followed by a continuation character), decode as UTF-8 instead and say so.

## 4. Schema

The writer emits one root element `settings` with seven children in this
order (`Builder:50-68`); the reader accepts them in any order
(`Unmarshaller:47-70`). Tables give: element and attribute; type; field
default; what it is; and whether the port imports it (**Import**: carried
into a port setting; **Hint**: carried as a starting value the user can
change, such as a last directory; **Drop**: not imported, with the reason).

### 4.1 `<window>` (editor main window)

| Element / attribute | Type | Field default | Meaning | Read | Written | Port |
|---------------------|------|---------------|---------|------|---------|------|
| `maximized value` | bool | false | Editor window maximised | `Unmarshaller:82-83` | `Builder:98-100` | Hint |
| `size x y` | int, int | 800 × 600 | **`x` is the height and `y` the width**: written `x`=height, `y`=width, read back as `Dimension(width=y, height=x)` | `Unmarshaller:85-87` | `Builder:103-106` | Hint (swap!) |
| `location x y` | int, int | centred | Window position, screen pixels | `Unmarshaller:89-92` | `Builder:109-112` | Hint |
| `splitpane location` | int | 150 | Divider between the table tree and the desktop | `Unmarshaller:94-95` | `Builder:115-117` | Hint |

Saved from the live window at editor exit (`editor/ecu/ECUEditor.java:302-306`).

### 4.2 `<files>`

| Element / attribute | Type | Field default | Meaning | Read | Written | Port |
|---------------------|------|---------------|---------|------|---------|------|
| `image_dir path` | path | `images` | Last directory a ROM image was opened from | `Unmarshaller:112-113` | `Builder:126-128` | Hint |
| `def_dir path` | path | `definitions` | Last directory a definition was chosen from | `Unmarshaller:114-115` | `Builder:130-132` | Hint |
| `repository-dir path` | path | `repositories` | Last repository directory | `Unmarshaller:116-117`; `Settings.java:77-78` | `Builder:135-137` | Hint |
| `ecudefinitionfile name` | path, repeated | none | The ordered ECU definition file list; order decides which definition matches first (`definitions.md` 1.1) | `Unmarshaller:109-110`; `Settings.java:178,312-314` | `Builder:140-146` (absolute paths, list order) | **Import**, keep order |

Paths are written absolute (`getAbsolutePath`) and read as given, so a
relative path in a hand-edited file is relative to the working directory.
There is no "last ROM" or recent-files list anywhere in settings (section 5).

### 4.3 `<options>`

| Element (`value` attribute) | Type | Field default | Meaning | Read | Written | Port |
|-----------------------------|------|---------------|---------|------|---------|------|
| `obsoletewarning` | bool | true | Warn when opening a ROM whose definition is marked obsolete | `Unmarshaller:131-132`; `editor/ecu/ECUEditor.java:404` | `Builder:155-157` | Import |
| `calcconflictwarning` | bool | true | "Warn me when real and byte value calculations conflict": shown in the settings dialog but **never read** by the editor | `Unmarshaller:137-138`; `swing/SettingsForm.java:94,244`; `legacy/i18n/com/romraider/swing/SettingsForm.properties:9` | `Builder:160-162` | Drop (the port always reports `to_byte` mismatches, `encoding-scaling.md` 6.5) |
| `debug` | bool | false | Editor debug mode | `Unmarshaller:134-135` | `Builder:165-167` | Drop (diagnostics are always on in the port) |
| `userlevel` | int, clamped 1–5 | 1 | User level (`definitions.md` 5.1) | `Unmarshaller:140-141`; `Settings.java:484-492` | `Builder:170-172` | **Import** |
| `tableclickcount` | int | 1 | Clicks to open a table | `Unmarshaller:143-144` | `Builder:175-177` | Hint |
| `tableclickbehavior` | int | 0 | 0 = open/close, 1 = open/focus | `Unmarshaller:146-147`; `Settings.java:506-510` | `Builder:180-182` | Hint |
| `tabletreesorted` | bool | false | Table tree alphabetical (else definition order) | `Unmarshaller:149-150`; `Settings.java:207`; `maps/Rom.java:118` | `Builder:185-187` | Hint |
| `version` | string | `x` | Version that last saved the file; a different running version shows the release notes | `Unmarshaller:152-153`; `editor/ecu/ECUEditor.java:141-143` | `Builder:190-192` (the running version) | Drop (record in the import report) |
| `savedebugtables` | bool | true | Save level-5 tables (`definitions.md` 5.1) | `Unmarshaller:155-156` | `Builder:195-197` | **Import** |
| `displayhightables` | bool | true | Show tables above the user level (`definitions.md` 5.1) | `Unmarshaller:158-159` | `Builder:200-202` | **Import** |
| `valuelimitwarning` | bool | true | "Warn when values exceed limits": shown in the settings dialog but **never read** by the editor | `Unmarshaller:161-162`; `swing/SettingsForm.java:130,445`; `legacy/i18n/com/romraider/swing/SettingsForm.properties:33` | `Builder:205-207` | Drop (record in the import report) |
| `coloraxis` | bool | false | Colour axis cells like data | `Unmarshaller:164-165`; `maps/DataCellView.java:145,165` | `Builder:210-212` | Hint |
| `showtabletoolbarborder` | bool | false | UI detail | `Unmarshaller:167-168` | `Builder:215-217` | Drop |
| `openromexpanded` | bool | true | Expand the ROM node on open | `Unmarshaller:170-171`; `editor/ecu/ECUEditor.java:392` | `Builder:220-222` | Hint |
| `alwaysopentableatzero` | bool | false | Open table windows at the desktop origin | `Unmarshaller:173-174`; `swing/MDIDesktopPane.java:68` | `Builder:225-227` | Drop |
| `defaultscale` | string: `Default`, `Metric`, `Standard` | `Metric` | Scaling category shown first: the editor's **unit-system choice** (`definitions.md` 5.2) | `Unmarshaller:176-177`; `Settings.java:152-154,256` | `Builder:230-232` | **Import** (map to the port's unit preference) |
| `scaleHeadersAndData` | bool | true | Apply the chosen scaling to axes as well as data | `Unmarshaller:179-180`; `maps/Table2D.java:123`; `maps/Table3D.java:346` | `Builder:235-237` | Import |

### 4.4 `<tabledisplay>`

| Element / attribute | Type | Field default | Meaning | Read | Written | Port |
|---------------------|------|---------------|---------|------|---------|------|
| `font face decoration size` | string, int (AWT style: 0 plain, 1 bold, 2 italic, 3 both), int | Arial, bold, 11 | Table cell font | `Unmarshaller:194-197` | `Builder:246-250` | Drop (egui fonts differ) |
| `cellsize width height` | int, int | 42 × 18 | Cell size in pixels | `Unmarshaller:199-201` | `Builder:253-256` | Drop |
| `colors/max min highlight select increaseborder decreaseborder axis warning livevalue curlivevalue` each `r g b` | int 0–255 ×3 | max 255,102,102; min 153,153,255; highlight 27,161,226; select 204,204,204; increase 255,0,0; decrease 0,0,255; axis 255,255,255; warning 255,0,0; live 0,255,0; current live 255,0,255 | Table colours | `Unmarshaller:211-251,365-369` | `Builder:259-321` | Hint (theme) |

### 4.5 `<logger>`

| Element / attribute | Type | Field default | Meaning | Read | Written | Port |
|---------------------|------|---------------|---------|------|---------|------|
| `logger locale` (attribute of the element) | `system` or `xx_YY` | `system` | Number format: `en_US` calls `Locale.setDefault(en_US)` during load, changing the whole JVM's number formatting (`csv.md` 7) | `Unmarshaller:256-258`; `Settings.java:222,875-895` | `Builder:327-328` | Drop (the port always writes `.`; record it so legacy CSVs are imported with the right decimal) |
| `autoConnectOnStartup value` | bool | true | Connect when the logger starts | `Unmarshaller:266-267`; `logger/ecu/EcuLogger.java:2115` | `Builder:331-333` | Import |
| `serial port refresh` | string, bool | `""`, false | Last serial port name (`COM3`, `/dev/ttyUSB0`); port-list refresh mode | `Unmarshaller:263-265` | `Builder:336-339`; `logger/ecu/EcuLogger.java:1943` | Hint (port names change; match by stable ID, EC-19c) |
| `protocol name` | string | `SSM` | Logger protocol: selects the definition section (`logger.md` 1.3) | `Unmarshaller:268-269` | `Builder:342-343` | **Import** |
| `protocol transport` | string | `ISO9141` | Transport ID, compared ignoring case | `Unmarshaller:270`; `Settings.java:240` | `Builder:344` | **Import** |
| `protocol module` | string | `ecu` | Target module name (`ecu`, `tcu`) | `Unmarshaller:271`; `Settings.java:234` | `Builder:345` | **Import** |
| `protocol fastpoll` | bool | true | Continuous (fast poll) mode (`ssm.md` 5) | `Unmarshaller:272`; `Settings.java:236` | `Builder:346` | Import |
| `protocol library` | path | `""` | J2534 pass-through DLL | `Unmarshaller:273`; `Settings.java:239` | `Builder:347` | Import |
| `maximized value` | bool | false | Logger window maximised | `Unmarshaller:275-276` | `Builder:351-353` | Hint |
| `size x y divider` | int ×3 | 1000 × 600, divider 400 | Logger window size, **`x` height, `y` width** as in 4.1; parameter-list divider | `Unmarshaller:278-281` | `Builder:356-360` | Hint (swap!) |
| `location x y` | int, int | 0, 0 | Logger window position | `Unmarshaller:283-285` | `Builder:363-366` | Hint |
| `tabs selected showlist` | int, bool | 0, true | Selected tab; parameter list shown | `Unmarshaller:287-289` | `Builder:369-372` | Hint |
| `definition path` | path | `""` | **The logger definition file** (`logger.md` 1.1) | `Unmarshaller:291-292` | `Builder:375-377` | **Import** |
| `profile path` | path | `""` | Last logger profile (`profile.xml`: selected parameters and their units) | `Unmarshaller:294-295` | `Builder:380-382` | **Import** |
| `filelogging path` | path | user's home | CSV output directory (`csv.md` 2) | `Unmarshaller:297-298` | `Builder:385-386` | **Import** |
| `filelogging switchid` | string, non-empty | `S20` | Switch that starts and stops file logging; empty aborts start-up | `Unmarshaller:299`; `Settings.java:216,648-651` | `Builder:387` | Import |
| `filelogging active` | bool | false | Whether that switch controls logging | `Unmarshaller:300` | `Builder:388` | Import |
| `filelogging absolutetimestamp` | bool | false | CSV time column mode (`csv.md` 5) | `Unmarshaller:301` | `Builder:389` | Import |
| `debug level` | string: `info`, `debug`, `trace` | `info` | log4j level | `Unmarshaller:303-304`; `logger/ecu/EcuLogger.java:352` | `Builder:393-395` | Drop |
| `plugins/plugin id port` | strings | none | Serial port per external-sensor plugin; empty IDs or ports skipped, values trimmed | `Unmarshaller:315-326,359` | `Builder:398-406` | Import (as device profiles; IDs are display-name based, MF-33) |
| `plugins/phidgets/phidget name number units expression format min max step` | string, int, strings, float ×3 | none; `number` -1 or no `name` skips the entry | Phidget InterfaceKit analogue input scaling | `Unmarshaller:327-357` | `Builder:407-424` (only inside `<plugins>`, MF-77) | Import (as device profile) |
| `gauge index` | int | 0 | Dashboard gauge style | `Unmarshaller:306-307` | `Builder:429-431` | Drop |
| `dyno car gear threshold units` | strings | `""`, `3`, `98`, `%` | Dyno tab: car from `cars_def.xml`, gear, WOT throttle threshold, throttle units | `Unmarshaller:309-313`; `Settings.java:263-278` | `Builder:434-439` | Import (dyno collector) |

Not saved although the logger has them: the "Logfile Text" (`csv.md` 2),
external-sensors-only mode, the ELM327 search flag
(`Settings.java:219-220,283`).

### 4.6 `<table-clipboard-format>`

| Element / attribute | Type | Field default | Meaning | Read | Written | Port |
|---------------------|------|---------------|---------|------|---------|------|
| `table-clipboard-format format-string` | `Default`, `Airboys`, `Custom` | `Default` | Clipboard layout for copying tables. `Default` and `Airboys` reset all four headers to built-in values and ignore the children | `Unmarshaller:371-381`; `Settings.java:54-74,821-835` | `Builder:446-447` | Import |
| `table`, `table1D`, `table2D`, `table3D` `table-header` | string | `[Table1D]⏎`, `""`, `[Table2D]⏎`, `[Table3D]⏎` | Header text per table kind, read only for `Custom`; the defaults contain the platform line separator | `Unmarshaller:383-397`; `Settings.java:63-66` | `Builder:450-466` | Import (with `Custom` only) |

### 4.7 `<icons>`

| Element / attribute | Type | Field default | Meaning | Read | Written | Port |
|---------------------|------|---------------|---------|------|---------|------|
| `editor-toolbar scale` | int (percent) | 50 | Editor toolbar icon scale; a bad number falls back | `Unmarshaller:406-411`; `Settings.java:80-88` | `Builder:476-477` | Drop |
| `table-toolbar scale` | int (percent) | 70 | Table toolbar icon scale | `Unmarshaller:412-417` | `Builder:480-481` | Drop |

## 5. What is not in settings.xml

- **No last ROM and no recent files.** Only the last *directory* of each
  file chooser (4.2).
- **Logger units per parameter** are in the logger profile, not here
  (`logger.md` 4.4). The logger has no unit-system setting; the editor's is
  `defaultscale`.
- **ECU definitions** themselves, logger definitions and `cars_def.xml` are
  separate files; settings hold only their paths (the car file is found next
  to the logger definition, `logger.md` 10).
- **Single-instance port** (TCP 23272, a constant,
  `EditorLoggerCommunication.java:35`; MF-36) and window layout of table
  frames (placed afresh at each open, `swing/MDIDesktopPane.java:62-85`) are
  not saved.

## 6. Example

*Hand-composed* to show the shape the writer produces (element order and
attribute values from section 4; exact whitespace, attribute order and the
declaration's `standalone` are the JDK serialiser's and are not significant).
A Linux user, editor window 1280 wide × 960 high, one definition file, logger
on SSM over K-line with a definition and an AEM wideband on `/dev/ttyUSB1`:

```xml
<?xml version="1.0" encoding="ISO-8859-1"?>
<settings>
    <window>
        <maximized value="false"/>
        <size x="960" y="1280"/>
        <location x="320" y="60"/>
        <splitpane location="150"/>
    </window>
    <files>
        <image_dir path="/home/user/roms"/>
        <def_dir path="/home/user/defs"/>
        <repository-dir path="/home/user/repositories"/>
        <ecudefinitionfile name="/home/user/defs/ecu_defs.xml"/>
    </files>
    <options>
        <obsoletewarning value="true"/>
        <calcconflictwarning value="true"/>
        <debug value="false"/>
        <userlevel value="3"/>
        <tableclickcount value="1"/>
        <tableclickbehavior value="0"/>
        <tabletreesorted value="false"/>
        <version value="1.1.0 1022 2026"/>
        <savedebugtables value="true"/>
        <displayhightables value="true"/>
        <valuelimitwarning value="true"/>
        <coloraxis value="false"/>
        <showtabletoolbarborder value="false"/>
        <openromexpanded value="true"/>
        <alwaysopentableatzero value="false"/>
        <defaultscale value="Metric"/>
        <scaleHeadersAndData value="true"/>
    </options>
    <tabledisplay>
        <font decoration="1" face="Arial" size="11"/>
        <cellsize height="18" width="42"/>
        <colors>
            <max b="102" g="102" r="255"/>
            <!-- min, highlight, select, increaseborder, decreaseborder,
                 axis, warning, livevalue, curlivevalue likewise -->
        </colors>
    </tabledisplay>
    <logger locale="system">
        <autoConnectOnStartup value="true"/>
        <serial port="/dev/ttyUSB0" refresh="false"/>
        <protocol fastpoll="true" library="" module="ecu" name="SSM" transport="ISO9141"/>
        <maximized value="false"/>
        <size divider="400" x="600" y="1000"/>
        <location x="0" y="0"/>
        <tabs selected="0" showlist="true"/>
        <definition path="/home/user/defs/logger_STD_EN_v370.xml"/>
        <profile path="/home/user/.RomRaider/profile.xml"/>
        <filelogging absolutetimestamp="false" active="false" path="/home/user/logs" switchid="S20"/>
        <debug level="info"/>
        <plugins>
            <plugin id="AEM" port="/dev/ttyUSB1"/>
        </plugins>
        <gauge index="0"/>
        <dyno car="" gear="3" threshold="98" units="%"/>
    </logger>
    <table-clipboard-format format-string="Default">
        <table table-header="[Table1D]&#10;"/>
        <table1D table-header=""/>
        <table2D table-header="[Table2D]&#10;"/>
        <table3D table-header="[Table3D]&#10;"/>
    </table-clipboard-format>
    <icons>
        <editor-toolbar scale="50"/>
        <table-toolbar scale="70"/>
    </icons>
</settings>
```

The plugin ID is whatever the data source reports; `AEM` is illustrative.
Imported, the editor window is 1280 wide and 960 high (4.1).

## 7. Mapping to the port

Pointers, not decisions. The port's settings are typed TOML under the
platform config directory, written atomically, read from one location
(ARCHITECTURE.md "Settings"; US-RR-59; closes MF-35).

- **Source.** The importer reads `~/.RomRaider/settings.xml` (or a file the
  user names), never a file in the working directory implicitly (MF-35), and
  never writes to it.
- **Robust read.** Unknown elements and attributes are reported, not fatal;
  a bad value falls back to the port default with a Warning per value, never
  an abort (MF-76). Element names match ignoring case, attributes exactly,
  `NaN` counts as absent, as in legacy.
- **Encoding.** Declared ISO-8859-1 with a UTF-8 fallback (section 3,
  MF-73).
- **Swaps and defaults.** `size x` is height and `y` width in both windows.
  Absent attributes take the legacy **reader** default only where that is the
  user-visible legacy behaviour; otherwise the port's own default (decide
  per row of 2.2, and record the decision in the importer).
- **What to import.** The Import rows of section 4: the definition file list
  in order, user level and the two level flags, `defaultscale`, the obsolete
  warning, logger protocol/transport/module/fast poll/J2534 library, logger
  definition, profile and output paths, file-logging switch and time mode,
  plugin ports and Phidget scalings as device profiles (stable IDs, MF-33),
  dyno settings. Hints seed UI state. Drops are listed in the import report so
  the user sees what was not carried over. Two editor options,
  `calcconflictwarning` and `valuelimitwarning`, were never used by legacy
  (4.3).
- **`locale`** is not a port setting (the port's CSV always uses `.`,
  csv.md 14), but it tells the CSV importer which decimal mark the user's old
  logs probably use.

## Ledgered issues

Problems first found while cataloguing this page, each since entered in
PROJECT.md's Must-Fix ledger (`MF-nn`) or matched to an existing entry or
edge case (US-RR-12-3). The `UL-SET-nn` IDs are kept here only so the
tasks and fixtures that cite them still resolve; new text cites the ledger
ID. (Location, non-atomic save and static fields are **MF-35**.)

| ID | Ledger | Problem | Source |
|----|--------|---------|--------|
| UL-SET-01 | MF-73 | The file declares ISO-8859-1 but is written with `FileWriter` in the platform charset; on a UTF-8 platform every save double-encodes non-ASCII characters in paths and names (`é` → `Ã©` → …), so settings with such paths degrade on each run | `xml/DOMSettingsBuilder.java:72,78`; `util/SettingsManager.java:78,88-92` |
| UL-SET-02 | MF-76 | Any malformed value (a non-numeric integer, an empty `switchid`, a `locale` without `_`) or a malformed file throws out of `load` and RomRaider does not start; only the icon scales fall back | `util/SettingsManager.java:106-108`; `xml/DOMHelper.java:53-59`; `Settings.java:648-651,886-891`; `xml/DOMSettingsUnmarshaller.java:407-417` |
| UL-SET-03 | MF-77 | Phidget sensor scalings are written only inside `<plugins>`, which is written only when at least one plugin port is set, so with no plugin ports the Phidget configuration is silently dropped on save | `xml/DOMSettingsBuilder.java:398-426` |
| UL-SET-04 | MF-77 (the fallbacks; the stored order itself is not a bug) | Window sizes are stored with `x` = height and `y` = width: a self-consistent convention the importer swaps back (LEG-SET-01). The reader's fallbacks follow the same swap (600 × 800, 600 × 1000), so they disagree with the field defaults (800 × 600, 1000 × 600) when an attribute is missing | `xml/DOMSettingsBuilder.java:103-106,356-358`; `xml/DOMSettingsUnmarshaller.java:85-87,278-280` |
| UL-SET-05 | MF-77 | Field and reader defaults disagree for the fifteen settings of section 2.2, so a file missing an attribute behaves differently from one missing the element; `image_dir` falls back to the file name `ecu_defs.xml`, and an empty `filelogging path` puts logs in the filesystem root | `xml/DOMSettingsUnmarshaller.java:113,135,144,156,159,281,284-285,298,300`; `logger/ecu/ui/handler/file/FileLoggerImpl.java:133-136` |

Ledger entries this page cross-references: **MF-35** (CWD wins, non-atomic
save, static fields), **MF-33** (plugin IDs from display names), **MF-36**
(single-instance port); edge cases **EC-13** (interrupted save) and **EC-19c**
(serial port re-enumeration). The table above adds **MF-76** and **MF-77**,
and **MF-73** (platform charset, shared with `csv.md` and
`definitions.md`).

## Fixtures to write

Stable IDs for US-RR-12-2. Each becomes a hand-written legacy settings file
(proposed `fixtures/public/settings/legacy-*.xml`, small, synthetic paths)
with the typed settings the importer must produce (US-RR-59 "hand-checked
fixture"), or a test where noted. Expected values are hand-derived from this
page (D-024), never from running Java.

| ID | Kind | What it pins down | Section |
|----|------|-------------------|---------|
| LEG-SET-01 | Fixture `legacy-full.xml` + importer test | The section 6 file: every Import row, list order of `ecudefinitionfile`, window `x`/`y` swap (1280 wide × 960 high), plugin port, `S20` switch | 4, 6 |
| LEG-SET-02 | Fixture `legacy-minimal.xml` | Root with only `<options><userlevel value="7"/></options>`: field defaults everywhere else, user level clamped to 5 | 2.2, 4.3 |
| LEG-SET-03 | Fixture `legacy-attrs-missing.xml` | Each element of 2.2 present with its attribute missing, and one attribute `NaN`: the importer's documented choice per row (legacy reader defaults in the notes) | 2.1, 2.2 |
| LEG-SET-04 | Fixtures `legacy-latin1.xml`, `legacy-utf8-mislabelled.xml` | A path with `é` and `ü`, once correctly in ISO-8859-1 and once as UTF-8 bytes under the ISO-8859-1 declaration: both import as the same text (MF-73) | 3 |
| LEG-SET-05 | Fixture `legacy-bad-values.xml` | `userlevel="abc"`, `switchid=""`, `locale="en"`, an unknown element: imports with Warnings and defaults, never an error (MF-76) | 1.2, 2.1 |
| LEG-SET-06 | Fixture `legacy-case.xml` | Element names in other case (`<Options>`, `<USERLEVEL>`) are read; attribute `Value` (wrong case) is not | 2.1 |
| LEG-SET-07 | Fixture `legacy-phidgets.xml` | `<plugins>` with plugin ports and two `<phidget>` entries (one with `number="-1"`, skipped): device profiles produced | 4.5 |
| LEG-SET-08 | Fixture `legacy-clipboard-custom.xml` | `format-string="Custom"` with four headers imported; `Airboys` ignores its children | 4.6 |
| LEG-SET-09 | Importer test (T0) | The source file's bytes and mtime are unchanged after import; a `settings.xml` in the working directory is not read unless named (MF-35) | 1.1, 7 |
| LEG-SET-10 | Fault-injection test (T6, US-RR-59) | Killing the process during a port settings save leaves the previous TOML intact (MF-35, EC-13) | 1.3 |
