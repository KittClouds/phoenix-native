use super::{PhoenixShell, BORDER, BORDER_BRIGHT, SURFACE, TEXT_MUTED};
use gpui::{div, prelude::*, px, rgb, Context, FontWeight, IntoElement, SharedString};
use gpui_animated_gradient_text::{GradientPalette, GradientText};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{Disableable, Sizable};
use std::{sync::LazyLock, time::Duration};

const ACCENT: u32 = 0x57e2bb;
const FOOTER_BG: u32 = 0x171918;

static HEALTHY_PALETTE: LazyLock<GradientPalette> = LazyLock::new(|| palette([0x32cd32, 0x00ff7f]));
static RISING_PALETTE: LazyLock<GradientPalette> = LazyLock::new(|| palette([0x32cd32, 0x7cfc00]));
static CAUTION_PALETTE: LazyLock<GradientPalette> = LazyLock::new(|| palette([0x9acd32, 0xadff2f]));
static WARNING_PALETTE: LazyLock<GradientPalette> = LazyLock::new(|| palette([0xffa500, 0xffd700]));
static DANGER_PALETTE: LazyLock<GradientPalette> = LazyLock::new(|| palette([0xffa500, 0xff6347]));
static OVER_LIMIT_PALETTE: LazyLock<GradientPalette> =
    LazyLock::new(|| palette([0xff4500, 0xff0000]));

/// A/B arm for the display-linked length warning.
///
/// This lives at the footer boundary so the experiment can be removed without
/// changing document metrics, editor state, or the gradient-text crate.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum FooterMotionArm {
    /// Repeat forever. Kept only as the control arm for performance diagnosis.
    Animated,
    /// Animate briefly when an actionable band is entered, then become static.
    #[default]
    Pulse,
    /// Keep the warning palette while eliminating the repeating scheduler.
    Static,
}

