use crate::{ColorSpace, GradientDraft, GradientStopId};
use gpui::{
    div, prelude::*, px, App, Context, Entity, EventEmitter, Hsla, IntoElement, Render, RenderOnce,
    Subscription, Window,
};
use gpui_component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};

/// Event emitted after the authoring draft changes.
#[derive(Clone, Debug, PartialEq)]
pub enum GradientPickerEvent {
    /// The complete new draft; consumers never have to reconstruct deltas.
    Change(GradientDraft),
}

/// Stateful bridge between a [`GradientDraft`] and GPUI Component's picker.
pub struct GradientPickerState {
    draft: GradientDraft,
    active: Option<GradientStopId>,
    picker: Entity<ColorPickerState>,
    _picker_subscription: Subscription,
}

impl GradientPickerState {
    /// Creates an editor and selects its first stop, if present.
    pub fn new(draft: GradientDraft, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let active = draft.stops().first().map(|stop| stop.id);
        let active_color = draft.stops().first().map(|stop| stop.color);
        let picker = cx.new(|cx| {
            let state = ColorPickerState::new(window, cx);
            match active_color {
                Some(color) => state.default_value(color),
                None => state,
            }
        });
        let subscription = cx.subscribe_in(
            &picker,
            window,
            |this, _, event: &ColorPickerEvent, _, cx| {
                let ColorPickerEvent::Change(Some(color)) = event else {
                    return;
                };
                let id = this.active.unwrap_or_else(|| {
                    let id = this.draft.insert_color(None, *color);
                    this.active = Some(id);
                    id
                });
                this.draft.set_color(id, *color);
                this.emit_change(cx);
            },
        );
        Self {
            draft,
            active,
            picker,
            _picker_subscription: subscription,
        }
    }

    /// Returns the current authoring draft.
    pub fn draft(&self) -> &GradientDraft {
        &self.draft
    }

    /// Returns the selected stop.
    pub const fn active_stop(&self) -> Option<GradientStopId> {
        self.active
    }

    /// Replaces the draft and selects its first stop.
    pub fn set_draft(&mut self, draft: GradientDraft, window: &mut Window, cx: &mut Context<Self>) {
        self.draft = draft;
        self.active = self.draft.stops().first().map(|stop| stop.id);
        self.sync_picker(window, cx);
        self.emit_change(cx);
    }

    /// Selects a stop without changing the draft.
    pub fn select_stop(
        &mut self,
        id: GradientStopId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.draft.stops().iter().any(|stop| stop.id == id) {
            return false;
        }
        self.active = Some(id);
        self.sync_picker(window, cx);
        cx.notify();
        true
    }

