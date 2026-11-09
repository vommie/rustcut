# Writing RustCut plugins

A RustCut plugin is a small WebAssembly component that reacts to what happens in RustCut (a video is opened, the user
assigns a tag) and answers with actions (set the target folder, set the rating, show a message). RustCut runs every
plugin in a sandbox: no files, no network, no environment, no other programs, a time limit per call and a memory
limit per plugin. A plugin that fails, crashes or runs too long never blocks or breaks RustCut.

This guide is for plugin authors. The contract itself is
[`crates/rustcut-plugin-api/wit/rustcut-plugin.wit`](../crates/rustcut-plugin-api/wit/rustcut-plugin.wit); the
plugins of the user live in the repository [`rustcut-plugins`](https://github.com/vommie/rustcut-plugins) (Auto-Folder and Auto-Rate are the examples).

## Quick start

In a clone of [`rustcut-plugins`](https://github.com/vommie/rustcut-plugins) (Rust stable through rustup; `rust-toolchain.toml` adds the `wasm32-wasip2` target):

```bash
cargo xtask new auto-name --name "Auto-Name"
cargo xtask build auto-name
```

Then in RustCut: Settings > Plugins > Development folders > Add development folder, and choose
`plugins/auto-name`. RustCut loads the plugin straight from its build output and reloads it whenever
`cargo xtask build` writes a new one; its settings appear as a page below Plugins, its log lines in the Logs panel
(source "Plugins"). `cargo xtask package auto-name` makes `dist/auto-name-<version>.rcplugin` for Settings > Plugins >
Install (or drop the file onto that page).

## The pieces of a plugin

| File | What it is |
|---|---|
| `plugin.toml` | Who the plugin is, which plugin API it needs, its settings (RustCut draws them). |
| `src/lib.rs` | The plugin: implements `rustcut_plugin_api::Plugin` and calls `export_plugin!`. |
| `README.md`, `CHANGELOG.md`, `LICENSE`, `icon.svg` | Packed into the plugin file next to the component. |

```rust
use rustcut_plugin_api::{Action, Context, Event, Issue, Plugin, export_plugin};

struct Hello;

impl Plugin for Hello {
    fn configure(settings: String) -> Result<(), String> { Ok(()) }
    fn handle(event: Event, context: Context) -> Result<Vec<Action>, String> { Ok(Vec::new()) }
    fn check(settings: String) -> Vec<Issue> { Vec::new() }
}

export_plugin!(Hello);
```

- `configure(settings)`: after loading and after every change of the plugin's settings. `settings` is a JSON object
  with one member per field of `plugin.toml` (`rustcut_plugin_api::parse_settings` reads it into your own type). An
  error marks the plugin "Cannot run: the settings were refused" until the settings change.
- `handle(event, context)`: for every event; returns the actions RustCut applies, in order.
- `check(settings)`: while the user edits the settings; returns findings that RustCut shows at the field or table row
  (a warning mark with the message as its tooltip). It must not change anything.
- `rustcut_plugin_api::log(level, text)` writes a line into the plugin log. What a plugin prints on stdout or stderr
  (a panic's message, too) lands there as well.
- Since API 1.1, inside `handle`: `rustcut_plugin_api::target_folder()` is the id of the folder of Settings > Target
  Folders that is the target now (0 = "Next to the source", `None` for a loaded job's own folder), the same kind of
  id as a `target` setting holds; `rustcut_plugin_api::hide_marks()` keeps the plugin marks away from what this call's
  actions set. Both need `api = "1.1"` in `plugin.toml`; outside RustCut (native tests) they are `None` and do nothing.

## plugin.toml

```toml
id = "auto-folder"          # lower case letters, digits, dashes; 2 to 40 characters; also the settings key
name = "Auto-Folder"        # what RustCut shows
version = "1.0.0"           # the plugin's own semantic version (Cargo.toml must have the same)
api = "1.0"                 # the plugin API it was built against
description = "Sets the target folder from the folder the video comes from."
authors = ["vommie"]
license = "MIT"
repository = "https://github.com/vommie/rustcut-plugins"
icon = "folder-bolt"        # a Tabler icon name RustCut knows, or "icon.svg" next to plugin.toml
wasm = "../../target/wasm32-wasip2/release/auto_folder.wasm"   # the build output; a plugin file holds plugin.wasm
```

Unknown keys are refused, so a typo never goes unnoticed.

### Settings

Settings are cards of fields; RustCut draws them on the plugin's page with its own controls (tooltips, checks while
typing, jump marks, the page's reset button) and stores the values in `config.toml` under
`[plugins.entries.<id>.values]`.

```toml
[[cards]]
title = "Behavior"
description = "When the rules apply."

[[cards.fields]]
key = "trigger"                     # lower case letters, digits, underscores; the member in the settings JSON
label = "Apply the rules when"
help = "What it does, with an example. It is the tooltip."
kind = "choice"
default = "new-video"
options = [{ value = "new-video", label = "A new video is opened" }, { value = "also-jobs", label = "..." }]
enabled_if = { key = "other_field", equals = true }   # optional: dimmed and ignored otherwise
```

| `kind` | JSON value | Keys |
|---|---|---|
| `switch` | `true` / `false` | `default` |
| `number` | number | `default`, `min`, `max`, `step` (1), `decimals` (0), `unit` |
| `choice` | the option's `value` | `options` (1 to 12: `value`, `label`, `help`), `default`; up to three short options show as segments |
| `text` | string | `default`, `placeholder` |
| `path` | string (a leading `~` is expanded before the plugin sees it) | `select` (`folder` or `file`), `default`, `placeholder` |
| `target` | a folder id of Settings > Target Folders, `0` = "Next to the source", or `null` | |
| `tag` | a RustView tag id, or `null` | |
| `rating` | 0 to 5, or `null` | `default` |
| `table` | an array of objects, one per row | `columns` (fields, no tables), `add_label`, `empty`, `reorder_drag`, `reorder_buttons` |

Tables are for rules. `reorder_drag = true` gives each row a grip for drag & drop, `reorder_buttons = true` move-up
and move-down buttons; RustCut does all of it (the gap, the ghost, scrolling near the edges, Escape) and the plugin
just gets the rows in their new order. In a table, `enabled_if` names a column of the same row.

Values are sanitized before the plugin sees them: missing or unusable values take their defaults, numbers are clamped
and rounded to `decimals`, unknown choices fall back, rows are capped at 500 and texts at 4096 characters. Values the
manifest no longer declares stay in `config.toml` (another version may use them) but are not handed over.

RustCut checks on its own what every plugin would have to check: a `tag` or `target` value whose tag or folder no
longer exists gets a warning at its cell ("Folder 7 (no longer exists)" stays listed until the user picks another), and
a `path` folder that does not exist on disk gets one too, unless the text looks like a pattern (it holds one of
`^$*+?()[]{}|\`).

A `tag` or `target` field's list has a search field above its entries and scrolls, however many tags or folders the
user has: the search has the keyboard as soon as the list opens, Up/Down and Enter pick, Escape closes. A `choice` of
more than twelve options would get the same, so no plugin declares or draws any of it.

## Events, context and actions

| Event | When |
|---|---|
| `video-opened(user)` | The user opened a video (File > Open, a drop, the command line, a second start). |
| `video-opened(job)` | A queued job was loaded into the editor; its own target and tags are already in place. |
| `tag-assigned(tag)` / `tag-removed(tag)` | The user clicked a tag in the Tag & Rate panel (tree or history). |

The `context` holds the open video's path, the rating, the assigned tags (each with `id`, `name` and `path` from the
root: `["Treasure chest", "Great"]`) and the current target folder's path; since API 1.1 the host's `target-folder`
(`rustcut_plugin_api::target_folder()`) gives the same target as an id, to compare with a `target` setting (Auto-Folder
sets nothing when the rule's folder is the target already).

| Action | What RustCut does |
|---|---|
| `set-target(folder(id) or path(p), reason)` | Sets Job Finalization's target folder, with a plugin mark at the field ("Set by Auto-Folder (reason)"; a click opens the plugin's page). A folder id that is not in the tree gets a banner instead; a folder missing on disk gets a banner and is created by the render. |
| `set-rating(stars, reason)` | Sets the rating, with a mark at the rating bar, when something can be rated (a video and "Active" on, or a queued job edited). |
| (since 1.1) `hide-marks()` in `handle` | The `set-target` and `set-rating` actions of this call apply without a plugin mark, and an earlier plugin's mark at the same place goes (it would name the wrong plugin). Auto-Rate's "Show mark" switch uses it. |
| `notify(notice)` | A toast or a banner (`place`), its title after the plugin's name; `none` only writes the plugin log. |

Rules RustCut applies, so plugins need not:

- Plugins run in the order of Settings > Plugins, top first; each sees what the earlier ones changed (target, rating)
  and the later one wins. Each plugin's answer is applied as soon as it is there.
- The user wins: once the user chose a target folder for this video (the folder list, the folder wheel), no plugin
  changes it until the next video. A user's own rating click removes the plugin mark.
- An answer about an earlier video (the user opened another one meanwhile) is dropped.

## Limits and failures

| Limit | Default | Where |
|---|---|---|
| Time per call | 1 s (0.1 to 30 s) | the plugin's page, "Time limit" |
| Memory | 64 MB (16 to 1024 MB) | the plugin's page, "Memory limit" |

A call that runs out of time or memory, crashes (a panic, a trap) or returns an error counts as a failure: the
plugin's page and the Logs panel show it, and a banner says so ("Auto-Folder failed: timed out after 1s", with "Open
plugin"). After 3 failures in a row the plugin is paused until it is reloaded or RustCut starts again. A crashed or
stopped plugin gets a fresh instance for its next call, configured with its settings again. Every plugin has its own
thread; a slow plugin delays only the plugins after it, and only for that one event, never the window.

`rustcut --no-plugins` starts without running any plugin; Settings > Plugins > Run plugins switches all of them off.

## Plugin files, installing and updates

A plugin file (`<id>-<version>.rcplugin`) is a tar archive compressed with zstd, every file at the top level:
`plugin.toml` (its `wasm` is `plugin.wasm`), `plugin.wasm`, and optionally `icon.svg`, `README.md`, `CHANGELOG.md`,
`LICENSE`. `cargo xtask package <id>` makes it reproducibly (sorted, time stamps 0) with a `.sha256` beside it.

- Install: Settings > Plugins > Install, a drop onto that page, or "Available in the update folder". RustCut reads the
  file with size limits, checks the manifest and the plugin API, compiles the component, and only then puts it into
  `<data>/plugins/<id>/` (the previous version stays until the new one is in place).
- An update keeps the settings; an older version than the installed one asks first ("Install anyway").
- Remove (a held button) deletes the plugin's folder; its settings stay for a reinstall. Export and import save and
  read a plugin's settings as TOML.
- "Check for updates" looks into the update folder (Settings > Plugins > Update folder, for example the `dist` folder
  of the plugin repository).
- A development folder with the same id as an installed plugin runs instead of it (the installed one shows "A
  development folder runs instead").

## Versions

The plugin API follows semantic versioning (the WIT package `rustcut:plugin@1.1.0`, the crate `rustcut-plugin-api`,
tags `plugin-api-vX.Y.Z` in RustCut's repository). A minor version only adds host functions, imported interfaces or
types that no export uses; RustCut runs every plugin of its major version whose `api` is not newer than its own. New
events, actions, record fields, enum cases or exports change the types of `configure`, `handle` and `check`, which
wasmtime checks exactly when it loads a plugin (a plugin built against 1.0 would no longer load), so they wait for the
next major version. A plugin keeps its own version, changelog and release tags (`<id>-v<version>`) in
`rustcut-plugins`.

| API | What it added |
|---|---|
| 1.0 | The events `video-opened`, `tag-assigned`, `tag-removed`; the actions `set-target`, `set-rating`, `notify`; the host's `log`. |
| 1.1 | The host's `target-folder` and `hide-marks` (new functions only, so a plugin built against 1.0 links unchanged; RustCut's host tests load a 1.0 build). |

## Testing

`crates/plugin-testkit` in `rustcut-plugins` builds a plugin for `wasm32-wasip2` and runs it in RustCut's real plugin
host, with its `plugin.toml` defaults plus the test's values:

```rust
let mut plugin = plugin_testkit::plugin!(serde_json::json!({ "rules": [] }));
let actions = plugin.handle(&Event::VideoOpened(OpenedBy::User), &context).unwrap();
```

Keep the logic in plain functions (tested natively) and add host tests for what RustCut sees. `cargo xtask check`
runs the manifest checks, formatting, clippy (native and wasm), the tests and `cargo deny`.
