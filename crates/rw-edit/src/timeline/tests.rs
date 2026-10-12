use super::*;
use crate::History;
use proptest::prelude::*;
use rw_model::*;
use std::path::PathBuf;

const S: fn(i64) -> Time = Time::from_secs;

fn media(name: &str, kind: MediaKind, secs: i64, audio: bool) -> MediaRef {
    MediaRef {
        id: MediaId::new(),
        name: name.into(),
        path: PathBuf::from(name),
        hash: ContentHash(name.into()),
        size_bytes: 1,
        info: MediaInfo {
            kind,
            container: "x".into(),
            duration: if kind == MediaKind::Image {
                Time::ZERO
            } else {
                S(secs)
            },
            video: None,
            audio: audio.then(|| AudioInfo {
                codec: "aac".into(),
                sample_rate: 48_000,
                channels: 2,
            }),
        },
        imported_at: 0,
    }
}

/// Project with a 10 s video (with sound), a still image and a 20 s song.
fn fixture() -> Project {
    let mut p = Project::new("t", Canvas::default());
    p.media = vec![
        media("clip.mp4", MediaKind::Video, 10, true),
        media("photo.jpg", MediaKind::Image, 0, false),
        media("song.mp3", MediaKind::Audio, 20, true),
    ];
    p.tracks.push(Track::new_main());
    p
}

fn main_id(p: &Project) -> TrackId {
    p.main_track().unwrap().id
}

fn run(p: &mut Project, h: &mut History, op: TimelineOp) -> Result<(), EditError> {
    let r = h.execute(p, Box::new(TimelineEdit::new(op)));
    check_invariants(&p.tracks).unwrap();
    r
}

fn add(p: &mut Project, h: &mut History, media: usize, placement: Placement, at: Time) -> ItemId {
    let item = Item::new(&p.media[media], Time::ZERO);
    let id = item.id;
    run(
        p,
        h,
        TimelineOp::Insert {
            placement,
            at,
            items: vec![item],
        },
    )
    .unwrap();
    id
}

fn starts(track: &Track) -> Vec<i64> {
    track
        .items
        .iter()
        .map(|i| i.start.ticks() / TICKS_PER_SECOND)
        .collect()
}

#[test]
fn main_track_is_magnetic() {
    let mut p = fixture();
    let mut h = History::new();
    let m = Placement::Track(main_id(&p));
    let a = add(&mut p, &mut h, 0, m, S(0));
    let b = add(&mut p, &mut h, 1, m, S(100)); // dropped far right: packs to 10 s
    assert_eq!(starts(p.main_track().unwrap()), [0, 10]);

    // Inserting at the start pushes everything right.
    let c = add(&mut p, &mut h, 1, m, S(0));
    assert_eq!(starts(p.main_track().unwrap()), [0, 5, 15]);
    assert_eq!(p.main_track().unwrap().items[0].id, c);

    // Deleting closes the gap.
    run(
        &mut p,
        &mut h,
        TimelineOp::Delete {
            items: vec![a],
            ripple: false,
        },
    )
    .unwrap();
    assert_eq!(starts(p.main_track().unwrap()), [0, 5]);
    assert_eq!(p.main_track().unwrap().items[1].id, b);
}

#[test]
fn free_track_overlap_moves_to_new_track() {
    let mut p = fixture();
    let mut h = History::new();
    let over = Placement::NewTrack {
        kind: TrackKind::Video,
        index: 1,
    };
    let a = add(&mut p, &mut h, 1, over, S(2));
    let top = p.tracks[1].id;
    add(&mut p, &mut h, 1, Placement::Track(top), S(4)); // overlaps 2..7
    assert_eq!(p.tracks.len(), 3, "overlap bumped to a new track");
    assert_eq!(p.tracks[1].items[0].id, a);
    assert_eq!(starts(&p.tracks[2]), [4]);
}

#[test]
fn incompatible_track_is_rejected() {
    let mut p = fixture();
    let mut h = History::new();
    let item = Item::new(&p.media[2], Time::ZERO);
    let before = p.clone();
    let main = main_id(&p);
    assert!(
        run(
            &mut p,
            &mut h,
            TimelineOp::Insert {
                placement: Placement::Track(main),
                at: S(0),
                items: vec![item]
            }
        )
        .is_err()
    );
    assert_eq!(p, before);
}