    /// Adds a color after the active stop and selects it.
    pub fn add_color(
        &mut self,
        color: Hsla,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> GradientStopId {
        let id = self.draft.insert_color(self.active, color);
        self.active = Some(id);
        self.sync_picker(window, cx);
        self.emit_change(cx);
        id
    }

    /// Removes the active stop.
    pub fn remove_active(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(id) = self.active else {
            return false;
        };
        if !self.draft.remove(id) {
            return false;
        }
        self.active = self.draft.stops().first().map(|stop| stop.id);
        self.sync_picker(window, cx);
        self.emit_change(cx);
        true
    }

    /// Moves the active stop by one or more slots.
    pub fn move_active(&mut self, delta: isize, cx: &mut Context<Self>) -> bool {
        let Some(id) = self.active else {
            return false;
        };
        if !self.draft.move_stop(id, delta) {
            return false;
        }
        self.emit_change(cx);
        true
    }

    /// Nudges the active stop position while preserving sorted order.
    pub fn nudge_active(&mut self, delta: f32, cx: &mut Context<Self>) -> bool {
        let Some(id) = self.active else {
            return false;
        };
        let Some(position) = self
            .draft
            .stops()
            .iter()
            .find(|stop| stop.id == id)
            .map(|stop| stop.position)
        else {
            return false;
        };
        if !self.draft.set_position(id, position + delta) {
            return false;
        }
        self.emit_change(cx);
        true
    }

    /// Changes interpolation space.
    pub fn set_color_space(&mut self, color_space: ColorSpace, cx: &mut Context<Self>) {
        self.draft.set_color_space(color_space);
        self.emit_change(cx);
    }

    fn active_color(&self) -> Option<Hsla> {
        let active = self.active?;
        self.draft
            .stops()
            .iter()
            .find(|stop| stop.id == active)
            .map(|stop| stop.color)
    }

    fn sync_picker(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(color) = self.active_color() else {
            return;
        };
        self.picker
            .update(cx, |picker, cx| picker.set_value(color, window, cx));
    }

    fn emit_change(&self, cx: &mut Context<Self>) {
        cx.emit(GradientPickerEvent::Change(self.draft.clone()));
        cx.notify();
    }
}

impl EventEmitter<GradientPickerEvent> for GradientPickerState {}

impl Render for GradientPickerState {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let featured = self
            .draft
            .stops()
            .iter()
            .map(|stop| stop.color)
            .collect::<Vec<_>>();
        let mut stop_row = div().flex().items_center().gap_2();
        for stop in self.draft.stops() {
            let id = stop.id;
            let is_active = self.active == Some(id);
            stop_row = stop_row.child(
                div()
                    .id(("gradient-stop", id.0))
                    .w(px(30.0))
                    .h(px(30.0))
                    .rounded_md()
                    .bg(stop.color)
                    .border_2()
                    .border_color(if is_active {
                        gpui::white()
                    } else {
                        gpui::transparent_black()
                    })
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select_stop(id, window, cx);
                    })),
            );
        }

        let active_color = self.active_color().unwrap_or_else(gpui::white);
        let controls = div()
            .flex()
            .flex_wrap()
            .gap_2()
            .child(control(
                "gradient-add",
                "Add",
                cx.listener(move |this: &mut Self, _, window, cx| {
                    this.add_color(active_color, window, cx);
                }),
            ))
            .child(control(
                "gradient-remove",
                "Remove",
                cx.listener(|this: &mut Self, _, window, cx| {
                    this.remove_active(window, cx);
                }),
            ))
            .child(control(
                "gradient-left",
                "Move left",
                cx.listener(|this: &mut Self, _, _, cx| {
                    this.move_active(-1, cx);
                }),
            ))
            .child(control(
                "gradient-right",
                "Move right",
                cx.listener(|this: &mut Self, _, _, cx| {
                    this.move_active(1, cx);
                }),
            ))
            .child(control(
                "gradient-pos-minus",
                "Position -",
                cx.listener(|this: &mut Self, _, _, cx| {
                    this.nudge_active(-0.05, cx);
                }),
            ))
            .child(control(
                "gradient-pos-plus",
                "Position +",
                cx.listener(|this: &mut Self, _, _, cx| {
                    this.nudge_active(0.05, cx);
                }),
            ));

        let spaces = div().flex().gap_2().children([
            color_space_control(ColorSpace::Srgb, self.draft.color_space(), cx),
            color_space_control(ColorSpace::LinearSrgb, self.draft.color_space(), cx),
            color_space_control(ColorSpace::Oklab, self.draft.color_space(), cx),
        ]);

        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(stop_row)
            .child(
                ColorPicker::new(&self.picker)
                    .featured_colors(featured)
                    .label("Selected stop"),
            )
            .child(controls)
            .child(spaces)
    }
}

/// Cloneable element that renders a [`GradientPickerState`].
#[derive(Clone, IntoElement)]
pub struct GradientPicker {
    state: Entity<GradientPickerState>,
}

impl GradientPicker {
    /// Creates an element for an existing picker state.
    pub fn new(state: &Entity<GradientPickerState>) -> Self {
        Self {
            state: state.clone(),
        }
    }
}

impl RenderOnce for GradientPicker {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.state.clone()
    }
}

fn control(
    id: &'static str,
    label: &'static str,
    listener: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .px_2()
        .py_1()
        .rounded_md()
        .bg(gpui::black().opacity(0.18))
        .cursor_pointer()
        .child(label)
        .on_click(listener)
}

fn color_space_control(
    color_space: ColorSpace,
    active: ColorSpace,
    cx: &mut Context<GradientPickerState>,
) -> impl IntoElement {
    let label = match color_space {
        ColorSpace::Srgb => "sRGB",
        ColorSpace::LinearSrgb => "Linear sRGB",
        ColorSpace::Oklab => "Oklab",
    };
    div()
        .id(("gradient-space", color_space as usize))
        .px_2()
        .py_1()
        .rounded_md()
        .bg(if color_space == active {
            gpui::white().opacity(0.18)
        } else {
            gpui::black().opacity(0.18)
        })
        .cursor_pointer()
        .child(label)
        .on_click(cx.listener(move |this, _, _, cx| {
            this.set_color_space(color_space, cx);
        }))
}
