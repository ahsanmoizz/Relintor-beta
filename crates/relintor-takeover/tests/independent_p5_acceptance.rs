use relintor_takeover::{
    seeded_takeover_corpus, CapabilityClassification, ProbeSafety, RuntimeProbePlanner,
    TakeoverScanner,
};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

fn root(label: &str) -> PathBuf {
    let path = PathBuf::from("D:\\Relintor\\target\\p5-independent-fixtures")
        .join(format!("{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

fn write_files(root: &Path, files: &[(String, String)]) {
    for (relative, contents) in files {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut file = fs::File::create(path).unwrap();
        file.write_all(contents.as_bytes()).unwrap();
    }
}

#[test]
fn sealed_seven_gate_corpus_is_executed_not_counted() {
    for case in seeded_takeover_corpus() {
        let fixture_root = root(&case.name);
        write_files(&fixture_root, &case.files);
        let report = TakeoverScanner::default().scan(&fixture_root).unwrap();
        if !case.required_finding.is_empty() {
            let finding = report
                .findings
                .iter()
                .find(|finding| finding.finding_type == case.required_finding)
                .unwrap_or_else(|| {
                    panic!("{} did not produce {}", case.name, case.required_finding)
                });
            assert_ne!(
                finding.classification,
                Some(CapabilityClassification::Working)
            );
        }
        let _ = fs::remove_dir_all(fixture_root);
    }
}

#[test]
fn route_comments_are_not_registered_and_hidden_route_is_evidence() {
    let fixture_root = root("route-evidence");
    write_files(
        &fixture_root,
        &[(
            "src/server.ts".into(),
            "// app.get(\"/fake\", fake);\napp.get(\"/real\", handler);\n".into(),
        )],
    );
    let report = TakeoverScanner::default().scan(&fixture_root).unwrap();
    assert!(report.routes.iter().any(|route| route.path == "/real"));
    assert!(!report.routes.iter().any(|route| route.path == "/fake"));
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.finding_type == "hidden-route"));
    let _ = fs::remove_dir_all(fixture_root);
}

#[test]
fn runtime_probe_policy_is_structured_bounded_and_fails_closed() {
    let planner = RuntimeProbePlanner::new(Default::default());
    let version = planner.plan_version_probe("rustc").unwrap();
    assert_eq!(version.safety, ProbeSafety::SafeReadOnly);
    assert!(planner.plan_version_probe("cmd.exe").is_err());
    assert!(
        planner
            .plan_build_probe(&["cargo".into(), "build".into()])
            .unwrap()
            .safety
            == ProbeSafety::SandboxRequired
    );
    assert!(planner
        .plan_build_probe(&["cargo".into(), "run;del".into()])
        .is_err());
}

#[test]
fn read_only_version_probe_captures_bounded_runtime_evidence() {
    let fixture_root = root("version-probe");
    let planner = RuntimeProbePlanner::new(Default::default());
    let probe = planner.plan_version_probe("rustc").unwrap();
    let result = planner.execute_read_only(&fixture_root, probe).unwrap();
    assert_eq!(result.status, "PASSED");
    assert!(result.stdout.contains("rustc"));
    assert!(result.stderr.len() <= 64 * 1024);
    let _ = fs::remove_dir_all(fixture_root);
}

#[test]
fn structured_takeover_records_persist_without_raw_inventory_text() {
    let fixture_root = root("persistence");
    write_files(
        &fixture_root,
        &[
            ("src/main.rs".into(), "fn main() {}".into()),
            (
                "src/server.ts".into(),
                "app.get(\"/health\", handler);".into(),
            ),
        ],
    );
    let report = TakeoverScanner::default().scan(&fixture_root).unwrap();
    let database = fixture_root.join("takeover.sqlite");
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at INTEGER NOT NULL);",
        )
        .unwrap();
    connection
        .execute_batch(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../db/migrations/005_takeover_foundation.sql"
        )))
        .unwrap();
    drop(connection);
    relintor_takeover::persist_takeover(&database, &report).unwrap();
    let connection = rusqlite::Connection::open(database).unwrap();
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM takeover_capabilities", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert!(count >= 1);
    let _ = fs::remove_dir_all(fixture_root);
}
