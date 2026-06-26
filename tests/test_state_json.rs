use anvil::state::{AppState, DesiredState};
use std::path::Path;
use tempfile::tempdir;

#[test]
fn test_state_atomic_write_and_read() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("state.json");
    let mut state = AppState::default();
    state.set_desired("lobby", DesiredState::Running);
    state.save(&path).unwrap();

    let loaded = AppState::load(&path).unwrap();
    assert_eq!(loaded.get_desired("lobby"), Some(&DesiredState::Running));
    assert_eq!(loaded.schema_version, 1);
}

#[test]
fn test_corrupt_state_returns_empty() {
    let corrupt = Path::new("tests/fixtures/corrupt_state.json");
    let state = AppState::load(corrupt).unwrap();
    assert!(
        state.servers.is_empty(),
        "Corrupt state should yield empty servers"
    );
}

#[test]
fn test_stop_changes_desired() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("state.json");
    let mut state = AppState::default();
    state.set_desired("survival", DesiredState::Running);
    state.save(&path).unwrap();

    let mut state2 = AppState::load(&path).unwrap();
    state2.set_desired("survival", DesiredState::Stopped);
    state2.save(&path).unwrap();

    let state3 = AppState::load(&path).unwrap();
    assert_eq!(state3.get_desired("survival"), Some(&DesiredState::Stopped));
}

#[test]
fn test_missing_state_returns_empty() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("nonexistent_state.json");
    let state = AppState::load(&path).unwrap();
    assert!(state.servers.is_empty());
}
