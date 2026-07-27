# gpui-animated-gradient-text

Fast, Unicode-aware animated gradient text and gradient authoring for GPUI.

The renderer shapes one `StyledText` with bounded color runs. It does not
allocate an element per character, create a timer thread, or paint a texture.
Animation uses GPUI's display-linked frame scheduler.

Version 0.2 adds:

- the optional `gpui-component` color picker bridge;
- empty, solid, and multi-stop gradient authoring;
- Unicode-grapheme-safe fills for selected text ranges;
- positioned hard stops and sRGB, linear-sRGB, or Oklab interpolation;
- built-in and versioned portable presets, with optional JSON.

The crate remains standalone and is not wired into Phoenix Native.

## Add it

```toml
[dependencies]
gpui-animated-gradient-text = "0.2"
```

Enable only the integrations you use:

```toml
[dependencies]
gpui-animated-gradient-text = {
    version = "0.2",
    features = ["picker-ui", "serde"],
}
```

This release targets GPUI `0.2.2`. `picker-ui` targets
`gpui-component 0.5.1`.

## Render animated text

```rust
use gpui::{div, prelude::*, rgb};
use gpui_animated_gradient_text::{
    ColorSpace, GradientPalette, GradientText, Motion,
};
use std::time::Duration;

let palette = GradientPalette::new([
    rgb(0x57e2bb),
    rgb(0x78a8ff),
    rgb(0xd779ff),
    rgb(0xff7ab8),
])
.unwrap()
.with_color_space(ColorSpace::Oklab);

let title = GradientText::new("PHOENIX NATIVE", palette)
    .cycles(0.85)
    .motion(Motion::Forward)
    .max_color_runs(48);

let element = div().child(
    title.animated("phoenix-title-gradient", Duration::from_secs(4)),
);
# let _ = element;
```

The animation ID must remain stable between renders. Use `.phase(0.35)` for a
fixed frame or reduced-motion presentation.

## Author colors

`GradientDraft` deliberately supports all intermediate states:

```rust
use gpui::rgb;
use gpui_animated_gradient_text::{GradientDraft, TextFill};

let mut draft = GradientDraft::new();
let first = draft.insert_color(None, rgb(0x57e2bb).into());
draft.insert_color(Some(first), rgb(0xd779ff).into());

match draft.resolved_fill() {
    TextFill::Inherit => {}        // zero selected colors
    TextFill::Solid(color) => {}   // one selected color
    TextFill::Gradient(palette) => {} // two or more
}
```

With `picker-ui`, create `GradientPickerState` inside a GPUI view context and
render `GradientPicker::new(&state)`. The host subscribes to
`GradientPickerEvent::Change`; each event contains the complete new draft.
Initialize `gpui-component` once in the host application.

## Style selected text

```rust
use gpui::rgb;
use gpui_animated_gradient_text::{GradientText, TextFill, TextFillMap};

let source = "keep this Unicode-safe";
let mut fills = TextFillMap::new();
fills
    .assign_bytes(source, 5..9, TextFill::Solid(rgb(0xff7ab8).into()))
    .unwrap();

let text = GradientText::phoenix(source).with_fill_map(fills);
# let _ = text;
```

Assignments replace overlapping spans and preserve sorted, non-overlapping
storage. Byte ranges must land on UTF-8 and Unicode grapheme boundaries.
`assign_graphemes` is available when the caller naturally works in grapheme
indices. `SelectableGradientText` converts pointer drags into aligned byte
ranges without owning or mutating an editor buffer.

## Positioned stops and presets

`GradientStop::new(position, color)` accepts positions in `0.0..=1.0`.
Duplicate positions create hard stops. Sampling is cyclic and allocation-free.

`GradientPresetV1` captures stops, interpolation space, motion, cycles, and
duration independently from the displayed text. Built-ins are available as
`phoenix()`, `aurora()`, `sunset()`, and `builtins()`. With `serde`, use
`to_json_pretty` and `from_json`; all builds can emit a pasteable Rust palette
builder through `to_rust_builder`.

## Performance model

- Source text and grapheme boundaries are indexed once, then shared by clones.
- Immutable palette colors are precomputed in all interpolation spaces.
- Palette sampling performs no allocation.
- Each frame allocates one bounded `Vec<TextRun>`.
- Explicit fill and selection boundaries always survive the run budget.
- Long unstyled regions produce at most 64 gradient bands by default.
- The editor stores compact stops with stable integer IDs and publishes an
  immutable `Arc`-backed palette.

Run the benchmark:

```powershell
cargo bench -p gpui-animated-gradient-text --bench gradient_runs
```

Run the standalone examples:

```powershell
cargo run -p gpui-animated-gradient-text --example showcase
cargo run -p gpui-animated-gradient-text --example authoring_showcase --features picker-ui,serde
```

## Publishing

```powershell
cargo test -p gpui-animated-gradient-text --all-features --all-targets
cargo clippy -p gpui-animated-gradient-text --all-features --all-targets -- -D warnings
cargo package -p gpui-animated-gradient-text --all-features
cargo publish -p gpui-animated-gradient-text --dry-run --all-features
```

Licensed under either Apache-2.0 or MIT, at your option.
