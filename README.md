# lupasta

A desktop file browser that draws your filesystem as a single horizontal path instead of a
tree view. Each column lists the siblings of one folder on the path to the current selection;
the next columns preview what is inside the neighbouring folders. Everything is monospace text
on black, joined by orthogonal connectors.

Text colour encodes age, not file type: names modified recently are white and fade through
grey towards orange (on the selection path) or blue (in the previews) as they get older.

Built with Tauri 2, Rust and Svelte 5. It is developed on Windows; CI builds it and runs a smoke
test of the real app on Windows, macOS and Linux on every push.

## Running it

You need Rust, [Bun](https://bun.sh) and the platform webview: WebView2 on Windows (ships with
Windows 11), WKWebView on macOS, WebKitGTK 4.1 on Linux (`libwebkit2gtk-4.1-dev` and the other
[Tauri prerequisites](https://tauri.app/start/prerequisites/)).

```sh
bun install
bun tauri dev              # opens the folder that contains your home directory
bun run dev:fixture        # opens the synthetic fixture used by the tests
bun tauri build            # release build and installer
```

Any stable Rust toolchain works. On Windows without the Visual Studio build tools you can use the
GNU toolchain for this folder only (`rustup override set stable-x86_64-pc-windows-gnu`); it needs
a MinGW gcc on `PATH` to compile the bundled SQLite, which [WinLibs](https://winlibs.com)
provides (`winget install BrechtSanders.WinLibs.POSIX.MSVCRT`).

The executable takes `--root <dir>` and `--select <path relative to root>`. With no arguments
it reopens the last place you were in, or the parent of your home directory with the home
directory selected.

## Using it

| Key | Action |
|---|---|
| `↑` `↓` | move within the column (also the mouse wheel) |
| `→` `Space` | enter the selected folder |
| `←` | back to the parent; on a top-level entry, go up to the folder above the root |
| `Enter` | enter a folder, open a file with the default application |
| `/` or `Ctrl+K` | search |
| `Ctrl+C` | copy the full path of the selection |
| `Ctrl+E` | show the selection in Explorer / Finder / the file manager |
| `Ctrl+H` | show or hide hidden files |
| `Ctrl+O` | open another folder |
| `Ctrl+=` `Ctrl+-` `Ctrl+0` | zoom |
| `Ctrl+,` | settings |
| `Esc` | close search or settings |

Clicking any name, including names in the preview columns, selects it. Moving the pointer to
the top edge reveals a bar with the current folder, search and settings.

Settings are kept in `settings.json` in the app data folder: colour by age or by kind and how
old the oldest colour is, font (Iosevka, JetBrains Mono, IBM Plex Mono or the system monospace,
each sized to the same 10 px cell so the layout does not change), zoom, animation speed,
truncation, a status line with the selection's size and age, hidden files, sort order, folders
first, wheel speed, reopening the last place, recent folders, and what the search index skips.
"Check for updates" asks GitHub for the latest release and links to it; nothing is downloaded
or installed automatically.

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

`bun tauri build --debug --no-bundle` followed by `lupasta --smoke --root fixtures/visual/Users`
runs the real app once and exits 0 only if it laid out the tree, indexed it, found an entry by
search and has a working filesystem watcher; CI runs this on all three platforms.

The unit tests include a sweep over every possible selection in the fixture, checking that no
names overlap and no connector crosses another or runs through text. The end-to-end run
navigates with keyboard and mouse, searches, edits files on disk to exercise the watcher, and
compares 13 screenshots at 1812×1344 against local baselines in `screenshots/baseline`. The
first run records them; `bun run test:e2e -- --update` rewrites them.

## Icons and releases

`bun run icons` regenerates every icon in `src-tauri/icons` from `scripts/make-icons.ts`: a vector
master for the large sizes and hand-placed pixel drawings for 16, 24, 32 and 48 px, rasterised
with a local Chromium and packed into `.ico` and `.icns`.

Pushing a tag such as `v0.2.0` (matching the version in `tauri.conf.json`) builds installers for
Windows, macOS (universal) and Linux into a draft GitHub release. The builds are not code-signed,
so SmartScreen and Gatekeeper will warn on first launch.

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
- `.git`, `node_modules`, `target` and `AppData` are browsable but not indexed for search (the
  list is editable in settings).
- Live updates watch the whole root recursively. On Linux a large root can exceed the inotify
  watch limit (`fs.inotify.max_user_watches`); browsing still works and settings say so. On
  macOS, indexing a home folder triggers the system prompts for Desktop, Documents and Downloads.

## License

MIT
