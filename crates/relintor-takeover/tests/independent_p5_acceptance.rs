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

#[test]
fn test_jetpack_compose_and_kotlin_ui_surfaces_detected() {
    let fixture_root = root("compose-ui");
    write_files(
        &fixture_root,
        &[
            (
                "apps/android/src/main/java/org/example/MainActivity.kt".into(),
                r#"
                package org.example
                import androidx.activity.ComponentActivity
                import androidx.compose.material3.*

                class MainActivity : ComponentActivity() {
                    override fun onCreate(savedInstanceState: Bundle?) {
                        super.onCreate(savedInstanceState)
                        setContent {
                            MaterialTheme {
                                SettingsScreen()
                            }
                        }
                    }
                }
                "#.into(),
            ),
            (
                "apps/android/src/main/java/org/example/SettingsScreen.kt".into(),
                r#"
                package org.example
                import androidx.compose.runtime.Composable
                import androidx.compose.material3.*

                @Composable
                fun SettingsScreen() {
                    Button(onClick = { /* save */ }) {
                        Text("Save")
                    }
                    OutlinedTextField(value = "", onValueChange = {})
                }
                "#.into(),
            ),
        ],
    );
    let report = TakeoverScanner::default().scan(&fixture_root).unwrap();
    assert!(!report.ui_surfaces.is_empty(), "Compose UI surfaces must be discovered");
    assert!(report.ui_surfaces.iter().any(|s| s.path.contains("SettingsScreen.kt")));
    assert!(report.ui_surfaces.iter().any(|s| s.path.contains("MainActivity.kt")));
    let _ = fs::remove_dir_all(fixture_root);
}

#[test]
fn test_log_error_string_resembling_path_is_not_a_route() {
    let fixture_root = root("false-route");
    write_files(
        &fixture_root,
        &[(
            "crates/routing/src/lib.rs".into(),
            r#"
            pub fn handle_error() {
                eprintln!("UI CiphrChat cannot read peer key path: {error}");
                let path: String = "local/path".to_string();
            }
            "#.into(),
        )],
    );
    let report = TakeoverScanner::default().scan(&fixture_root).unwrap();
    assert!(!report.routes.iter().any(|r| r.path.contains("cannot read peer key path")));
    assert!(!report.findings.iter().any(|f| f.summary.contains("cannot read peer key path")));
    let _ = fs::remove_dir_all(fixture_root);
}

#[test]
fn test_diagnostic_route_does_not_emit_hidden_route_finding() {
    let fixture_root = root("diagnostic-route");
    write_files(
        &fixture_root,
        &[(
            "services/relay/src/main.rs".into(),
            r#"
            // Background relay service with health check
            app.get("/health", health_handler);
            app.get("/metrics", metrics_handler);
            "#.into(),
        )],
    );
    let report = TakeoverScanner::default().scan(&fixture_root).unwrap();
    assert!(report.routes.iter().any(|r| r.path == "/health"));
    assert!(!report.findings.iter().any(|f| f.finding_type == "hidden-route" && f.summary.contains("/health")));
    let _ = fs::remove_dir_all(fixture_root);
}

#[test]
fn test_sql_create_table_if_not_exists_extracts_table_name_not_if() {
    let fixture_root = root("schema-if");
    write_files(
        &fixture_root,
        &[
            (
                "db/schema.sql".into(),
                "CREATE TABLE IF NOT EXISTS user_accounts (id TEXT PRIMARY KEY, email TEXT NOT NULL);\n".into(),
            ),
            (
                "src/main.rs".into(),
                "fn check_condition() { if x > 0 { println!(\"positive\"); } }\n".into(),
            ),
        ],
    );
    let report = TakeoverScanner::default().scan(&fixture_root).unwrap();
    assert!(report.schema_objects.iter().any(|s| s.name == "user_accounts"));
    assert!(!report.schema_objects.iter().any(|s| s.name == "IF" || s.name == "if" || s.name == "NOT" || s.name == "EXISTS"));
    let _ = fs::remove_dir_all(fixture_root);
}