#[test]
fn split_trim_and_slip() {
    let mut p = fixture();
    let mut h = History::new();
    let m = Placement::Track(main_id(&p));
    let a = add(&mut p, &mut h, 0, m, S(0));
    run(
        &mut p,
        &mut h,
        TimelineOp::Split {
            at: S(4),
            items: vec![],
        },
    )
    .unwrap();
    let main = p.main_track().unwrap();
    assert_eq!(main.items.len(), 2);
    assert_eq!(main.items[0].id, a);
    assert_eq!(main.items[0].duration, S(4));
    assert_eq!(main.items[1].source_in, S(4));
    assert_eq!(main.items[1].duration, S(6));
    let right = main.items[1].id;

    // Trim the right half's start later: on the main track it stays packed.
    run(
        &mut p,
        &mut h,
        TimelineOp::Trim {
            item: right,
            edge: Edge::Start,
            to: S(5),
            ripple: false,
        },
    )
    .unwrap();
    let r = &p.main_track().unwrap().items[1];
    assert_eq!((r.start, r.source_in, r.duration), (S(4), S(5), S(5)));

    // Extending past the source end is clamped.
    run(
        &mut p,
        &mut h,
        TimelineOp::Trim {
            item: right,
            edge: Edge::End,
            to: S(100),
            ripple: false,
        },
    )
    .unwrap();
    assert_eq!(p.main_track().unwrap().items[1].duration, S(5));

    // Slip within the source.
    run(
        &mut p,
        &mut h,
        TimelineOp::Trim {
            item: a,
            edge: Edge::End,
            to: S(2),
            ripple: false,
        },
    )
    .unwrap();
    run(
        &mut p,
        &mut h,
        TimelineOp::Slip {
            item: a,
            source_in: S(3),
        },
    )
    .unwrap();
    assert_eq!(p.main_track().unwrap().items[0].source_in, S(3));
    run(
        &mut p,
        &mut h,
        TimelineOp::Slip {
            item: a,
            source_in: S(50),
        },
    )
    .unwrap();
    assert_eq!(
        p.main_track().unwrap().items[0].source_in,
        S(8),
        "clamped to 10 s - 2 s"
    );
}

#[test]
fn image_duration_is_free() {
    let mut p = fixture();
    let mut h = History::new();
    let m = Placement::Track(main_id(&p));
    let img = add(&mut p, &mut h, 1, m, S(0));
    assert_eq!(p.main_track().unwrap().items[0].duration, IMAGE_DURATION);
    run(
        &mut p,
        &mut h,
        TimelineOp::SetDuration {
            items: vec![img],
            duration: S(12),
        },
    )
    .unwrap();
    assert_eq!(p.main_track().unwrap().items[0].duration, S(12));
    run(
        &mut p,
        &mut h,
        TimelineOp::Trim {
            item: img,
            edge: Edge::End,
            to: S(60),
            ripple: false,
        },
    )
    .unwrap();
    assert_eq!(p.main_track().unwrap().items[0].duration, S(60));
}

#[test]
fn ripple_delete_and_close_gap_on_free_track() {
    let mut p = fixture();
    let mut h = History::new();
    let a = add(
        &mut p,
        &mut h,
        2,
        Placement::NewTrack {
            kind: TrackKind::Audio,
            index: 1,
        },
        S(0),
    );
    let audio = p.tracks[1].id;
    let mut b = Item::new(&p.media[2], Time::ZERO);
    b.duration = S(5);
    let mut c = b.clone();
    c.id = ItemId::new();
    c.start = S(10);
    let (bid, cid) = (b.id, c.id);
    run(
        &mut p,
        &mut h,
        TimelineOp::Insert {
            placement: Placement::Track(audio),
            at: S(25),
            items: vec![b, c],
        },
    )
    .unwrap();
    assert_eq!(starts(&p.tracks[1]), [0, 25, 35]);

    run(
        &mut p,
        &mut h,
        TimelineOp::CloseGap {
            track: audio,
            at: S(22),
        },
    )
    .unwrap();
    assert_eq!(starts(&p.tracks[1]), [0, 20, 30]);

    run(
        &mut p,
        &mut h,
        TimelineOp::Delete {
            items: vec![bid],
            ripple: true,
        },
    )
    .unwrap();
    assert_eq!(starts(&p.tracks[1]), [0, 25]);
    assert_eq!(p.tracks[1].items[1].id, cid);

    run(
        &mut p,
        &mut h,
        TimelineOp::Delete {
            items: vec![a, cid],
            ripple: false,
        },
    )
    .unwrap();
    assert_eq!(p.tracks.len(), 1, "empty free track removed, main kept");
}

