//! M10: a panic leaves a crash report the next start can point to. (Its own
//! test file: the panic hook is process-wide.)

use rw_ui::recovery::{REPORTS, Recovery, install_panic_hook};

#[test]
fn a_panic_writes_a_report_that_the_next_start_finds() {
    let tmp = tempfile::tempdir().unwrap();
    let (_run, _) = Recovery::start(tmp.path());
    install_panic_hook(tmp.path().to_path_buf(), "9.9.9");
    let r = std::thread::Builder::new()
        .name("editor".into())
        .spawn(|| panic!("the preview fell over"))
        .unwrap()
        .join();
    assert!(r.is_err());
    let _ = std::panic::take_hook();

    let reports: Vec<_> = std::fs::read_dir(tmp.path().join(REPORTS))
        .unwrap()
        .flatten()
        .collect();
    assert_eq!(reports.len(), 1);
    let text = std::fs::read_to_string(reports[0].path()).unwrap();
    assert!(text.contains("version: 9.9.9"), "{text}");
    assert!(text.contains("message: the preview fell over"), "{text}");
    assert!(text.contains("thread: editor"), "{text}");

    let (_, unclean) = Recovery::start(tmp.path());
    assert_eq!(unclean.unwrap().report, Some(reports[0].path()));
}
