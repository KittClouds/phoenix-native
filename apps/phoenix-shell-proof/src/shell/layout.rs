use super::{
    LEFT_SIDEBAR_MAX_WIDTH, LEFT_SIDEBAR_MIN_WIDTH, RIGHT_SIDEBAR_MAX_WIDTH,
    RIGHT_SIDEBAR_MIN_WIDTH,
};

pub(super) const NAV_RAIL_WIDTH: f32 = 56.;
const CENTER_COMFORT_MIN_WIDTH: f32 = 480.;
const CENTER_HARD_MIN_WIDTH: f32 = 240.;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ShellWidthBudget {
    pub left_width: f32,
    pub left_min: f32,
    pub center_min: f32,
    pub center_width: f32,
    pub right_width: f32,
    pub right_min: f32,
}

/// Reconciles persisted pane preferences with the current window before the
/// resizable component applies its own bounds. The preferences remain intact;
/// only their render-time projection is reduced when the window is narrow.
pub(super) fn shell_width_budget(
    viewport_width: f32,
    left_open: bool,
    right_open: bool,
    desired_left: f32,
    desired_right: f32,
) -> ShellWidthBudget {
    let rail_width = if left_open { 0. } else { NAV_RAIL_WIDTH };
    let available = (viewport_width.max(0.) - rail_width).max(0.);
    let desired_left = if left_open {
        desired_left.clamp(LEFT_SIDEBAR_MIN_WIDTH, LEFT_SIDEBAR_MAX_WIDTH)
    } else {
        0.
    };
    let desired_right = if right_open {
        desired_right.clamp(RIGHT_SIDEBAR_MIN_WIDTH, RIGHT_SIDEBAR_MAX_WIDTH)
    } else {
        0.
    };
    let desired_sides = desired_left + desired_right;

    let center_floor = if available >= desired_sides + CENTER_COMFORT_MIN_WIDTH {
        CENTER_COMFORT_MIN_WIDTH
    } else {
        (available * 0.5)
            .clamp(CENTER_HARD_MIN_WIDTH, CENTER_COMFORT_MIN_WIDTH)
            .min(available)
    };
    let side_budget = (available - center_floor).max(0.);
    let side_scale = if desired_sides > side_budget && desired_sides > 0. {
        side_budget / desired_sides
    } else {
        1.
    };
    let left_width = desired_left * side_scale;
    let right_width = desired_right * side_scale;
    let center_width = (available - left_width - right_width).max(0.);

    ShellWidthBudget {
        left_width,
        left_min: LEFT_SIDEBAR_MIN_WIDTH.min(left_width),
        center_min: center_floor.min(center_width),
        center_width,
        right_width,
        right_min: RIGHT_SIDEBAR_MIN_WIDTH.min(right_width),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct SplitWidthBudget {
    pub sidebar_width: f32,
    pub sidebar_min: f32,
    pub content_min: f32,
}

pub(super) fn split_width_budget(
    available: f32,
    desired_sidebar: f32,
    preferred_sidebar_min: f32,
    sidebar_max: f32,
    content_comfort_min: f32,
) -> SplitWidthBudget {
    let available = available.max(0.);
    let desired_sidebar = desired_sidebar.clamp(preferred_sidebar_min, sidebar_max);
    let content_floor = if available >= desired_sidebar + content_comfort_min {
        content_comfort_min
    } else {
        (available * 0.55)
            .max(CENTER_HARD_MIN_WIDTH)
            .min(content_comfort_min)
            .min(available)
    };
    let sidebar_width = desired_sidebar.min((available - content_floor).max(0.));
    SplitWidthBudget {
        sidebar_width,
        sidebar_min: preferred_sidebar_min.min(sidebar_width),
        content_min: content_floor.min(available - sidebar_width),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn total(budget: ShellWidthBudget) -> f32 {
        budget.left_width + budget.center_width + budget.right_width
    }

    #[test]
    fn wide_shell_preserves_user_sidebar_preferences() {
        let budget = shell_width_budget(1_600., true, true, 344., 400.);
        assert_eq!(budget.left_width, 344.);
        assert_eq!(budget.right_width, 400.);
        assert_eq!(budget.center_width, 856.);
        assert_eq!(budget.center_min, CENTER_COMFORT_MIN_WIDTH);
    }

    #[test]
    fn narrow_shell_softens_all_minima_without_overflow() {
        for (left_open, right_open) in [(false, false), (true, false), (false, true), (true, true)]
        {
            for viewport in [1_100., 900., 600., 320., 120.] {
                let budget = shell_width_budget(viewport, left_open, right_open, 520., 480.);
                let rail = if left_open {
                    0.
                } else {
                    NAV_RAIL_WIDTH.min(viewport)
                };
                assert!((total(budget) + rail - viewport).abs() < 0.01);
                assert!(budget.left_min <= budget.left_width);
                assert!(budget.right_min <= budget.right_width);
                assert!(budget.center_min <= budget.center_width);
            }
        }
    }

    #[test]
    fn collapsed_left_rail_is_part_of_the_width_budget() {
        let budget = shell_width_budget(800., false, true, 344., 320.);
        assert!((total(budget) + NAV_RAIL_WIDTH - 800.).abs() < 0.01);
    }

    #[test]
    fn expansion_restores_preferences_instead_of_persisting_soft_widths() {
        let narrow = shell_width_budget(700., true, true, 344., 320.);
        assert!(narrow.left_width < 344.);
        assert!(narrow.right_width < 320.);
        let expanded = shell_width_budget(1_600., true, true, 344., 320.);
        assert_eq!(expanded.left_width, 344.);
        assert_eq!(expanded.right_width, 320.);
    }

    #[test]
    fn nested_sidebar_and_content_always_fit_the_center() {
        for width in [900., 600., 420., 240., 100.] {
            let budget = split_width_budget(width, 336., 236., 520., 360.);
            assert!(budget.sidebar_min <= budget.sidebar_width);
            assert!(budget.content_min <= width - budget.sidebar_width);
            assert!(budget.sidebar_width + budget.content_min <= width + 0.01);
        }
    }
}