#[test]
fn groups_move_and_delete_together() {
    let mut p = fixture();
    let mut h = History::new();
    let a = add(
        &mut p,
        &mut h,
        1,
        Placement::NewTrack {
            kind: TrackKind::Video,
            index: 1,
        },
        S(1),
    );
    let b = add(
        &mut p,
        &mut h,
        2,
        Placement::NewTrack {
            kind: TrackKind::Audio,
            index: 2,
        },
        S(1),
    );
    run(&mut p, &mut h, TimelineOp::Group { items: vec![a, b] }).unwrap();
    run(
        &mut p,
        &mut h,
        TimelineOp::Move {
            items: vec![a],
            delta: S(3),
            to: None,
        },
    )
    .unwrap();
    assert_eq!(p.find_item(b).unwrap().1.start, S(4));
    run(
        &mut p,
        &mut h,
        TimelineOp::Delete {
            items: vec![b],
            ripple: false,
        },
    )
    .unwrap();
    assert!(p.find_item(a).is_none());
}

#[test]
fn detach_audio_links_items() {
    let mut p = fixture();
    let mut h = History::new();
    let mp = Placement::Track(main_id(&p));
    let v = add(&mut p, &mut h, 0, mp, S(0));
    run(&mut p, &mut h, TimelineOp::DetachAudio { item: v }).unwrap();
    let (_, video) = p.find_item(v).unwrap();
    assert!(video.audio_detached);
    let audio_track = p
        .tracks
        .iter()
        .find(|t| t.kind == TrackKind::Audio)
        .unwrap();
    let audio = &audio_track.items[0];
    assert_eq!(audio.group, video.group);
    assert_eq!((audio.start, audio.duration), (video.start, video.duration));
    assert!(
        run(&mut p, &mut h, TimelineOp::DetachAudio { item: v }).is_err(),
        "only once"
    );
}

#[test]
fn locked_tracks_are_untouched() {
    let mut p = fixture();
    let mut h = History::new();
    let mp = Placement::Track(main_id(&p));
    let a = add(&mut p, &mut h, 0, mp, S(0));
    let main = main_id(&p);
    run(
        &mut p,
        &mut h,
        TimelineOp::SetTrack {
            track: main,
            change: TrackChange::Locked(true),
        },
    )
    .unwrap();
    let before = p.clone();
    assert!(
        run(
            &mut p,
            &mut h,
            TimelineOp::Delete {
                items: vec![a],
                ripple: false
            }
        )
        .is_err()
    );
    assert!(
        run(
            &mut p,
            &mut h,
            TimelineOp::Split {
                at: S(5),
                items: vec![]
            }
        )
        .is_err()
    );
    assert_eq!(p, before);
}

#[test]
fn move_between_tracks() {
    let mut p = fixture();
    let mut h = History::new();
    let mp = Placement::Track(main_id(&p));
    let a = add(&mut p, &mut h, 1, mp, S(0));
    run(
        &mut p,
        &mut h,
        TimelineOp::Move {
            items: vec![a],
            delta: S(3),
            to: Some(Placement::NewTrack {
                kind: TrackKind::Video,
                index: 1,
            }),
        },
    )
    .unwrap();
    assert!(p.main_track().unwrap().items.is_empty());
    assert_eq!(starts(&p.tracks[1]), [3]);
    // And back onto the main track: packs to zero.
    let main = main_id(&p);
    run(
        &mut p,
        &mut h,
        TimelineOp::Move {
            items: vec![a],
            delta: Time::ZERO,
            to: Some(Placement::Track(main)),
        },
    )
    .unwrap();
    assert_eq!(starts(p.main_track().unwrap()), [0]);
    assert_eq!(p.tracks.len(), 1);
}

