# gpui-animated-gradient-text

Fast, Unicode-aware animated gradient text for GPUI.

The crate renders one `StyledText` with a bounded number of colored text runs.
It does not allocate one GPUI element per character, create a timer thread, or
paint a texture. Animation uses GPUI's display-linked frame scheduler.

## Add it

```toml
[dependencies]
gpui-animated-gradient-text = "0.1"
```

This release targets GPUI `0.2.2`.

## Use it

Keep the prepared value in your view state. Its text, grapheme index, and
palette are immutable shared storage, so cloning it during `render` is cheap.

```rust
use gpui::{Context, Entity, Render, Window, div, prelude::*, rgb};
use gpui_animated_gradient_text::{GradientPalette, GradientText, Motion};
use std::time::Duration;

struct Header {
    title: GradientText,
}

impl Header {
    fn new() -> Self {
        let palette = GradientPalette::new([
            rgb(0x57e2bb),
            rgb(0x78a8ff),
            rgb(0xd779ff),
            rgb(0xff7ab8),
        ])
        .unwrap();

        Self {
            title: GradientText::new("PHOENIX NATIVE", palette)
                .cycles(0.85)
                .motion(Motion::Forward)
                .max_color_runs(48),
        }
    }
}

impl Render for Header {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .text_xl()
            .child(
                self.title
                    .clone()
                    .animated("phoenix-title-gradient", Duration::from_secs(4)),
            )
    }
}
```

The animation ID must remain stable between renders.

## Static and reduced-motion rendering

Render a fixed phase without scheduling another frame:

```rust
# use gpui_animated_gradient_text::GradientText;
let label = GradientText::phoenix("Saved").phase(0.35);
```

This is also the right path when your application honors a reduced-motion
preference.

## Performance model

- Unicode grapheme boundaries are indexed once in `GradientText::new`.
- Clones share the source text, grapheme offsets, and palette.
- Each rendered frame performs one bounded `Vec<TextRun>` allocation.
- Long strings produce at most 64 color runs by default.
- `.max_color_runs(n)` trades gradient resolution for less shaping and paint
  work; it never splits a grapheme.
- Palette sampling is allocation-free and uses cyclic sRGB interpolation.

Run the benchmark:

```powershell
cargo bench -p gpui-animated-gradient-text --bench gradient_runs
```

Run the standalone showcase:

```powershell
cargo run -p gpui-animated-gradient-text --example showcase
```

## Publishing

Before publishing a release:

```powershell
cargo test -p gpui-animated-gradient-text --all-targets
cargo clippy -p gpui-animated-gradient-text --all-targets -- -D warnings
cargo package -p gpui-animated-gradient-text
cargo publish -p gpui-animated-gradient-text --dry-run
```

Licensed under either Apache-2.0 or MIT, at your option.
