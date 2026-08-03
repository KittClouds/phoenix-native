use anyhow::{anyhow, Context, Result};
use raw_window_handle::{HasWindowHandle, RawWindowHandle, Win32WindowHandle};
use std::ffi::c_void;
use std::num::NonZeroIsize;
use std::sync::Mutex;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_STYLE, SWP_FRAMECHANGED,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WS_CLIPCHILDREN, WS_CLIPSIBLINGS,
};
use winit::window::Window;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewportGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f32,
    pub visible: bool,
}

impl ViewportGeometry {
    pub(super) fn hidden() -> Self {
        Self {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
            scale_factor: 1.0,
            visible: false,
        }
    }

    fn validate(self) -> Result<Self> {
        if !self.scale_factor.is_finite() || self.scale_factor <= 0.0 {
            return Err(anyhow!(
                "invalid graph viewport scale factor {}",
                self.scale_factor
            ));
        }
        if self.width > i32::MAX as u32 || self.height > i32::MAX as u32 {
            return Err(anyhow!(
                "graph viewport exceeds Win32 bounds: {}x{}",
                self.width,
                self.height
            ));
        }
        Ok(self)
    }

    pub(super) fn position_changed_from(self, previous: Self) -> bool {
        self.x != previous.x || self.y != previous.y
    }

    pub(super) fn framebuffer_changed_from(self, previous: Self) -> bool {
        self.width != previous.width
            || self.height != previous.height
            || self.scale_factor.to_bits() != previous.scale_factor.to_bits()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ParentWindowHandle {
    hwnd: NonZeroIsize,
    hinstance: Option<NonZeroIsize>,
}

impl ParentWindowHandle {
    pub fn new(hwnd: NonZeroIsize, hinstance: Option<NonZeroIsize>) -> Self {
        Self { hwnd, hinstance }
    }

    pub(super) fn raw(self) -> RawWindowHandle {
        let mut handle = Win32WindowHandle::new(self.hwnd);
        handle.hinstance = self.hinstance;
        RawWindowHandle::Win32(handle)
    }

    pub(super) fn hwnd(self) -> HWND {
        HWND(self.hwnd.get() as *mut c_void)
    }

    pub(super) fn prepare_for_child_hosting(self) -> Result<()> {
        ensure_window_style(self.hwnd(), WS_CLIPCHILDREN.0).context("enable GPUI child clipping")
    }

    pub(super) fn clips_children(self) -> bool {
        has_window_style(self.hwnd(), WS_CLIPCHILDREN.0)
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct StampedGeometry {
    pub revision: u64,
    pub geometry: ViewportGeometry,
}

#[derive(Debug)]
struct ViewportState {
    latest: StampedGeometry,
    wake_pending: bool,
    proof_active: bool,
}

#[derive(Debug)]
pub(super) struct ViewportMailbox {
    state: Mutex<ViewportState>,
}

impl ViewportMailbox {
    pub(super) fn new() -> Self {
        Self {
            state: Mutex::new(ViewportState {
                latest: StampedGeometry {
                    revision: 0,
                    geometry: ViewportGeometry::hidden(),
                },
                wake_pending: false,
                proof_active: false,
            }),
        }
    }

    pub(super) fn publish_ui(&self, geometry: ViewportGeometry) -> Result<bool> {
        let geometry = geometry.validate()?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow!("graph viewport mailbox is poisoned"))?;
        if state.proof_active {
            return Ok(false);
        }
        Ok(Self::record(&mut state, geometry))
    }

    pub(super) fn publish_proof(&self, geometry: ViewportGeometry) -> Result<bool> {
        let geometry = geometry.validate()?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow!("graph viewport mailbox is poisoned"))?;
        Ok(Self::record(&mut state, geometry))
    }

    fn record(state: &mut ViewportState, geometry: ViewportGeometry) -> bool {
        if state.latest.geometry == geometry {
            return false;
        }
        state.latest = StampedGeometry {
            revision: state.latest.revision.saturating_add(1),
            geometry,
        };
        if state.wake_pending {
            return false;
        }
        state.wake_pending = true;
        true
    }

    pub(super) fn next_after(&self, revision: u64) -> Result<Option<StampedGeometry>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow!("graph viewport mailbox is poisoned"))?;
        if state.latest.revision == revision {
            state.wake_pending = false;
            return Ok(None);
        }
        Ok(Some(state.latest))
    }

    pub(super) fn latest(&self) -> Result<StampedGeometry> {
        self.state
            .lock()
            .map(|state| state.latest)
            .map_err(|_| anyhow!("graph viewport mailbox is poisoned"))
    }

