//! Editor actions: what toolbars, menus, the timeline and shortcuts ask the
//! editor to do. One handler applies them, so every entry point behaves the
//! same.

use crate::timeline::logic::DropRow;
use rw_edit::timeline::TimelineOp;
use rw_model::{MarkerId, MediaId, Time};

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
    Export,
    AddMarker,
    RemoveMarker(MarkerId),
    MoveMarker(MarkerId, Time),
}
