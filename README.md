# Xenomorph

*Spots the foreign shapes in your XML.*

Compares the *shape* of two XML files and lists the differences as findings,
not as a line diff. One file is the reference (known to be right), the other
is the candidate you are working on. Values are never compared, only how the
documents are built and how values are written.

## What it reports

| Finding | Meaning |
|---|---|
| Wrong namespace | Same element name, different namespace URI. Prefixes are ignored. |
| Probably misnamed | An unknown element whose name is close to a missing one, with the spot where the names differ. |
| Wrong nesting level | A wrapper element is missing or extra, or a value sits higher than in the reference. |
| Text instead of child elements | One file has a plain value where the other has children. |
| Missing elements | Grouped per parent. Elements that are optional in the reference are marked. |
| Empty in candidate | The element is there but has none of the reference's children. |
| Unexpected elements | Grouped per parent. |
| Element order | One finding per parent, naming only the elements that are out of place. |
| Repeated where reference has one | The candidate repeats an element the reference has once per parent. |
| Not in every entry | In a repeated structure, an element is missing from some entries. |
| Attributes | Attribute names that differ. |
| Value format | Date pattern, time zone, timestamp precision, thousands separator, decimal separator, whole number against decimal, fraction digits, scientific notation, value type. Grouped per kind of difference. |
| Unset-looking values | `null`, `NaN` and similar written out as a value or inside a text. |

All occurrences of an element path are merged before comparing, so a list with
28 entries in one file and 600 in the other is not a difference in itself.

## Using the window

```bash
xenomorph reference.xml candidate.xml
```

Both arguments are optional; files can also be chosen with **Browse…** or by
typing a path and pressing Enter.

- Click a finding to see it in both files. Findings with several places have
  a list and **Previous** / **Next**.
- **Dismiss** hides one finding, **Dismiss all** a whole group, including
  findings of that group that appear later. **Show dismissed** brings them
  back into view so they can be restored. Dismissals last until the program
  is closed.
- Both files are watched. When one changes on disk the comparison runs again
  and each finding is marked **new**, still open, or **fixed**, compared with
  the baseline. A grouped finding that shrank says how much, for example
  "3 of 12 resolved".
- Choosing a different candidate file counts as a new version too: its
  findings are compared with the baseline in the same way. This suits
  generators that write a new file each run.
- The baseline is the first comparison after a reference was chosen.
  **Accept as baseline** makes the current state the new baseline; choosing
  a different reference starts over.

## Using it without the window

```bash
xenomorph reference.xml candidate.xml --report
```

Prints the findings as text and exits with 1 if there are any, 0 if the
shapes match, 2 on errors. `--output FILE` writes to a file instead; use that
on Windows, where the program has no console.

## Building

Tools are managed with [mise](https://mise.jdx.dev): Rust, Zig and
cargo-zigbuild. Zig does the linking, so no system C compiler is needed, and
it cross-compiles the Windows binary from Linux.

```bash
mise install
```

```bash
mise run test
```

```bash
mise run build
```

```bash
mise run build-windows
```

The results are single files:

- `target/x86_64-unknown-linux-gnu/release/xenomorph`
- `target/x86_64-pc-windows-gnu/release/xenomorph.exe`

On a machine with a normal C toolchain, plain `cargo build --release` works as
well; in that case delete `.cargo/config.toml`, which only redirects linking
to Zig.

The Linux binary loads the display libraries of the system at run time. Under
X11 it needs `libxkbcommon-x11` (package `libxkbcommon-x11-0` on Debian and
Ubuntu).

The icon is drawn in `assets/icon.svg`. After changing it, `mise run icon`
renders the PNG and ICO files that are embedded in the binaries.

## Layout of the code

- `src/shape.rs` reads a document into one node per element path.
- `src/types.rs` recognises value formats and describes format differences.
- `src/compare.rs` matches the two shapes and produces the findings.
- `src/session.rs` tracks baseline, new / fixed status and dismissals.
- `src/main.rs` and `ui/app.slint` are the window ([Slint](https://slint.dev)).

Built by Claude, credit taken by [Julian Kurz](https://j8z.dev).

## Licence

[MIT](LICENSE)
