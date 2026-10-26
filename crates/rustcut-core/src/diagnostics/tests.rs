use std::path::{Path, PathBuf};

use super::*;

fn facts(dir: &Path) -> Facts {
    Facts {
        rustview_enabled: false,
        config_file: dir.join("config.toml"),
        config_writable: true,
        ui_state_file: dir.join("ui-state.toml"),
        ui_state_writable: true,
        database: dir.join("queue.sqlite"),
        folders: vec![
            NamedPath::new("Settings", dir),
            NamedPath::new("Cache", dir.join("missing")),
        ],
        targets: vec![NamedPath::new("Clips", dir.join("clips"))],
        cache: dir.join("missing"),
        rustview_library: dir.join("rustview"),
        ..Facts::default()
    }
}

#[test]
fn problems_come_first_and_the_summary_counts_them() {
    let group = vec![
        Check::new("a", Status::Ok, "fine"),
        Check::new("b", Status::Warning, "careful"),
        Check::new("c", Status::Problem, "broken"),
        Check::new("d", Status::Info, "note"),
    ];
    let (attention, passed) = split(&group);
    assert_eq!(attention.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["c", "b"]);
    assert_eq!(passed.len(), 2);
    assert_eq!(summary(&group), "1 problem, 1 warning");
    assert_eq!(summary(&group[..1]), "All 1 checks passed");
    assert_eq!(summary(&[]), "Not checked yet");
    let text = report_text(
        &Report {
            dependencies: group,
            health: Vec::new(),
        },
        "RustCut 0.0.54",
    );
    assert!(text.starts_with("# RustCut 0.0.54\n\n## Dependencies (1 problem, 1 warning)"));
    assert!(text.contains("- [problem] broken\n"));
}

#[test]
fn files_folders_and_space_are_looked_at_without_writing() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("config.toml"), "version = 4\n").unwrap();
    std::fs::write(dir.path().join("ui-state.toml"), "version = 9\n").unwrap();
    let f = facts(dir.path());
    let files = probes::settings_files(&f);
    assert_eq!(files[0].status, Status::Ok, "{:?}", files[0]);
    assert_eq!(files[1].status, Status::Problem, "a newer format: {:?}", files[1]);
    let folders = probes::folders(&f.folders, &f.targets);
    assert_eq!(folders[0].id, "folders");
    assert!(
        folders
            .iter()
            .any(|c| c.id == "folder-Cache" && c.status == Status::Info)
    );
    assert!(
        folders
            .iter()
            .any(|c| c.id == "target-Clips" && c.status == Status::Warning)
    );
    assert_eq!(writable(dir.path()), Some(true));
    assert_eq!(writable(&dir.path().join("missing")), None);
    let spaces = probes::spaces(&f.targets, &f.cache);
    assert_eq!(spaces.len(), 1, "one file system: {spaces:?}");
    assert!(free_space(dir.path()).is_some_and(|s| s.total > 0));
    assert!(
        std::fs::read_dir(dir.path()).unwrap().count() == 2,
        "nothing was created"
    );
}

#[test]
fn a_program_tells_its_version_or_why_not() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let missing = rt.block_on(program_version(&PathBuf::from("/nonexistent/ffmpeg"), "-version"));
    assert_eq!(missing, Err("not found".into()));
    let ffmpeg = rt.block_on(program_version(Path::new("ffmpeg"), "-version")).unwrap();
    assert!(
        ffmpeg.starts_with("ffmpeg version") && !ffmpeg.contains("Copyright"),
        "{ffmpeg}"
    );
    let dir = tempfile::tempdir().unwrap();
    // A database file that is no database is reported, the queue in memory is a problem.
    std::fs::write(dir.path().join("queue.sqlite"), b"not sqlite").unwrap();
    let broken = rt.block_on(probes::database(&dir.path().join("queue.sqlite"), false));
    assert!(broken.status >= Status::Warning, "{broken:?}");
    let memory = rt.block_on(probes::database(&dir.path().join("queue.sqlite"), true));
    assert_eq!(memory.status, Status::Problem);
    let report = rt.block_on(run(facts(dir.path())));
    assert!(
        report
            .dependencies
            .iter()
            .any(|c| c.id == "ffmpeg" && c.status == Status::Ok)
    );
    assert!(
        report
            .dependencies
            .iter()
            .any(|c| c.id == "encoders" && c.status == Status::Info),
        "still running"
    );
    assert!(
        report
            .health
            .iter()
            .any(|c| c.id == "rustview" && c.status == Status::Info),
        "switched off"
    );
}
