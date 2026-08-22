use relintor_takeover::{
    AuthEvidenceState, CapabilityClassification, ProbeSafety, RuntimeProbe, RuntimeProbePlanner,
    TakeoverConfig, TakeoverScanner,
};
use std::fs;
use std::path::{Path, PathBuf};

fn root(label: &str) -> PathBuf {
    let base = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("p5-independent-source-audit");
    let path = base.join(format!("{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

fn write(root: &Path, relative: &str, contents: impl AsRef<[u8]>) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

#[test]
fn forged_safe_read_only_build_probe_is_rejected() {
    let workspace = root("forged-probe");
    let planner = RuntimeProbePlanner::new(Default::default());

    // This probe deliberately has the caller-controlled enum set to SafeReadOnly.
    // The executor itself must still reject the operation because `cargo build`
    // is not a read-only metadata/version probe.
    let forged = RuntimeProbe {
        id: "forged".into(),
        target: "forged build".into(),
        exact_command: vec!["cargo".into(), "build".into()],
        safety: ProbeSafety::SafeReadOnly,
        status: "PLANNED".into(),
        exit_code: None,
        stdout: String::new(),
        stderr: String::new(),
        started_at: 0,
        ended_at: None,
        timeout_ms: 1_000,
        workspace_fingerprint: String::new(),
        source: vec![],
    };

    assert!(
        planner.execute_read_only(&workspace, forged).is_err(),
        "executor must validate the exact safe operation instead of trusting a caller-controlled safety enum"
    );

    let _ = fs::remove_dir_all(workspace);
}

#[test]
fn same_size_large_file_change_invalidates_repository_fingerprint() {
    let workspace = root("large-fingerprint");

    let config = TakeoverConfig {
        max_files: 100,
        max_total_bytes: 16 * 1024 * 1024,
        max_file_bytes: 4 * 1024 * 1024,
        max_hash_bytes: 1024,
        ..Default::default()
    };
    let scanner = TakeoverScanner::new(config);

    write(&workspace, "src/large.bin.txt", vec![b'a'; 8192]);
    let first = scanner.scan(&workspace).unwrap();

    write(&workspace, "src/large.bin.txt", vec![b'b'; 8192]);
    let second = scanner.scan(&workspace).unwrap();

    assert_ne!(
        first.snapshot.fingerprint.value, second.snapshot.fingerprint.value,
        "same-size content changes must invalidate repository/runtime-evidence fingerprints"
    );

    let _ = fs::remove_dir_all(workspace);
}

#[test]
fn deadline_filename_is_not_deterministic_dead_code() {
    let workspace = root("deadline-not-dead");
    write(
        &workspace,
        "src/deadline.ts",
        "export function calculateDeadline() { return 7; }\n",
    );

    let report = TakeoverScanner::default().scan(&workspace).unwrap();

    assert!(
        !report.findings.iter().any(|finding| {
            finding.finding_type == "dead-code"
                && finding.classification == Some(CapabilityClassification::Dead)
        }),
        "a filename substring is not reachability proof"
    );

    let _ = fs::remove_dir_all(workspace);
}

#[test]
fn comment_only_auth_words_do_not_become_auth_implemented() {
    let workspace = root("auth-comment");
    write(
        &workspace,
        "src/auth.ts",
        "// TODO: add OAuth/OIDC or JWT session authentication later.\nexport const placeholder = true;\n",
    );

    let report = TakeoverScanner::default().scan(&workspace).unwrap();

    assert!(
        report
            .auth_systems
            .iter()
            .all(|system| { !system.states.contains(&AuthEvidenceState::AuthImplemented) }),
        "comment/keyword presence must not be upgraded to AuthImplemented"
    );

    let _ = fs::remove_dir_all(workspace);
}

#[test]
fn internal_workspace_dependency_edges_are_materialized() {
    let workspace = root("workspace-edges");

    write(
        &workspace,
        "packages/a/package.json",
        r#"{
          "name": "@fixture/a",
          "dependencies": {
            "@fixture/b": "workspace:*"
          }
        }"#,
    );
    write(
        &workspace,
        "packages/b/package.json",
        r#"{
          "name": "@fixture/b"
        }"#,
    );

    let report = TakeoverScanner::default().scan(&workspace).unwrap();

    assert!(
        report
            .dependency_graph
            .internal_edges
            .iter()
            .any(|(from, to)| from == "@fixture/a" && to == "@fixture/b"),
        "discovered internal workspace dependencies must produce graph edges"
    );

    let _ = fs::remove_dir_all(workspace);
}

#[test]
fn missing_workspace_reference_is_recorded() {
    let workspace = root("workspace-missing");

    write(
        &workspace,
        "packages/a/package.json",
        r#"{
          "name": "@fixture/a",
          "dependencies": {
            "@fixture/missing": "workspace:*"
          }
        }"#,
    );

    let report = TakeoverScanner::default().scan(&workspace).unwrap();

    assert!(
        report
            .dependency_graph
            .dependencies
            .iter()
            .any(|dependency| {
                dependency.name == "@fixture/missing" && dependency.missing_reference
            }),
        "workspace protocol references with no discovered member must be explicit missing references"
    );

    let _ = fs::remove_dir_all(workspace);
}

#[test]
fn migration_evidence_is_correlated_per_schema_object() {
    let workspace = root("partial-migration");

    write(
        &workspace,
        "schema.sql",
        "CREATE TABLE users\nCREATE TABLE payments\n",
    );
    write(
        &workspace,
        "migrations/001_users.sql",
        "CREATE TABLE users\n",
    );

    let report = TakeoverScanner::default().scan(&workspace).unwrap();

    let payments = report
        .schema_objects
        .iter()
        .find(|object| object.name == "payments")
        .expect("payments schema object");

    assert!(
        payments.migration_required || payments.migration_evidence.is_empty(),
        "an unrelated migration must not satisfy migration evidence for every schema object"
    );

    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.finding_type == "missing-migration"),
        "partially migrated schemas must still surface missing migration evidence"
    );

    let _ = fs::remove_dir_all(workspace);
}
