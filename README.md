# lupasta

A desktop file browser that draws your filesystem as a single horizontal path instead of a
tree view. Each column lists the siblings of one folder on the path to the current selection;
the next columns preview what is inside the neighbouring folders. Everything is monospace text
on black, joined by orthogonal connectors.

Text colour encodes age, not file type: names modified recently are white and fade through
grey towards orange (on the selection path) or blue (in the previews) as they get older.

Built with Tauri 2, Rust and Svelte 5. It runs on Windows, and should run on macOS and Linux
(the Rust side is portable; only Windows has been tested).

## Running it

You need Rust, [Bun](https://bun.sh) and the platform webview (WebView2 on Windows, which
ships with Windows 11).

```sh
bun install
bun tauri dev              # opens the folder that contains your home directory
bun run dev:fixture        # opens the synthetic fixture used by the tests
bun tauri build            # release build and installer
```

On Windows the project is pinned to the GNU toolchain (`rust-toolchain.toml`), so a MinGW gcc
must be on `PATH` to compile the bundled SQLite; [WinLibs](https://winlibs.com) works
(`winget install BrechtSanders.WinLibs.POSIX.MSVCRT`). Delete `rust-toolchain.toml` to use the
MSVC toolchain instead.

The executable takes `--root <dir>` and `--select <path relative to root>`. With no arguments
it opens the parent of your home directory with the home directory selected.

## Using it

| Key | Action |
|---|---|
| `↑` `↓` | move within the column (also the mouse wheel) |
| `→` `Space` | enter the selected folder |
| `←` | back to the parent |
| `Enter` | enter a folder, open a file with the default application |
| `/` or `Ctrl+K` | search |
| `Esc` | close search |

Clicking any name, including names in the preview columns, selects it.

Search is fuzzy and matches names and paths: `rtr` finds `OrthogonalRouter.swift`,
`mordor roster` finds `05 Mordor/Orc shift roster.csv`. Picking a result opens the path to it.

## How it works

The Rust side owns the filesystem. The UI only ever sends paths relative to the root, and
every one is normalised and checked against the root after resolving symlinks, so `..`, drive
letters, UNC paths and junctions that point outside are refused. Listing a folder reads that
one folder; there is no recursive traversal on the UI side.

A background thread walks the root in parallel and keeps a SQLite index (with an FTS5 trigram
index on names), synced on later launches by diffing against what is already stored. A
filesystem watcher coalesces bursts of events and updates the index, the search corpus and
any folder that is on screen. Search takes name candidates from FTS5 and falls back to a
parallel fuzzy scan (nucleo) over all paths held in memory.

The UI keeps a lazy model of the folders it has listed and turns it into a layout with a pure
function; connectors are routed from that geometry and redrawn every animation frame, so moving
the selection interpolates the whole scene instead of re-rendering it. Only rows near the focus
line are laid out, which keeps a 20,000-entry folder responsive.

The layout and colour rules were measured from a screen recording of someone else's macOS
prototype; [docs/design.md](docs/design.md) explains what was measured and where this
implementation differs.

## Tests

```sh
bun run test        # TypeScript unit tests and Rust tests
bun run test:e2e    # drives the real app through WebView2's DevTools protocol (Windows)
```

The unit tests include a sweep over every possible selection in the fixture, checking that no
names overlap and no connector crosses another or runs through text. The end-to-end run
navigates with keyboard and mouse, searches, edits files on disk to exercise the watcher, and
compares 13 screenshots at 1812×1344 against local baselines in `screenshots/baseline`. The
first run records them; `bun run test:e2e -- --update` rewrites them.

## Performance

On a 500,000-entry tree, searching takes under 45 ms and moving through a 20,000-entry
folder costs about 4 ms of layout per keystroke. The full numbers, and how to reproduce them,
are in [docs/benchmarks.md](docs/benchmarks.md).

## Limitations

- When a long name sits where connectors need to pass, the preview column is pushed right
  rather than routed around the name as tightly as the original does.
- Width is computed in terminal cells, so names in scripts that need a fallback font (CJK,
  emoji) are only approximately aligned.
- The webview dominates memory use: about 400 MB for the whole process tree on Windows, most of
  it WebView2.
- `.git`, `node_modules`, `target` and `AppData` are browsable but not indexed for search.

## License

MIT