    pub(super) fn begin_proof(&self) -> Result<ViewportProofLease<'_>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow!("graph viewport mailbox is poisoned"))?;
        if state.proof_active {
            return Err(anyhow!("graph viewport proof is already active"));
        }
        state.proof_active = true;
        Ok(ViewportProofLease { mailbox: self })
    }
}

pub(super) struct ViewportProofLease<'a> {
    mailbox: &'a ViewportMailbox,
}

impl Drop for ViewportProofLease<'_> {
    fn drop(&mut self) {
        match self.mailbox.state.lock() {
            Ok(mut state) => state.proof_active = false,
            Err(_) => crate::lifecycle::mark_proof_failed(),
        }
    }
}

pub(super) fn hwnd_for_window(window: &Window) -> Result<HWND> {
    match HasWindowHandle::window_handle(window)
        .context("read graph HWND")?
        .as_raw()
    {
        RawWindowHandle::Win32(handle) => Ok(HWND(handle.hwnd.get() as *mut c_void)),
        other => Err(anyhow!("graph window is not Win32: {other:?}")),
    }
}

pub(super) fn prepare_child_window(window: &Window) -> Result<HWND> {
    let hwnd = hwnd_for_window(window)?;
    ensure_window_style(hwnd, WS_CLIPSIBLINGS.0).context("enable graph sibling clipping")?;
    Ok(hwnd)
}

fn ensure_window_style(hwnd: HWND, required: u32) -> Result<()> {
    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as u32;
    if style & required == required {
        return Ok(());
    }
    unsafe {
        SetWindowLongPtrW(hwnd, GWL_STYLE, (style | required) as isize);
        SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
        )
        .context("commit Win32 child-host style")?;
    }
    if has_window_style(hwnd, required) {
        Ok(())
    } else {
        Err(anyhow!(
            "Win32 refused required window style 0x{required:08x}"
        ))
    }
}

fn has_window_style(hwnd: HWND, required: u32) -> bool {
    unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 & required == required }
}

#[cfg(test)]
mod tests {
    use super::{ViewportGeometry, ViewportMailbox};

    fn geometry(width: u32) -> ViewportGeometry {
        ViewportGeometry {
            x: 8,
            y: 16,
            width,
            height: 200,
            scale_factor: 1.5,
            visible: true,
        }
    }

    #[test]
    fn viewport_mailbox_is_latest_wins_with_one_pending_wake() {
        let mailbox = ViewportMailbox::new();
        assert!(mailbox.publish_ui(geometry(300)).expect("first publish"));
        assert!(!mailbox
            .publish_ui(geometry(320))
            .expect("coalesced publish"));
        let latest = mailbox
            .next_after(0)
            .expect("read latest")
            .expect("latest geometry");
        assert_eq!(latest.geometry.width, 320);
        assert!(mailbox
            .next_after(latest.revision)
            .expect("drain latest")
            .is_none());
        assert!(mailbox.publish_ui(geometry(340)).expect("next wake"));
    }

    #[test]
    fn viewport_rejects_non_finite_scale() {
        let mailbox = ViewportMailbox::new();
        let mut invalid = geometry(300);
        invalid.scale_factor = f32::NAN;
        assert!(mailbox.publish_ui(invalid).is_err());
    }

    #[test]
    fn position_only_change_does_not_resize_the_framebuffer() {
        let previous = geometry(300);
        let mut moved = previous;
        moved.x += 12;
        moved.y += 20;

        assert!(moved.position_changed_from(previous));
        assert!(!moved.framebuffer_changed_from(previous));
    }

    #[test]
    fn scale_change_requires_a_framebuffer_reconfiguration() {
        let previous = geometry(300);
        let mut scaled = previous;
        scaled.scale_factor = 2.0;

        assert!(scaled.framebuffer_changed_from(previous));
    }

    #[test]
    fn proof_lease_suppresses_ui_geometry_without_blocking_proof_geometry() {
        let mailbox = ViewportMailbox::new();
        assert!(mailbox.publish_ui(geometry(300)).expect("initial UI"));
        let initial = mailbox
            .next_after(0)
            .expect("read initial")
            .expect("initial geometry");
        assert!(mailbox
            .next_after(initial.revision)
            .expect("drain initial")
            .is_none());

        {
            let _lease = mailbox.begin_proof().expect("begin proof");
            assert!(!mailbox
                .publish_ui(geometry(320))
                .expect("suppressed UI geometry"));
            assert!(mailbox
                .publish_proof(geometry(340))
                .expect("proof geometry"));
            assert_eq!(mailbox.latest().expect("proof latest").geometry.width, 340);
        }

        assert!(!mailbox
            .publish_ui(geometry(360))
            .expect("coalesced post-proof UI geometry"));
        assert_eq!(
            mailbox.latest().expect("post-proof latest").geometry.width,
            360
        );
    }
}