#[test]
fn test_omnichat_isolated_scan_reality_report() {
    let omnichat_path = Path::new("D:\\Relintor-Beta-Test\\phase5-serious-omnichat");
    if !omnichat_path.exists() {
        return;
    }
    let report = TakeoverScanner::default().scan(omnichat_path).unwrap();
    println!("OmniChat Scan Results:");
    println!("  Files inspected: {}", report.inventory.len());
    println!("  UI Surfaces: {}", report.ui_surfaces.len());
    println!("  Routes: {}", report.routes.len());
    for r in &report.routes {
        println!("    Route: {} {} at {}", r.method, r.path, r.source.locator);
    }
    println!("  Schema Objects: {}", report.schema_objects.len());
    for s in &report.schema_objects {
        println!("    Schema: {} (type: {})", s.name, s.object_type);
    }
    println!("  Findings: {}", report.findings.len());
    for f in &report.findings {
        println!("    Finding: [{}] {} - {}", f.severity, f.finding_type, f.summary);
    }

    assert!(report.ui_surfaces.len() > 0, "OmniChat must have discovered UI surfaces");
    assert!(!report.routes.iter().any(|r| r.path.contains("cannot read peer key path")));
    assert!(!report.schema_objects.iter().any(|s| s.name == "IF" || s.name == "if"));
    assert!(!report.findings.iter().any(|f| f.finding_type == "hidden-route" && f.summary.contains("/health")));
    assert!(!report.findings.iter().any(|f| f.finding_type == "missing-migration" && f.summary.contains("for: IF")));
}

fn init_test_db(path: &Path) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at INTEGER NOT NULL);"
    ).unwrap();
    conn.execute_batch(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../db/migrations/005_takeover_foundation.sql"
    ))).unwrap();
    conn.execute_batch(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../db/migrations/008_takeover_project_binding.sql"
    ))).unwrap();
}

#[test]
fn test_rescan_evicts_stale_findings_and_maintains_deterministic_authority() {
    let db_dir = root("rescan-authority-db");
    let db_path = db_dir.join("test.sqlite");
    let ws_root = root("rescan-ws");
    
    // 1. Initial workspace with a suspicious comment that might generate a finding
    write_files(
        &ws_root,
        &[(
            "src/main.rs".into(),
            "// Password: hardcoded_secret_12345\nfn main() {}\n".into(),
        )],
    );

    let report_a = TakeoverScanner::default().scan(&ws_root).unwrap();
    let project_id = "test-project-1";
    init_test_db(&db_path);
    relintor_takeover::persist_takeover_for_project(&db_path, &report_a, project_id).unwrap();

    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let count_a: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM takeover_findings WHERE takeover_id = ?1",
            rusqlite::params![report_a.takeover.id],
            |row| row.get(0),
        )
        .unwrap();

    // 2. Fix the file
    write_files(
        &ws_root,
        &[(
            "src/main.rs".into(),
            "fn main() { println!(\"clean\"); }\n".into(),
        )],
    );

    let report_b = TakeoverScanner::default().scan(&ws_root).unwrap();
    assert_ne!(report_a.fingerprint, report_b.fingerprint);
    assert_ne!(report_a.takeover.id, report_b.takeover.id);

    relintor_takeover::persist_takeover_for_project(&db_path, &report_b, project_id).unwrap();

    // 3. Verify that old findings for project_id were purged
    let stale_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM takeover_findings WHERE takeover_id = ?1",
            rusqlite::params![report_a.takeover.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stale_count, 0, "Stale findings from Scan A must be evicted");

    let count_b: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM takeover_findings WHERE takeover_id = ?1",
            rusqlite::params![report_b.takeover.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count_b, report_b.findings.len() as i64);

    let _ = fs::remove_dir_all(db_dir);
    let _ = fs::remove_dir_all(ws_root);
}

#[test]
fn test_multi_workspace_isolation() {
    let db_dir = root("multi-ws-db");
    let db_path = db_dir.join("test.sqlite");
    let ws_a = root("ws-a");
    let ws_b = root("ws-b");

    write_files(&ws_a, &[("src/lib.rs".into(), "pub fn a() {}\n".into())]);
    write_files(&ws_b, &[("src/lib.rs".into(), "pub fn b() {}\n".into())]);

    let rep_a = TakeoverScanner::default().scan(&ws_a).unwrap();
    let rep_b = TakeoverScanner::default().scan(&ws_b).unwrap();

    init_test_db(&db_path);
    relintor_takeover::persist_takeover_for_project(&db_path, &rep_a, "proj-a").unwrap();
    relintor_takeover::persist_takeover_for_project(&db_path, &rep_b, "proj-b").unwrap();

    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let takeovers_a: Vec<String> = conn
        .prepare("SELECT id FROM project_takeovers WHERE project_id = 'proj-a'")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();

    let takeovers_b: Vec<String> = conn
        .prepare("SELECT id FROM project_takeovers WHERE project_id = 'proj-b'")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();

    assert_eq!(takeovers_a.len(), 1);
    assert_eq!(takeovers_b.len(), 1);
    assert_ne!(takeovers_a[0], takeovers_b[0]);

    let _ = fs::remove_dir_all(db_dir);
    let _ = fs::remove_dir_all(ws_a);
    let _ = fs::remove_dir_all(ws_b);
}

