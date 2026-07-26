use crate::graph_window::{GraphWindow, ParentWindowHandle, ViewportGeometry};
use anyhow::{anyhow, Context, Result};
use gpui::{canvas, prelude::*, rgb, Bounds, IntoElement, Pixels, Window};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use super::CANVAS;

pub(super) fn parent_window_handle(window: &Window) -> Result<ParentWindowHandle> {
    let handle = HasWindowHandle::window_handle(window)
        .context("read GPUI parent window handle")?
        .as_raw();
    match handle {
        RawWindowHandle::Win32(handle) => {
            Ok(ParentWindowHandle::new(handle.hwnd, handle.hinstance))
        }
        other => Err(anyhow!("GPUI parent is not a Win32 window: {other:?}")),
    }
}

pub(super) fn graph_viewport(
    graph: Rc<RefCell<Option<GraphWindow>>>,
    latest_geometry: Rc<Cell<Option<ViewportGeometry>>>,
    visible: bool,
) -> impl IntoElement {
    canvas(
        move |bounds, window, _| {
            let geometry = geometry_from_bounds(bounds, window.scale_factor(), visible);
            latest_geometry.set(Some(geometry));
            if let Some(graph) = graph.borrow().as_ref() {
                if let Err(error) = graph.set_viewport(geometry) {
                    tracing::error!(%error, "embedded graph viewport update failed");
                }
            }
        },
        |_, _, _, _| {},
    )
    .size_full()
    .min_w_0()
    .min_h_0()
    .bg(rgb(CANVAS))
}

fn geometry_from_bounds(
    bounds: Bounds<Pixels>,
    scale_factor: f32,
    visible: bool,
) -> ViewportGeometry {
    let physical = bounds.to_device_pixels(scale_factor);
    ViewportGeometry {
        x: physical.origin.x.0,
        y: physical.origin.y.0,
        width: physical.size.width.0.max(0) as u32,
        height: physical.size.height.0.max(0) as u32,
        scale_factor,
        visible,
    }
}

#[cfg(test)]
mod tests {
    use super::geometry_from_bounds;
    use gpui::{bounds, point, px, size};

    #[test]
    fn viewport_bounds_convert_once_to_physical_pixels() {
        let geometry = geometry_from_bounds(
            bounds(point(px(10.25), px(20.5)), size(px(300.5), px(140.25))),
            2.0,
            true,
        );
        assert_eq!(geometry.x, 21);
        assert_eq!(geometry.y, 41);
        assert_eq!(geometry.width, 601);
        assert_eq!(geometry.height, 281);
        assert_eq!(geometry.scale_factor, 2.0);
        assert!(geometry.visible);
    }
}
