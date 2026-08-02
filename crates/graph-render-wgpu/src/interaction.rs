use crate::CameraSnapshot;
use graph_model::NodeId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerButton {
    Left,
    Middle,
    Right,
}

/// A pointer coordinate in the renderer's framebuffer coordinate space.
///
/// `winit` reports cursor positions in physical pixels. Keeping that unit at
/// the host boundary avoids a lossy physical -> logical -> physical round
/// trip when a child window is hosted by a scaled GPUI viewport. The picking
/// texture, camera projection, and surface are all physical-pixel based.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PhysicalPointer {
    pub x: f32,
    pub y: f32,
}

impl PhysicalPointer {
    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GraphInput {
    PointerMoved {
        pointer: PhysicalPointer,
    },
    PointerPressed {
        pointer: PhysicalPointer,
        button: PointerButton,
        shift: bool,
        alt: bool,
    },
    PointerReleased {
        pointer: PhysicalPointer,
        button: PointerButton,
    },
    Wheel {
        delta_x: f32,
        delta_y: f32,
    },
    Resize {
        width: u32,
        height: u32,
        scale_factor: f32,
    },
    FitGraph,
    ResetCamera,
    ClearSelection,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GraphEvent {
    HoverChanged(Option<NodeId>),
    SelectionChanged { sequence: u64, node: Option<NodeId> },
    CameraChanged(CameraSnapshot),
}

#[must_use]
pub fn logical_to_physical(logical: f32, scale_factor: f32) -> f32 {
    logical * scale_factor.max(0.01)
}

#[must_use]
pub fn physical_delta_to_logical(physical: f32, scale_factor: f32) -> f32 {
    physical / scale_factor.max(0.01)
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PointerState {
    pub position: (f32, f32),
    pub left_down: bool,
    pub middle_down: bool,
    pub right_down: bool,
    pub shift_down: bool,
    pub alt_down: bool,
    pub press_origin: Option<(f32, f32)>,
    pub dragged: bool,
}

impl PointerState {
    pub fn update_drag(&mut self, x: f32, y: f32) {
        if let Some((origin_x, origin_y)) = self.press_origin {
            let distance_squared = (x - origin_x).mul_add(x - origin_x, (y - origin_y).powi(2));
            self.dragged |= distance_squared >= 16.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{logical_to_physical, physical_delta_to_logical, PhysicalPointer, PointerState};

    #[test]
    fn coordinate_conversion_respects_dpi_scale() {
        assert_eq!(logical_to_physical(125.0, 1.5), 187.5);
        assert_eq!(logical_to_physical(125.0, 0.0), 1.25);
        assert_eq!(physical_delta_to_logical(187.5, 1.5), 125.0);
        assert_eq!(physical_delta_to_logical(1.25, 0.0), 125.0);
    }

    #[test]
    fn four_pixel_motion_crosses_drag_threshold() {
        let mut pointer = PointerState {
            press_origin: Some((10.0, 10.0)),
            ..PointerState::default()
        };
        pointer.update_drag(13.0, 10.0);
        assert!(!pointer.dragged);
        pointer.update_drag(14.0, 10.0);
        assert!(pointer.dragged);
    }

    #[test]
    fn right_button_state_is_independent_of_selection_drag_state() {
        let pointer = PointerState {
            right_down: true,
            ..PointerState::default()
        };
        assert!(pointer.right_down);
        assert!(!pointer.left_down);
        assert!(!pointer.middle_down);
    }

    #[test]
    fn physical_pointer_keeps_framebuffer_coordinates_unchanged() {
        let pointer = PhysicalPointer::new(3840.0, 2160.0);
        assert_eq!(
            pointer,
            PhysicalPointer {
                x: 3840.0,
                y: 2160.0
            }
        );
    }
}