#[test]
fn gain_and_mute_are_clamped() {
    let mut p = fixture();
    let mut h = History::new();
    let mp = Placement::Track(main_id(&p));
    let a = add(&mut p, &mut h, 0, mp, S(0));
    run(
        &mut p,
        &mut h,
        TimelineOp::SetGain {
            items: vec![a],
            gain: 99.0,
        },
    )
    .unwrap();
    assert_eq!(p.find_item(a).unwrap().1.gain, MAX_GAIN);
    run(
        &mut p,
        &mut h,
        TimelineOp::SetMuted {
            items: vec![a],
            muted: true,
        },
    )
    .unwrap();
    assert!(p.find_item(a).unwrap().1.muted);
}

// ---- Property test over random edit sequences ----

#[derive(Debug, Clone)]
enum Gen {
    Insert {
        media: usize,
        track: usize,
        new_track: bool,
        at: i64,
    },
    Move {
        item: usize,
        delta: i64,
        track: Option<usize>,
    },
    Trim {
        item: usize,
        start: bool,
        to: i64,
        ripple: bool,
    },
    Slip {
        item: usize,
        to: i64,
    },
    Split {
        at: i64,
        item: Option<usize>,
    },
    Delete {
        item: usize,
        ripple: bool,
    },
    CloseGap {
        track: usize,
        at: i64,
    },
    Group {
        a: usize,
        b: usize,
    },
    Ungroup {
        item: usize,
    },
    Detach {
        item: usize,
    },
    Duration {
        item: usize,
        secs: i64,
    },
    Lock {
        track: usize,
        on: bool,
    },
    Look {
        item: usize,
        fade_in: i64,
        fade_out: i64,
        scale: u8,
        opacity: u8,
    },
    Fill {
        item: usize,
        shade: u8,
    },
    Speed {
        item: usize,
        speed: u8,
    },
    Reverse {
        item: usize,
        on: bool,
    },
    Freeze {
        item: usize,
        at: i64,
        secs: i64,
    },
    Transition {
        item: usize,
        kind: usize,
        millis: i64,
    },
    RemoveTransition {
        n: usize,
    },
    Text {
        item: usize,
        size: u8,
        enter_ms: i64,
    },
    Undo,
    Redo,
}

fn any_edit() -> impl Strategy<Value = Gen> {
    let i = 0usize..12;
    let t = -2i64..40;
    prop_oneof![
        3 => (0usize..7, 0usize..6, any::<bool>(), t.clone()).prop_map(|(media, track, new_track, at)| Gen::Insert { media, track, new_track, at }),
        2 => (i.clone(), -8i64..8, proptest::option::of(0usize..6)).prop_map(|(item, delta, track)| Gen::Move { item, delta, track }),
        2 => (i.clone(), any::<bool>(), t.clone(), any::<bool>()).prop_map(|(item, start, to, ripple)| Gen::Trim { item, start, to, ripple }),
        1 => (i.clone(), 0i64..12).prop_map(|(item, to)| Gen::Slip { item, to }),
        2 => (t.clone(), proptest::option::of(i.clone())).prop_map(|(at, item)| Gen::Split { at, item }),
        2 => (i.clone(), any::<bool>()).prop_map(|(item, ripple)| Gen::Delete { item, ripple }),
        1 => (0usize..6, t.clone()).prop_map(|(track, at)| Gen::CloseGap { track, at }),
        1 => (i.clone(), i.clone()).prop_map(|(a, b)| Gen::Group { a, b }),
        1 => i.clone().prop_map(|item| Gen::Ungroup { item }),
        1 => i.clone().prop_map(|item| Gen::Detach { item }),
        1 => (i.clone(), 1i64..10).prop_map(|(item, secs)| Gen::Duration { item, secs }),
        1 => (0usize..6, any::<bool>()).prop_map(|(track, on)| Gen::Lock { track, on }),
        2 => (i.clone(), 0i64..8, 0i64..8, any::<u8>(), any::<u8>())
            .prop_map(|(item, fade_in, fade_out, scale, opacity)| Gen::Look { item, fade_in, fade_out, scale, opacity }),
        1 => (i.clone(), any::<u8>()).prop_map(|(item, shade)| Gen::Fill { item, shade }),
        2 => (i.clone(), any::<u8>()).prop_map(|(item, speed)| Gen::Speed { item, speed }),
        1 => (i.clone(), any::<bool>()).prop_map(|(item, on)| Gen::Reverse { item, on }),
        1 => (i.clone(), t.clone(), 0i64..4).prop_map(|(item, at, secs)| Gen::Freeze { item, at, secs }),
        2 => (i.clone(), 0usize..20, 0i64..5000).prop_map(|(item, kind, millis)| Gen::Transition { item, kind, millis }),
        1 => (0usize..4).prop_map(|n| Gen::RemoveTransition { n }),
        1 => (i.clone(), any::<u8>(), 0i64..20_000).prop_map(|(item, size, enter_ms)| Gen::Text { item, size, enter_ms }),
        2 => Just(Gen::Undo),
        1 => Just(Gen::Redo),
    ]
}

