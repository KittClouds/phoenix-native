use crate::GradientText;
use gpui::{
    Animation, AnimationElement, AnimationExt, App, Bounds, CursorStyle, DispatchPhase, Element,
    ElementId, GlobalElementId, Hitbox, InspectorElementId, IntoElement, LayoutId, MouseDownEvent,
    MouseUpEvent, Pixels, StyledText, Window,
};
use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;
use std::time::Duration;

type SelectionListener = dyn Fn(&Range<usize>, &mut Window, &mut App);

/// Read-only drag-selectable gradient text.
pub struct SelectableGradientText {
    id: ElementId,
    text: GradientText,
    on_select: Option<Rc<SelectionListener>>,
}

impl SelectableGradientText {
    /// Creates a selection surface with a stable element ID.
    pub fn new(id: impl Into<ElementId>, text: GradientText) -> Self {
        Self {
            id: id.into(),
            text,
            on_select: None,
        }
    }

    /// Handles a completed grapheme-aligned byte selection.
    pub fn on_select(
        mut self,
        listener: impl Fn(&Range<usize>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_select = Some(Rc::new(listener));
        self
    }

    /// Repeats this selectable gradient on GPUI's frame scheduler.
    pub fn animated(self, id: impl Into<ElementId>, duration: Duration) -> AnimationElement<Self> {
        self.with_animation(
            id,
            Animation::new(duration.max(Duration::from_millis(1))).repeat(),
            |mut text, phase| {
                text.text = text.text.phase(phase);
                text
            },
        )
    }
}

#[derive(Default)]
struct SelectableState {
    mouse_down: Rc<Cell<Option<usize>>>,
}

impl Element for SelectableGradientText {
    type RequestLayoutState = StyledText;
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut styled = self.text.styled_text(&window.text_style());
        let (layout, ()) = styled.request_layout(None, inspector_id, window, cx);
        (layout, styled)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        styled: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        styled.prepaint(None, inspector_id, bounds, &mut (), window, cx);
        window.insert_hitbox(bounds, gpui::HitboxBehavior::Normal)
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        styled: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let layout = styled.layout().clone();
        let offsets = self.text.grapheme_offsets.clone();
        let source_len = self.text.text().len();
        let on_select = self.on_select.take();
        window.set_cursor_style(CursorStyle::IBeam, hitbox);
        window.with_element_state::<SelectableState, _>(
            global_id.expect("selectable gradient has an ID"),
            |state, window| {
                let state = state.unwrap_or_default();
                let mouse_down = state.mouse_down.clone();
                if let Some(start) = mouse_down.get() {
                    let layout = layout.clone();
                    let hitbox = hitbox.clone();
                    window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                        if phase != DispatchPhase::Bubble {
                            return;
                        }
                        if hitbox.is_hovered(window) {
                            if let Ok(end) = layout.index_for_position(event.position) {
                                let range = snap_range(&offsets, source_len, start, end);
                                if let Some(listener) = &on_select {
                                    listener(&range, window, cx);
                                }
                            }
                        }
                        mouse_down.take();
                        window.refresh();
                    });
                } else {
                    let layout = layout.clone();
                    let hitbox = hitbox.clone();
                    window.on_mouse_event(move |event: &MouseDownEvent, phase, window, _cx| {
                        if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                            if let Ok(index) = layout.index_for_position(event.position) {
                                mouse_down.set(Some(index));
                                window.refresh();
                            }
                        }
                    });
                }
                styled.paint(None, inspector_id, _bounds, &mut (), &mut (), window, cx);
                ((), state)
            },
        );
    }
}

impl IntoElement for SelectableGradientText {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

fn snap_range(offsets: &[usize], source_len: usize, a: usize, b: usize) -> Range<usize> {
    let grapheme_count = offsets.len().saturating_sub(1);
    if grapheme_count == 0 {
        return 0..0;
    }
    let lower = a.min(b).min(source_len);
    let upper = a.max(b).min(source_len);
    let start = offsets
        .partition_point(|offset| *offset <= lower)
        .saturating_sub(1)
        .min(grapheme_count - 1);
    let mut end = offsets.partition_point(|offset| *offset < upper);
    if end <= start {
        end = start + 1;
    }
    offsets[start]..offsets[end.min(grapheme_count)]
}
