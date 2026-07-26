# Phoenix integration boundary

This directory vendors Velotype `v0.7.0` at the immutable commit recorded in
`PHOENIX-VENDOR.toml`. The upstream source and assets were imported as a complete
Git tree so Phoenix can build, debug, profile, and modify the editor locally.

The initial import used a nested local Cargo workspace. Velotype and Phoenix
both resolve GPUI 0.2.2, but their original dependency graphs could not share
one lockfile: Phoenix's `gpui-component` resolves `tree-sitter` 0.25 while
Velotype declared optional `tree-sitter` 0.26 support, and both crates publish
the same native `links` name.

Phoenix patch 1 pins Velotype's `tree-sitter` and `tree-sitter-highlight`
runtime pair to 0.25.10, the exact version already resolved by
`gpui-component`. The frozen upstream default build and 739-test suite were
proved before this patch. The same 739 tests passed afterward, so the nested
workspace was removed and Velotype became a member of the main
`phoenix-native` workspace.

Phoenix patch 2 applies four behavior-preserving Clippy repairs required by the
workspace's warnings-denied gate: a named click-handler type, direct PDF
parameter initialization, removal of a needless theme-ID borrow, and a
test-only redundant-closure removal.

Phoenix patch 3 converts the package into a library plus its original thin
executable, adds an embedded initialization path with no preferences or network
startup, and adds an embedded editor host mode. Embedded mode omits standalone
file/drop/export/save/close actions, window chrome, menus, workspace panel, and
status bar while retaining the Markdown document, editing, selection, undo,
IME, rendering, and source-mode machinery.

Phoenix patch 4 moves the upstream development dependency optimization table
to the actual Phoenix workspace root. Cargo ignores profiles declared by a
member package; centralizing the unchanged package overrides makes the intended
interactive debug performance effective and removes the misleading warning.

This isolation does not grant the standalone Velotype application authority
inside Phoenix.

## Embedded ownership contract

The embedded editor must receive document identity and content from
`PhoenixKernel`. It may own transient editing, selection, composition, and undo
state for the active revision. It must not become the authority for:

- workspace enumeration or active document identity;
- durable file paths, saving, rename, delete, or recovery;
- graph generations, manifolds, or style state;
- application shutdown or native window lifetime.

Standalone menus, update checks, network fetching, dialogs, exports, and direct
filesystem ownership remain outside the first embedded feature set.

The next extraction step will expose a Phoenix-owned adapter instead of making
the shell depend on Velotype's executable entry point.

## Modification policy

Keep upstream behavior changes separate from mechanical integration changes.
When source is modified, record the reason and Phoenix contract affected in this
file or a successor changelog. Preserve `LICENSE-APACHE` and the provenance file.
