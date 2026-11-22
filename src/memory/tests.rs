use super::*;

#[test]
fn slugify_normalises_names() {
    assert_eq!(slugify("Auth Decision!"), "auth-decision");
    assert_eq!(slugify("  spaces  "), "spaces");
    assert_eq!(slugify("__keep_underscore__"), "__keep_underscore__");
    assert_eq!(slugify("!!!"), "");
}

#[test]
fn write_load_and_index_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = MemoryStore::load_from(dir.path());
    store
        .write(
            "auth-decision",
            "Auth uses signed cookies",
            "Why sessions are stateless",
            MemoryKind::Project,
            "The session cookie is signed.",
        )
        .unwrap();

    let loaded = MemoryStore::load_from(dir.path());
    assert_eq!(loaded.len(), 1);
    let entry = loaded.get("auth-decision").unwrap();
    assert_eq!(entry.title, "Auth uses signed cookies");
    assert_eq!(entry.kind, MemoryKind::Project);
    assert!(entry.body.contains("signed"));

    let index = std::fs::read_to_string(dir.path().join(INDEX_FILE)).unwrap();
    assert!(index.contains("[Auth uses signed cookies](auth-decision.md)"));
    assert!(index.contains("(project)"));
}

#[test]
fn write_replaces_existing_entry() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = MemoryStore::load_from(dir.path());
    store
        .write("note", "One", "first", MemoryKind::Project, "first body")
        .unwrap();
    store
        .write("note", "Two", "second", MemoryKind::Feedback, "second body")
        .unwrap();

    let loaded = MemoryStore::load_from(dir.path());
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded.get("note").unwrap().body, "second body");
    assert_eq!(loaded.get("note").unwrap().kind, MemoryKind::Feedback);
}

#[test]
fn delete_removes_file_and_index_line() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = MemoryStore::load_from(dir.path());
    store
        .write("gone", "Gone", "delete me", MemoryKind::Reference, "body")
        .unwrap();
    assert!(store.delete("gone").unwrap());
    assert!(!store.delete("gone").unwrap());

    let loaded = MemoryStore::load_from(dir.path());
    assert!(loaded.is_empty());
    assert!(!dir.path().join("gone.md").exists());
    let index = std::fs::read_to_string(dir.path().join(INDEX_FILE)).unwrap();
    assert!(!index.contains("gone.md"));
}

#[test]
fn prompt_block_is_absent_without_entries() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::load_from(dir.path());
    assert!(store.prompt_block().is_none());
}

#[test]
fn prompt_block_lists_entries() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = MemoryStore::load_from(dir.path());
    store
        .write(
            "build",
            "Build needs nightly",
            "Toolchain note",
            MemoryKind::Project,
            "body",
        )
        .unwrap();
    let block = store.prompt_block().unwrap();
    assert!(block.contains("<memory>"));
    assert!(block.contains("Build needs nightly"));
    assert!(block.contains("`memory` tool"));
}

#[test]
fn files_without_frontmatter_still_load() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("plain.md"), "# Plain\nbody text").unwrap();
    let store = MemoryStore::load_from(dir.path());
    assert_eq!(store.len(), 1);
    let entry = store.get("plain").unwrap();
    assert_eq!(entry.kind, MemoryKind::Project);
    assert!(entry.body.contains("body text"));
}
