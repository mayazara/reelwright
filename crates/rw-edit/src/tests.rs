use super::*;
use crate::commands::*;
use proptest::prelude::*;
use rw_model::*;
use std::path::PathBuf;

fn media(n: u8) -> MediaRef {
    MediaRef {
        id: MediaId::new(),
        name: format!("clip{n}.mp4"),
        path: PathBuf::from(format!("/media/clip{n}.mp4")),
        hash: ContentHash(format!("{n:032}")),
        size_bytes: u64::from(n) * 1000,
        info: MediaInfo {
            kind: MediaKind::Video,
            container: "mp4".into(),
            duration: Time::from_secs(i64::from(n) + 1),
            video: None,
            audio: None,
        },
        imported_at: 0,
    }
}

fn project_with_media(count: u8) -> Project {
    let mut p = Project::new("Test", Canvas::default());
    p.media = (0..count).map(media).collect();
    p
}

#[test]
fn undo_redo_restores_states() {
    let mut p = Project::new("Test", Canvas::default());
    let original = p.clone();
    let mut h = History::new();
    assert!(!h.is_dirty());

    h.execute(&mut p, Box::new(AddMedia::new(media(1))))
        .unwrap();
    h.execute(&mut p, Box::new(RenameProject::new("Renamed")))
        .unwrap();
    let after = p.clone();
    assert!(h.is_dirty());

    assert_eq!(h.undo(&mut p).as_deref(), Some("Rename project"));
    h.undo(&mut p);
    assert_eq!(p, original);
    assert!(!h.is_dirty(), "back at the saved state");
    assert!(h.undo(&mut p).is_none());

    h.redo(&mut p);
    h.redo(&mut p);
    assert_eq!(p, after);
}

#[test]
fn new_command_clears_redo() {
    let mut p = Project::new("Test", Canvas::default());
    let mut h = History::new();
    h.execute(&mut p, Box::new(RenameProject::new("A")))
        .unwrap();
    h.undo(&mut p);
    assert!(h.can_redo());
    h.execute(&mut p, Box::new(RenameProject::new("B")))
        .unwrap();
    assert!(!h.can_redo());
}

#[test]
fn failed_command_is_not_recorded() {
    let mut p = Project::new("Test", Canvas::default());
    let mut h = History::new();
    let before = p.clone();
    assert!(
        h.execute(&mut p, Box::new(RenameProject::new("  ")))
            .is_err()
    );
    assert!(
        h.execute(&mut p, Box::new(RemoveMedia::new(MediaId::new())))
            .is_err()
    );
    assert_eq!(p, before);
    assert!(!h.can_undo());
}

#[test]
fn compound_rolls_back_on_failure() {
    let mut p = project_with_media(2);
    let before = p.clone();
    let id = p.media[0].id;
    let mut h = History::new();
    let bad = Compound::new(
        "Remove twice",
        vec![
            Box::new(RemoveMedia::new(id)),
            Box::new(RemoveMedia::new(id)),
        ],
    );
    assert!(h.execute(&mut p, Box::new(bad)).is_err());
    assert_eq!(p, before);
}

#[test]
fn used_media_cannot_be_removed() {
    let mut p = project_with_media(1);
    let id = p.media[0].id;
    let mut track = Track::new_main();
    track.items.push(Item::new(&p.media[0], Time::ZERO));
    p.tracks.push(track);
    let mut h = History::new();
    assert!(h.execute(&mut p, Box::new(RemoveMedia::new(id))).is_err());
}

#[test]
fn saved_state_tracks_position() {
    let mut p = Project::new("Test", Canvas::default());
    let mut h = History::new();
    h.execute(&mut p, Box::new(RenameProject::new("A")))
        .unwrap();
    h.mark_saved();
    assert!(!h.is_dirty());
    h.undo(&mut p);
    assert!(h.is_dirty());
    h.redo(&mut p);
    assert!(!h.is_dirty());
}

