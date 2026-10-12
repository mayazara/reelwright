//! Editor actions: what toolbars, menus, the timeline and shortcuts ask the
//! editor to do. One handler applies them, so every entry point behaves the
//! same.

use crate::timeline::logic::DropRow;
use rw_edit::timeline::TimelineOp;
use rw_model::{Generated, MarkerId, MediaId, Time, TransitionKind};

#[derive(Debug, Clone, PartialEq)]
pub enum EditorAction {
    Op(TimelineOp),
    Seek(Time),
    PlayPause,
    SplitAtPlayhead,
    Delete {
        ripple: bool,
    },
    Copy,
    Paste,
    Duplicate,
    Group,
    Ungroup,
    DetachAudio,
    StepFrames(i64),
    PrevEdit,
    NextEdit,
    ZoomIn,
    ZoomOut,
    ZoomFit,
    /// Drop media from the bin onto the timeline.
    InsertMedia {
        media: MediaId,
        row: DropRow,
        at: Time,
    },
    /// Add media at the playhead (bin double-click or "Add to timeline").
    AppendMedia(MediaId),
    /// Add media at the playhead on a new track above the others (below
    /// the others for sound).
    AppendMediaNewTrack(MediaId),
    Export,
    AddMarker,
    /// Add a colour/gradient clip at the playhead on the main track.
    AppendGenerated(Generated),
    InsertGenerated {
        fill: Generated,
        row: DropRow,
        at: Time,
    },
    RemoveMarker(MarkerId),
    MoveMarker(MarkerId, Time),
    /// Hold the selected video clip's frame at the playhead.
    FreezeFrame,
    /// Put a transition on the cut next to the selected clip.
    AddTransition(TransitionKind),
    /// Tell the user something (a hint after an action that did nothing).
    Notice(String),
}