fn op_for(g: &Gen, p: &Project) -> Option<TimelineOp> {
    let items: Vec<ItemId> = p.items().map(|i| i.id).collect();
    let item = |n: usize| items.get(n % items.len().max(1)).copied();
    let track = |n: usize| p.tracks.get(n % p.tracks.len().max(1)).map(|t| t.id);
    Some(match g {
        Gen::Insert {
            media,
            track: tn,
            new_track,
            at,
        } => {
            // Index 3 is a generated background clip, 4 a text item, 5 a
            // shape and 6 a visualiser.
            let (kind, item) = match (p.media.get(*media), *media) {
                (Some(m), _) => (track_kind_for(m.kind()), Item::new(m, Time::ZERO)),
                (None, 3) => (
                    TrackKind::Video,
                    Item::generated(Generated::Solid { color: Rgba::BLACK }, Time::ZERO),
                ),
                (None, 5) => (
                    TrackKind::Video,
                    Item::shape_item(ShapeSpec::new(ShapeKind::Arrow), Time::ZERO),
                ),
                (None, 6) => (
                    TrackKind::Overlay,
                    Item::visualiser_item(VisualiserSpec::new(VisualiserStyle::Bars), Time::ZERO),
                ),
                (None, _) => (
                    TrackKind::Text,
                    Item::text_item(TextSpec::new("Title"), Time::ZERO),
                ),
            };
            let placement = if *new_track {
                Placement::NewTrack {
                    kind,
                    index: tn % (p.tracks.len() + 1),
                }
            } else {
                Placement::Track(track(*tn)?)
            };
            TimelineOp::Insert {
                placement,
                at: S(*at),
                items: vec![item],
            }
        }
        Gen::Move {
            item: n,
            delta,
            track: tn,
        } => TimelineOp::Move {
            items: vec![item(*n)?],
            delta: S(*delta),
            to: tn.and_then(track).map(Placement::Track),
        },
        Gen::Trim {
            item: n,
            start,
            to,
            ripple,
        } => TimelineOp::Trim {
            item: item(*n)?,
            edge: if *start { Edge::Start } else { Edge::End },
            to: S(*to),
            ripple: *ripple,
        },
        Gen::Slip { item: n, to } => TimelineOp::Slip {
            item: item(*n)?,
            source_in: S(*to),
        },
        Gen::Split { at, item: n } => TimelineOp::Split {
            at: S(*at),
            items: n.and_then(item).into_iter().collect(),
        },
        Gen::Delete { item: n, ripple } => TimelineOp::Delete {
            items: vec![item(*n)?],
            ripple: *ripple,
        },
        Gen::CloseGap { track: tn, at } => TimelineOp::CloseGap {
            track: track(*tn)?,
            at: S(*at),
        },
        Gen::Group { a, b } => TimelineOp::Group {
            items: vec![item(*a)?, item(*b)?],
        },
        Gen::Ungroup { item: n } => TimelineOp::Ungroup {
            items: vec![item(*n)?],
        },
        Gen::Detach { item: n } => TimelineOp::DetachAudio { item: item(*n)? },
        Gen::Duration { item: n, secs } => TimelineOp::SetDuration {
            items: vec![item(*n)?],
            duration: S(*secs),
        },
        Gen::Lock { track: tn, on } => TimelineOp::SetTrack {
            track: track(*tn)?,
            change: TrackChange::Locked(*on),
        },
        Gen::Look {
            item: n,
            fade_in,
            fade_out,
            scale,
            opacity,
        } => TimelineOp::Look {
            items: vec![item(*n)?],
            patch: LookPatch {
                transform: Some(Transform {
                    size: [f32::from(*scale) / 64.0; 2],
                    rotation: f32::from(*scale) * 3.0,
                    ..Transform::default()
                }),
                opacity: Some(f32::from(*opacity) / 200.0),
                fade_in: Some(S(*fade_in)),
                fade_out: Some(S(*fade_out)),
                ..LookPatch::default()
            },
        },
        Gen::Fill { item: n, shade } => TimelineOp::SetFill {
            items: vec![item(*n)?],
            fill: Generated::Solid {
                color: Rgba([*shade, *shade, *shade, 255]),
            },
        },
        Gen::Speed { item: n, speed } => TimelineOp::SetSpeed {
            items: vec![item(*n)?],
            // 0.0 to 17.0: below and above the allowed range too.
            speed: f32::from(*speed) / 15.0,
        },
        Gen::Reverse { item: n, on } => TimelineOp::SetReverse {
            items: vec![item(*n)?],
            reverse: *on,
        },
        Gen::Freeze { item: n, at, secs } => TimelineOp::Freeze {
            item: item(*n)?,
            at: S(*at),
            duration: S(*secs),
        },
        Gen::Transition {
            item: n,
            kind,
            millis,
        } => TimelineOp::AddTransition {
            a: item(*n)?,
            kind: TransitionKind::ALL[*kind],
            duration: Time::from_millis(*millis),
        },
        Gen::RemoveTransition { n } => {
            let all: Vec<TransitionId> = p
                .tracks
                .iter()
                .flat_map(|t| &t.transitions)
                .map(|t| t.id)
                .collect();
            TimelineOp::RemoveTransition {
                id: *all.get(n % all.len().max(1))?,
            }
        }
        Gen::Text {
            item: n,
            size,
            enter_ms,
        } => {
            let mut spec = TextSpec::new("Edited");
            spec.style.size = f32::from(*size) / 200.0;
            spec.enter.duration = Time::from_millis(*enter_ms);
            TimelineOp::SetText {
                item: item(*n)?,
                text: spec,
            }
        }
        Gen::Undo | Gen::Redo => return None,
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 400, ..ProptestConfig::default() })]

    /// Random edits always leave a valid timeline; every command reverts
    /// exactly, and redo reproduces the same project (same ids included).
    #[test]
    fn random_edits_keep_invariants_and_undo_exactly(gens in proptest::collection::vec(any_edit(), 1..60)) {
        let start = fixture();
        let mut p = start.clone();
        let mut h = History::new();
        // Each edit must revert exactly on its own.
        h.set_coalescing(false);
        let mut states = vec![p.clone()];
        for g in &gens {
            match g {
                Gen::Undo => {
                    if h.undo(&mut p).is_some() {
                        states.pop();
                        prop_assert_eq!(&p, states.last().unwrap());
                    }
                }
                Gen::Redo => {
                    if h.redo(&mut p).is_some() {
                        states.push(p.clone());
                    }
                }
                other => {
                    if let Some(op) = op_for(other, &p) {
                        let before = p.clone();
                        match h.execute(&mut p, Box::new(TimelineEdit::new(op))) {
                            Ok(()) => {
                                // Undo then redo must land on the same state.
                                let after = p.clone();
                                h.undo(&mut p);
                                prop_assert_eq!(&p, &before);
                                h.redo(&mut p);
                                prop_assert_eq!(&p, &after);
                                states.push(p.clone());
                            }
                            Err(_) => prop_assert_eq!(&p, &before),
                        }
                    }
                }
            }
            for track in &p.tracks {
                for item in &track.items {
                    prop_assert_eq!(item.text().is_some(), track.kind == TrackKind::Text, "text only on text tracks");
                }
            }
            if let Err(e) = check_invariants(&p.tracks) {
                return Err(TestCaseError::fail(format!("{e} after {g:?}\n{:#?}", p.tracks)));
            }
            for item in p.items() {
                prop_assert!(item.look.fade_in + item.look.fade_out <= item.duration, "fades overflow: {item:?}");
                prop_assert!((0.0..=1.0).contains(&item.look.opacity));
                prop_assert!((MIN_SPEED..=MAX_SPEED).contains(&item.speed));
                if let Some(t) = item.text() {
                    prop_assert!(t.enter.duration + t.exit.duration <= item.duration, "text motion overflows: {item:?}");
                    prop_assert!((MIN_TEXT_SIZE..=MAX_TEXT_SIZE).contains(&t.style.size));
                }
                let Some(m) = item.media().and_then(|id| p.media(id)) else { continue };
                if let Some(max) = max_duration(m, Time::ZERO) {
                    if let Some(at) = item.held_frame() {
                        prop_assert!(at >= Time::ZERO && at < max, "held frame outside its source: {item:?}");
                        continue;
                    }
                    // Allow rounding of a couple of ticks (a few nanoseconds).
                    prop_assert!(item.source_in + item.source_span() <= max + Time(2), "item runs past its source: {item:?}");
                    for t in [item.start, item.end() - Time(1)] {
                        let s = item.source_time(t);
                        prop_assert!(s >= item.source_in && s <= max + Time(2), "source time {s:?} outside source: {item:?}");
                    }
                }
            }
        }
        while h.undo(&mut p).is_some() {}
        prop_assert_eq!(&p, &start);
    }
}