impl FooterMotionArm {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Animated => "animated",
            Self::Pulse => "pulse",
            Self::Static => "static",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "animated" => Some(Self::Animated),
            "pulse" => Some(Self::Pulse),
            "static" => Some(Self::Static),
            _ => None,
        }
    }

    pub(crate) const fn render_arm(self, pulse_active: bool) -> Self {
        match self {
            Self::Pulse if pulse_active => Self::Animated,
            Self::Pulse => Self::Static,
            arm => arm,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct DocumentMetrics {
    words: usize,
    characters: usize,
}

impl DocumentMetrics {
    pub(super) const fn counts(self) -> (usize, usize) {
        (self.words, self.characters)
    }

    pub(super) const fn band_names(self) -> (&'static str, &'static str) {
        (
            word_band(self.words).as_str(),
            character_band(self.characters).as_str(),
        )
    }
}

impl DocumentMetrics {
    pub(super) fn from_text(text: &str) -> Self {
        let mut words = 0;
        let mut characters = 0;
        let mut in_latin_word = false;

        for character in text.chars() {
            if !matches!(character, '\r' | '\n') {
                characters += character.len_utf16();
            }
            if is_cjk(character) {
                if in_latin_word {
                    words += 1;
                    in_latin_word = false;
                }
                words += 1;
            } else if character.is_whitespace() {
                if in_latin_word {
                    words += 1;
                    in_latin_word = false;
                }
            } else {
                in_latin_word = true;
            }
        }
        if in_latin_word {
            words += 1;
        }

        Self { words, characters }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LengthBand {
    Healthy,
    Rising,
    Caution,
    Warning,
    Danger,
    OverLimit,
}

impl LengthBand {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Rising => "rising",
            Self::Caution => "caution",
            Self::Warning => "warning",
            Self::Danger => "danger",
            Self::OverLimit => "over_limit",
        }
    }
}

impl PhoenixShell {
    pub(super) fn render_left_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h_8()
            .flex_shrink_0()
            .flex()
            .items_center()
            .px_3()
            .border_t_1()
            .border_r_1()
            .border_color(rgb(BORDER))
            .bg(rgb(FOOTER_BG))
            .text_xs()
            .text_color(rgb(TEXT_MUTED))
            .child(div().text_color(rgb(ACCENT)).child("LOCAL / NATIVE"))
            .child(
                div().ml_auto().child(
                    Button::new("footer-toggle-files")
                        .label("FILES")
                        .small()
                        .ghost()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.left_open = false;
                            cx.notify();
                        })),
                ),
            )
    }

    pub(super) fn render_center_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity_count = self.entity_count();
        let motion_arm = self
            .footer_motion_arm
            .render_arm(self.footer_pulse_active());
        let graph_available = self.graph.borrow().is_some()
            || self.scene_error.is_some()
            || self.graph_init_error.is_some();
        let alert = actionable_status(&self.status);
        div()
            .h_8()
            .min_w_0()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .border_t_1()
            .border_color(rgb(BORDER))
            .bg(rgb(SURFACE))
            .text_xs()
            .text_color(rgb(TEXT_MUTED))
            .child(
                Button::new("footer-entity-pill")
                    .label(format!("ATLAS  ·  {entity_count} ENTITIES"))
                    .small()
                    .rounded(px(14.))
                    .border_1()
                    .border_color(rgb(if self.drawer_layout.is_open() {
                        ACCENT
                    } else {
                        BORDER_BRIGHT
                    }))
                    .bg(rgb(if self.drawer_layout.is_open() {
                        0x173b32
                    } else {
                        0x202423
                    }))
                    .text_color(rgb(ACCENT))
                    .disabled(!graph_available)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_drawer(cx);
                    })),
            )
            .child(
                Button::new("footer-atlas-full-page")
                    .label(if self.drawer_layout.is_full_page() {
                        "RESTORE"
                    } else {
                        "FULL"
                    })
                    .small()
                    .rounded(px(14.))
                    .border_1()
                    .border_color(rgb(if self.drawer_layout.is_full_page() {
                        ACCENT
                    } else {
                        BORDER_BRIGHT
                    }))
                    .bg(rgb(if self.drawer_layout.is_full_page() {
                        0x173b32
                    } else {
                        0x202423
                    }))
                    .text_color(rgb(if self.drawer_layout.is_full_page() {
                        ACCENT
                    } else {
                        TEXT_MUTED
                    }))
                    .disabled(!graph_available)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_drawer_full_page(cx);
                    })),
            )
            .when_some(alert, |footer, alert| {
                footer.child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .truncate()
                        .text_color(rgb(0xe6a06f))
                        .child(alert),
                )
            })
            .child(
                div()
                    .ml_auto()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .mr_1()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(metric_text(
                                "footer-word-health",
                                format!("{} words", self.document_metrics.words),
                                word_band(self.document_metrics.words),
                                motion_arm,
                            ))
                            .child(metric_text(
                                "footer-character-health",
                                format!("{} chars", self.document_metrics.characters),
                                character_band(self.document_metrics.characters),
                                motion_arm,
                            )),
                    )
                    .when(!self.left_open, |controls| {
                        controls.child(
                            Button::new("footer-show-files")
                                .label("FILES")
                                .small()
                                .ghost()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.left_open = true;
                                    cx.notify();
                                })),
                        )
                    })
                    .when(!self.right_open, |controls| {
                        controls.child(
                            Button::new("footer-show-inspector")
                                .label("INSPECTOR")
                                .small()
                                .ghost()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.right_open = true;
                                    cx.notify();
                                })),
                        )
                    }),
            )
    }

    pub(super) fn render_right_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h_8()
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_end()
            .px_3()
            .border_t_1()
            .border_l_1()
            .border_color(rgb(BORDER))
            .bg(rgb(FOOTER_BG))
            .child(
                Button::new("footer-toggle-inspector")
                    .label(self.right_sidebar_page.label())
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.right_open = false;
                        cx.notify();
                    })),
            )
    }
}

fn metric_text(
    id: &'static str,
    label: String,
    band: LengthBand,
    motion_arm: FooterMotionArm,
) -> gpui::AnyElement {
    let text = GradientText::new(label, band_palette(band))
        .cycles(0.82)
        .max_color_runs(12);
    if uses_animation(motion_arm, band) {
        text.animated(id, Duration::from_millis(5_200))
            .into_any_element()
    } else {
        text.into_any_element()
    }
}

const fn uses_animation(motion_arm: FooterMotionArm, band: LengthBand) -> bool {
    matches!(motion_arm, FooterMotionArm::Animated) && animation_enabled(band)
}

fn palette(colors: [u32; 2]) -> GradientPalette {
    GradientPalette::from_hex(colors).unwrap_or_else(|_| GradientPalette::phoenix())
}

fn band_palette(band: LengthBand) -> GradientPalette {
    match band {
        LengthBand::Healthy => HEALTHY_PALETTE.clone(),
        LengthBand::Rising => RISING_PALETTE.clone(),
        LengthBand::Caution => CAUTION_PALETTE.clone(),
        LengthBand::Warning => WARNING_PALETTE.clone(),
        LengthBand::Danger => DANGER_PALETTE.clone(),
        LengthBand::OverLimit => OVER_LIMIT_PALETTE.clone(),
    }
}

const fn word_band(words: usize) -> LengthBand {
    match words {
        0..3_000 => LengthBand::Healthy,
        3_000..4_500 => LengthBand::Caution,
        4_500..6_000 => LengthBand::Warning,
        6_000..7_500 => LengthBand::Danger,
        _ => LengthBand::OverLimit,
    }
}

