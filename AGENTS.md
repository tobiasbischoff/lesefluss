# AGENTS.md

Rust workspace for **Lesefluss**, a GTK4 / libadwaita / WebKitGTK 6 RSS reader for Linux (primary target: Arch / Omarchy, Wayland).

## Commands

CI (`.github/workflows/ci.yml`) runs these in this order — keep it that way, it is the gate:

```sh
cargo build --workspace --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
```

Focused runs:

```sh
cargo test -p reader                                  # one package
cargo test -p reader sanitize::image_tests            # one module
cargo test --workspace media                          # name substring
cargo build --release --locked --bin lesefluss
cargo run --locked
```

- Clippy is **strict**: `-D warnings` over `--all-targets`. There is no `clippy.toml` and no `[lints]` section, so plain `cargo clippy` is clean — any new warning is yours. Fix it, don't allow it.
- Always pass `--locked`. `Cargo.lock` is committed and CI enforces it.
- Building needs native libs discoverable by `pkg-config`: `gtk4`, `libadwaita-1`, `webkitgtk-6.0` (note the exact `.pc` name), plus `glib2-devel`, `sqlite`, `libsecret`. Missing them is a link/`pkg-config` error, not a Rust error.
- Tests that exercise networking bind a loopback socket; they need no display and no external service.
- Helper binaries: `lesefluss-probe` (rendering prototype, declared as `[[bin]]` in `crates/app/Cargo.toml`) and `lesefluss-bench` (DB benchmark, `src/bin/`). A stale `target/release/lf-bench` from before the rename may still sit in `target/` — it is not produced by the current sources.

## Conventions

- **Comments and doc comments are German.** README and `docs/` are German too (README prose is English). Match the surrounding file rather than switching languages.
- **UI strings never use `format!` directly.** Use the macros from `crates/app/src/strings.rs` — `tr!("Deutsch", "English")`, `tr_format!(...)`, `tr_plural!(count, de_one, de_many, en_one, en_many)`. **German is the first argument.** Language is resolved once at startup; changes apply on the next launch.
- Never commit tokens: `*.token`, `feedly-token`, `feedly-credential` are gitignored. Log output must go through `window::redact`.

## Runtime environment variables

Handy for local runs of `./target/release/lesefluss`:

| Var | Effect |
| --- | --- |
| `LF_DEBUG` | enables `[lf] …` stderr logging (redacted) |
| `LF_SEED` | seeds fixtures — **only if the library is empty** |
| `LF_LANG` | `de`, `en`, `system`; overrides the saved choice |
| `LF_SUBSCRIBE` | subscribes to a feed on startup |
| `LF_FRAMECHECK` | WebKit frame-check toggle |
| `LF_FEEDLY_CONNECT`, `LF_FEEDLY_BASE` | Feedly test wiring |

Data locations: DB at `~/.local/share/lesefluss/library.db` (`$XDG_DATA_HOME`), image cache at `~/.cache/lesefluss/media` (`$XDG_CACHE_HOME`), credential fallback at `~/.config/lesefluss/`. Inspect the live DB directly with `sqlite3`.

## Architecture notes

Crates: `app` (GTK orchestration), `domain`, `storage` (SQLite + FTS + migrations), `sync`, `provider-local` (fetch/discover/image cache), `provider-feedly`, `reader` (sanitize + render).

- **The DB is single-threaded by design.** All queries go through `DbWorker` / `App::db_query` onto the `lf-db` thread; results come back as typed callbacks. Never touch `Database` from the GTK main loop. Network and media work runs on the `lf-net` tokio runtime and returns via `pending_media` / `pending_db` queues drained in `drain_once`.
- **Async results are generation-bound.** `ReaderPane::document_generation` is reserved *before* the HTML is built; late media/search/position results for a stale generation are dropped. Reserve the generation first when touching the reader pipeline.
- **Reader pipeline order matters.** `reader::sanitize::rewrite_images` must run *before* `image_alt_texts`, because only the rewritten HTML carries `data-lf-src`. Reading the image list from raw feed HTML silently yields nothing — this is exactly the bug fixed in `load_reader_html` (`crates/app/src/window.rs`).
- `data-lf-src` holds the **HTML-escaped** URL; everything downstream (scraper, `article_media`, the JS bridge) compares against the **decoded** DOM value. Keep the escaping in `escape_attr` exactly once — double-escaping (`&amp;amp;`) makes a URL unmatchable.
- The webview runs under `default-src 'none'; style-src 'unsafe-inline'; img-src data:`. Only `data:` URIs may load, so **every** image must be fetched by the app and inlined. Images arrive via the user-script `lf-media` bridge (`crates/app/src/reader.rs`), which sets `img.src`.
- Media cache (`crates/provider-local/src/media.rs`): file name is the 16-hex hash of the URL, no extension. Accepts JPEG/PNG/GIF/WebP by magic bytes only — **SVG is rejected by design**, as are files ≤128 bytes, >12 MB, and >40 MP. Prune keeps DB-pinned keys (saved/shared articles). Per-URL in-flight mutex means concurrent requests cause exactly one HTTP fetch; leave that intact.
- Asynchronous image rewrites and network failures must never be able to swap the visible article: no `Unchecked` panics across the queue, and errors surface as text, never swallowed (see commit `1595e0c`).

## Storage migrations

`MIGRATIONS: &[(i64, &str)]` in `crates/storage/src/lib.rs`, currently **1–14**, applied in ascending order, each in its own transaction.

- **Append only.** Never edit or renumber an existing entry — add a new `(n+1, sql)` pair.
- `max_schema_version()` derives the ceiling; don't hardcode it.
- A DB whose `MAX(version)` exceeds the app's ceiling is **rejected** (read-only) — a downgrade guard.
- Migrating writes a `<name>.pre-migrate-<ms>.db` backup next to the DB.

## Packaging

`packaging/PKGBUILD` builds the **parent checkout** (`_srcdir`), not a downloaded tarball.

- It exports `CARGO_PROFILE_RELEASE_LTO=false` and clears `LTOFLAGS`: makepkg's LTO breaks the bundled C deps (`libsqlite3-sys`, `aws-lc-sys`). Keep those overrides.
- Desktop file, icon and AppStream metadata come from `data/`, which is the source of truth for the app ID `io.github.tobiasbischoff.Lesefluss` (must match `application_id` in `crates/app/src/main.rs`).
- Bump `pkgrel` on changes. Build with `cd packaging && makepkg -f --noconfirm --nodeps` as a **non-root** user.
- `packaging/pkg/`, `packaging/src/` and `*.pkg.tar.zst` are ignored by `packaging/.gitignore`; leftover build output there is not a dirty tree.

## Before you claim a fix works

- A green unit test is not evidence for pipeline behaviour. Reader/media bugs have twice hidden behind tests that only asserted a *substring* (see `only_img_src_is_replaced`). Assert the whole emitted markup, and re-check against real data: `sqlite3 ~/.local/share/lesefluss/library.db "SELECT html FROM article_contents …"`.
- To reproduce a library-level bug without polluting the real DB, copy the DB first and point `$XDG_DATA_HOME`/`$XDG_CACHE_HOME` at scratch dirs.
- `target/release/lesefluss` is what a running session uses; after `cargo build --release` a **restart is required** for the user to see the change. Say so explicitly.