#[test]
fn split_keeps_fades_on_outer_pieces_and_backgrounds_resize() {
    let mut p = fixture();
    let mut h = History::new();
    let mp = Placement::Track(main_id(&p));
    let bg = Item::generated(Generated::Solid { color: Rgba::BLACK }, Time::ZERO);
    let id = bg.id;
    run(
        &mut p,
        &mut h,
        TimelineOp::Insert {
            placement: mp,
            at: S(0),
            items: vec![bg],
        },
    )
    .unwrap();
    run(
        &mut p,
        &mut h,
        TimelineOp::SetDuration {
            items: vec![id],
            duration: S(20),
        },
    )
    .unwrap();
    run(
        &mut p,
        &mut h,
        TimelineOp::Look {
            items: vec![id],
            patch: LookPatch {
                fade_in: Some(S(2)),
                fade_out: Some(S(3)),
                ..LookPatch::default()
            },
        },
    )
    .unwrap();
    run(
        &mut p,
        &mut h,
        TimelineOp::Split {
            at: S(10),
            items: vec![],
        },
    )
    .unwrap();
    let main = p.main_track().unwrap();
    assert_eq!(
        (main.items[0].look.fade_in, main.items[0].look.fade_out),
        (S(2), Time::ZERO)
    );
    assert_eq!(
        (main.items[1].look.fade_in, main.items[1].look.fade_out),
        (Time::ZERO, S(3))
    );
    // Trimming shorter than the fade shrinks the fade.
    let right = main.items[1].id;
    run(
        &mut p,
        &mut h,
        TimelineOp::Trim {
            item: right,
            edge: Edge::End,
            to: S(11),
            ripple: false,
        },
    )
    .unwrap();
    assert_eq!(p.find_item(right).unwrap().1.look.fade_out, S(1));
    // Recolour.
    run(
        &mut p,
        &mut h,
        TimelineOp::SetFill {
            items: vec![right],
            fill: Generated::Solid {
                color: Rgba([255, 0, 0, 255]),
            },
        },
    )
    .unwrap();
    assert!(matches!(
        p.find_item(right).unwrap().1.generated_fill(),
        Some(Generated::Solid {
            color: Rgba([255, 0, 0, 255])
        })
    ));
    // Backgrounds can't go on audio tracks.
    let bg2 = Item::generated(Generated::Solid { color: Rgba::BLACK }, Time::ZERO);
    assert!(
        run(
            &mut p,
            &mut h,
            TimelineOp::Insert {
                placement: Placement::NewTrack {
                    kind: TrackKind::Audio,
                    index: 9
                },
                at: S(0),
                items: vec![bg2]
            }
        )
        .is_err()
    );
}