/// An abstract edit, resolved against the project at the time it runs.
#[derive(Debug, Clone)]
enum Op {
    Add(u8),
    Remove(usize),
    RenameMedia(usize, String),
    RenameProject(String),
    Canvas(usize),
    Undo,
    Redo,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        any::<u8>().prop_map(Op::Add),
        (0usize..8).prop_map(Op::Remove),
        (0usize..8, "[a-z]{0,6}").prop_map(|(i, s)| Op::RenameMedia(i, s)),
        "[a-z ]{0,6}".prop_map(Op::RenameProject),
        (0usize..AspectPreset::ALL.len()).prop_map(Op::Canvas),
        Just(Op::Undo),
        Just(Op::Redo),
    ]
}

fn command_for(op: &Op, p: &Project) -> Option<Box<dyn Command>> {
    let pick = |i: usize| p.media.get(i % p.media.len().max(1)).map(|m| m.id);
    Some(match op {
        Op::Add(n) => Box::new(AddMedia::new(media(*n))),
        Op::Remove(i) => Box::new(RemoveMedia::new(pick(*i)?)),
        Op::RenameMedia(i, s) => Box::new(RenameMedia::new(pick(*i)?, s.clone())),
        Op::RenameProject(s) => Box::new(RenameProject::new(s.clone())),
        Op::Canvas(i) => Box::new(SetCanvas::new(Canvas::from_preset(AspectPreset::ALL[*i]))),
        Op::Undo | Op::Redo => return None,
    })
}

proptest! {
    /// Any sequence of edits, undos and redos can be fully undone back to the
    /// starting project and fully redone to the newest one.
    #[test]
    fn history_round_trips(ops in proptest::collection::vec(op(), 0..40)) {
        let start = project_with_media(3);
        let mut p = start.clone();
        let mut h = History::new();
        let mut snapshots = vec![p.clone()];
        for op in &ops {
            match op {
                Op::Undo => {
                    if h.undo(&mut p).is_some() {
                        snapshots.pop();
                        prop_assert_eq!(&p, snapshots.last().unwrap());
                    }
                }
                Op::Redo => {
                    if h.redo(&mut p).is_some() {
                        snapshots.push(p.clone());
                    }
                }
                other => {
                    if let Some(cmd) = command_for(other, &p) {
                        let before = p.clone();
                        if h.execute(&mut p, cmd).is_ok() {
                            snapshots.push(p.clone());
                        } else {
                            prop_assert_eq!(&p, &before);
                        }
                    }
                }
            }
        }
        // Redo anything still undone, so `tip` is the newest state in history.
        while h.redo(&mut p).is_some() {}
        let tip = p.clone();
        while h.undo(&mut p).is_some() {}
        prop_assert_eq!(&p, &start);
        while h.redo(&mut p).is_some() {}
        prop_assert_eq!(&p, &tip);
    }
}

#[test]
fn markers_add_move_remove_undo() {
    let mut p = Project::new("m", Canvas::default());
    let mut h = History::new();
    let add = AddMarker::new(Time::from_secs(3));
    let id = add.id();
    h.execute(&mut p, Box::new(add)).unwrap();
    assert!(
        h.execute(&mut p, Box::new(AddMarker::new(Time::from_secs(3))))
            .is_err(),
        "no duplicates"
    );
    h.execute(&mut p, Box::new(AddMarker::new(Time::from_secs(1))))
        .unwrap();
    assert_eq!(p.markers[0].at, Time::from_secs(1), "kept sorted");
    h.execute(&mut p, Box::new(MoveMarker::new(id, Time::ZERO)))
        .unwrap();
    assert_eq!(p.markers[0].id, id);
    h.execute(&mut p, Box::new(RemoveMarker::new(id))).unwrap();
    assert_eq!(p.markers.len(), 1);
    while h.undo(&mut p).is_some() {}
    assert!(p.markers.is_empty());
}
