//! Typed presentation states; raw provider messages remain in playback details.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Phase {
    #[default]
    Idle,
    Preparing,
    Ready,
    Buffering,
    Playing,
    Paused,
    Completed,
    Changed,
    Stopped,
    Failed,
}

impl Phase {
    pub fn headline(self) -> &'static str {
        match self {
            Self::Idle => "Your page, read aloud",
            Self::Preparing => "Preparing local voice…",
            Self::Ready => "Ready to listen",
            Self::Buffering => "Preparing this passage…",
            Self::Playing => "Reading",
            Self::Paused => "Paused",
            Self::Completed => "Reading complete",
            Self::Changed => "Text changed — save to continue",
            Self::Stopped => "Position saved",
            Self::Failed => "Unable to prepare narration · open Details",
        }
    }

    pub fn primary(self, requested: bool, dirty: bool) -> &'static str {
        if dirty {
            return "Save & listen";
        }
        if self == Self::Preparing {
            return "Cancel";
        }
        if requested {
            return "Pause";
        }
        match self {
            Self::Idle | Self::Changed | Self::Stopped => "Listen",
            Self::Failed => "Retry",
            Self::Completed => "Replay",
            _ => "Resume",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preparation_cancels_but_buffered_playback_pauses() {
        assert_eq!(Phase::Preparing.primary(true, false), "Cancel");
        assert_eq!(Phase::Buffering.primary(true, false), "Pause");
    }
    #[test]
    fn edit_and_failure_have_recovery_actions() {
        assert_eq!(Phase::Playing.primary(true, true), "Save & listen");
        assert_eq!(Phase::Failed.primary(false, false), "Retry");
        assert_eq!(Phase::Completed.primary(false, false), "Replay");
    }
}
