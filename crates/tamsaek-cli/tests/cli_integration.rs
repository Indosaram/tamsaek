use std::process::Command;

fn run_cli(args: &[&str], index_dir: &std::path::Path) -> std::process::Output {
    Command::new("cargo")
        .args(["run", "-p", "tamsaek-cli", "--"])
        .arg("--index-path")
        .arg(index_dir.to_str().unwrap())
        .args(args)
        .output()
        .expect("failed to execute tamsaek CLI")
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

#[test]
fn test_stats_empty_index() {
    let tmp = tempfile::tempdir().unwrap();
    let index_dir = tmp.path().join("test-index");

    let output = run_cli(&["stats"], &index_dir);
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let out = stdout(&output);
    assert!(out.contains("Documents: 0"), "unexpected stats output: {}", out);
}

#[test]
fn test_index_and_search() {
    let tmp = tempfile::tempdir().unwrap();
    let index_dir = tmp.path().join("test-index");

    // Create some test files to index
    let docs_dir = tmp.path().join("docs");
    std::fs::create_dir_all(&docs_dir).unwrap();
    std::fs::write(docs_dir.join("hello.txt"), "Hello world from Rust programming language").unwrap();
    std::fs::write(docs_dir.join("goodbye.txt"), "Goodbye cruel world, farewell to all").unwrap();

    // Index
    let output = run_cli(&["index", docs_dir.to_str().unwrap()], &index_dir);
    assert!(output.status.success(), "index stderr: {}", stderr(&output));
    let out = stdout(&output);
    assert!(out.contains("2 indexed"), "expected 2 indexed: {}", out);

    // Search
    let output = run_cli(&["search", "Rust"], &index_dir);
    assert!(output.status.success(), "search stderr: {}", stderr(&output));
    let out = stdout(&output);
    assert!(out.contains("hello.txt"), "expected hello.txt in results: {}", out);

    // Stats
    let output = run_cli(&["stats"], &index_dir);
    assert!(output.status.success());
    let out = stdout(&output);
    assert!(out.contains("Documents: 2"), "expected 2 documents: {}", out);
}

#[test]
fn test_get_document() {
    let tmp = tempfile::tempdir().unwrap();
    let index_dir = tmp.path().join("test-index");

    let docs_dir = tmp.path().join("docs");
    std::fs::create_dir_all(&docs_dir).unwrap();
    let test_file = docs_dir.join("test.md");
    std::fs::write(&test_file, "# Test Document\n\nThis is a test.").unwrap();

    // Index it first
    let output = run_cli(&["index", docs_dir.to_str().unwrap()], &index_dir);
    assert!(output.status.success());

    // Get document by its path-based ID
    let id = test_file.to_str().unwrap();
    let output = run_cli(&["get-document", id], &index_dir);
    assert!(output.status.success(), "get-document stderr: {}", stderr(&output));
    let out = stdout(&output);
    assert!(out.contains("Document:"), "expected document header: {}", out);
    assert!(out.contains("test.md"), "expected title: {}", out);

    // Test not-found
    let output = run_cli(&["get-document", "nonexistent-id"], &index_dir);
    assert!(output.status.success());
    let out = stdout(&output);
    assert!(out.contains("not found"), "expected not found message: {}", out);
}

#[test]
fn test_remove_document() {
    let tmp = tempfile::tempdir().unwrap();
    let index_dir = tmp.path().join("test-index");

    let docs_dir = tmp.path().join("docs");
    std::fs::create_dir_all(&docs_dir).unwrap();
    let test_file = docs_dir.join("removeme.txt");
    std::fs::write(&test_file, "Remove this file").unwrap();

    // Index
    let output = run_cli(&["index", docs_dir.to_str().unwrap()], &index_dir);
    assert!(output.status.success());

    // Verify it exists
    let output = run_cli(&["stats"], &index_dir);
    let out = stdout(&output);
    assert!(out.contains("Documents: 1"));

    // Remove
    let id = test_file.to_str().unwrap();
    let output = run_cli(&["remove", id], &index_dir);
    assert!(output.status.success(), "remove stderr: {}", stderr(&output));
    let out = stdout(&output);
    assert!(out.contains("Removed document"), "expected removal confirmation: {}", out);
}

#[test]
fn test_clear_index() {
    let tmp = tempfile::tempdir().unwrap();
    let index_dir = tmp.path().join("test-index");

    let docs_dir = tmp.path().join("docs");
    std::fs::create_dir_all(&docs_dir).unwrap();
    std::fs::write(docs_dir.join("a.txt"), "File A content").unwrap();
    std::fs::write(docs_dir.join("b.txt"), "File B content").unwrap();

    // Index
    run_cli(&["index", docs_dir.to_str().unwrap()], &index_dir);

    // Clear with --force
    let output = run_cli(&["clear", "--force"], &index_dir);
    assert!(output.status.success(), "clear stderr: {}", stderr(&output));
    let out = stdout(&output);
    assert!(out.contains("Cleared index"), "expected clear message: {}", out);

    // Verify empty
    let output = run_cli(&["stats"], &index_dir);
    let out = stdout(&output);
    assert!(out.contains("Documents: 0"), "index should be empty: {}", out);
}

#[test]
fn test_search_regex() {
    let tmp = tempfile::tempdir().unwrap();
    let index_dir = tmp.path().join("test-index");

    let docs_dir = tmp.path().join("docs");
    std::fs::create_dir_all(&docs_dir).unwrap();
    std::fs::write(docs_dir.join("code.rs"), "fn main() {\n    println!(\"hello world\");\n}\n").unwrap();

    // Index
    run_cli(&["index", docs_dir.to_str().unwrap()], &index_dir);

    // Tantivy regex works on individual terms, use a simple term-level regex
    let output = run_cli(&["search-regex", "println"], &index_dir);
    assert!(output.status.success(), "regex stderr: {}", stderr(&output));
    let out = stdout(&output);
    assert!(out.contains("code.rs"), "expected code.rs in regex results: {}", out);
}