#[test]
fn test_all_supported_project_types_discovery() {
    // 1. Rust
    let rust_root = root("proj-rust");
    write_files(&rust_root, &[
        ("Cargo.toml".into(), "[package]\nname = \"rust-demo\"\nversion = \"0.1.0\"\n".into()),
        ("src/main.rs".into(), "fn main() { println!(\"hello\"); }\n".into()),
    ]);
    let rep_rust = TakeoverScanner::default().scan(&rust_root).unwrap();
    assert_eq!(rep_rust.findings.len(), 0);
    let _ = fs::remove_dir_all(rust_root);

    // 2. TypeScript / React
    let ts_root = root("proj-ts");
    write_files(&ts_root, &[
        ("package.json".into(), "{\"name\": \"ts-demo\", \"dependencies\": {\"react\": \"^18.0.0\"}}\n".into()),
        ("src/App.tsx".into(), "import React from 'react'; export function App() { return <div>App</div>; }\n".into()),
    ]);
    let rep_ts = TakeoverScanner::default().scan(&ts_root).unwrap();
    assert!(rep_ts.ui_surfaces.iter().any(|s| s.path.contains("App.tsx")));
    let _ = fs::remove_dir_all(ts_root);

    // 3. Python Backend
    let py_root = root("proj-py");
    write_files(&py_root, &[
        ("requirements.txt".into(), "fastapi==0.100.0\nuvicorn==0.22.0\n".into()),
        ("app/main.py".into(), "from fastapi import FastAPI\napp = FastAPI()\n@app.get('/health')\ndef health(): return {'ok': True}\n".into()),
    ]);
    let rep_py = TakeoverScanner::default().scan(&py_root).unwrap();
    assert_eq!(rep_py.findings.len(), 0);
    let _ = fs::remove_dir_all(py_root);

    // 4. Kotlin / Android
    let kt_root = root("proj-kt");
    write_files(&kt_root, &[
        ("build.gradle.kts".into(), "plugins { id(\"com.android.application\") }\n".into()),
        ("app/src/main/kotlin/Screen.kt".into(), "package app\nimport androidx.compose.runtime.Composable\n@Composable\nfun Screen() {}\n".into()),
    ]);
    let rep_kt = TakeoverScanner::default().scan(&kt_root).unwrap();
    assert!(rep_kt.ui_surfaces.iter().any(|s| s.path.contains("Screen.kt")));
    let _ = fs::remove_dir_all(kt_root);

    // 5. Swift / iOS
    let swift_root = root("proj-swift");
    write_files(&swift_root, &[
        ("Package.swift".into(), "// swift-tools-version:5.7\nimport PackageDescription\nlet package = Package(name: \"SwiftDemo\")\n".into()),
        ("Sources/ContentView.swift".into(), "import SwiftUI\nstruct ContentView: View { var body: some View { Text(\"Hi\") } }\n".into()),
    ]);
    let rep_swift = TakeoverScanner::default().scan(&swift_root).unwrap();
    assert!(rep_swift.ui_surfaces.iter().any(|s| s.path.contains("ContentView.swift")));
    let _ = fs::remove_dir_all(swift_root);

    // 6. Java / Gradle
    let java_root = root("proj-java");
    write_files(&java_root, &[
        ("build.gradle".into(), "plugins { id 'java' }\n".into()),
        ("src/main/java/App.java".into(), "public class App { public static void main(String[] args) {} }\n".into()),
    ]);
    let rep_java = TakeoverScanner::default().scan(&java_root).unwrap();
    assert_eq!(rep_java.findings.len(), 0);
    let _ = fs::remove_dir_all(java_root);
}