const fn character_band(characters: usize) -> LengthBand {
    match characters {
        0..15_000 => LengthBand::Healthy,
        15_000..25_000 => LengthBand::Rising,
        25_000..40_000 => LengthBand::Caution,
        40_000..42_500 => LengthBand::Warning,
        42_500..50_000 => LengthBand::Danger,
        _ => LengthBand::OverLimit,
    }
}

const fn animation_enabled(band: LengthBand) -> bool {
    matches!(
        band,
        LengthBand::Warning | LengthBand::Danger | LengthBand::OverLimit
    )
}

const fn is_cjk(character: char) -> bool {
    matches!(
        character as u32,
        0x4e00..=0x9fff
            | 0x3400..=0x4dbf
            | 0x20000..=0x2a6df
            | 0xf900..=0xfaff
            | 0x2e80..=0x2eff
            | 0x2f00..=0x2fdf
            | 0x3040..=0x30ff
            | 0xac00..=0xd7af
    )
}

fn actionable_status(status: &SharedString) -> Option<SharedString> {
    let status_text: &str = status.as_ref();
    ["BLOCKED", "SAVE BLOCKED", "GRAPH BLOCKED", "CONFIRM"]
        .into_iter()
        .any(|prefix| status_text.starts_with(prefix))
        .then(|| status.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routine_status_does_not_pollute_the_footer() {
        assert!(actionable_status(&"READY / SHARED NATIVE KERNEL ONLINE".into()).is_none());
        assert!(actionable_status(&"ATLAS / DRAWER OPEN / RESIDENT GRAPH READY".into()).is_none());
    }

    #[test]
    fn blocking_and_confirmation_status_remain_visible() {
        assert!(actionable_status(&"BLOCKED / SAVE FIRST".into()).is_some());
        assert!(actionable_status(&"SAVE BLOCKED / STALE LEASE".into()).is_some());
        assert!(actionable_status(&"GRAPH BLOCKED / SURFACE LOST".into()).is_some());
        assert!(actionable_status(&"CONFIRM / DELETE BRANCH".into()).is_some());
    }

    #[test]
    fn document_metrics_count_visible_characters_and_mixed_words_once() {
        assert_eq!(
            DocumentMetrics::from_text("hello world\r\n中文"),
            DocumentMetrics {
                words: 4,
                characters: 13,
            }
        );
        assert_eq!(
            DocumentMetrics::from_text("A👩🏽‍🚀Z").characters,
            9,
            "character health keeps the Angular UTF-16 counting contract"
        );
    }

    #[test]
    fn word_health_thresholds_match_the_angular_contract() {
        assert_eq!(word_band(2_999), LengthBand::Healthy);
        assert_eq!(word_band(3_000), LengthBand::Caution);
        assert_eq!(word_band(4_500), LengthBand::Warning);
        assert_eq!(word_band(6_000), LengthBand::Danger);
        assert_eq!(word_band(7_500), LengthBand::OverLimit);
    }

    #[test]
    fn character_health_thresholds_preserve_the_long_note_warning() {
        assert_eq!(character_band(14_999), LengthBand::Healthy);
        assert_eq!(character_band(15_000), LengthBand::Rising);
        assert_eq!(character_band(25_000), LengthBand::Caution);
        assert_eq!(character_band(40_000), LengthBand::Warning);
        assert_eq!(character_band(42_500), LengthBand::Danger);
        assert_eq!(character_band(50_000), LengthBand::OverLimit);
    }

    #[test]
    fn animation_is_reserved_for_actionable_length_bands() {
        assert!(!animation_enabled(LengthBand::Healthy));
        assert!(!animation_enabled(LengthBand::Rising));
        assert!(!animation_enabled(LengthBand::Caution));
        assert!(animation_enabled(LengthBand::Warning));
        assert!(animation_enabled(LengthBand::Danger));
        assert!(animation_enabled(LengthBand::OverLimit));
    }

    #[test]
    fn footer_motion_arms_are_explicit_and_parseable() {
        assert_eq!(FooterMotionArm::default(), FooterMotionArm::Pulse);
        assert_eq!(
            FooterMotionArm::parse("pulse"),
            Some(FooterMotionArm::Pulse)
        );
        assert_eq!(
            FooterMotionArm::parse("static"),
            Some(FooterMotionArm::Static)
        );
        assert_eq!(FooterMotionArm::parse("unknown"), None);
        assert!(!uses_animation(
            FooterMotionArm::Static,
            LengthBand::OverLimit
        ));
        assert_eq!(
            FooterMotionArm::Pulse.render_arm(true),
            FooterMotionArm::Animated
        );
        assert_eq!(
            FooterMotionArm::Pulse.render_arm(false),
            FooterMotionArm::Static
        );
        assert!(uses_animation(
            FooterMotionArm::Animated,
            LengthBand::OverLimit
        ));
        assert!(!uses_animation(
            FooterMotionArm::Animated,
            LengthBand::Healthy
        ));
    }
}
