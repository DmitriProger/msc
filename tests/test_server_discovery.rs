use anvil::config::GlobalConfig;
use anvil::server::{discover_servers, validate_server_name};
use tempfile::tempdir;

fn make_config(root: &str) -> GlobalConfig {
    GlobalConfig {
        servers_root: root.to_string(),
        log_level: "info".to_string(),
        tmux_socket: "anvil_test".to_string(),
        ..Default::default()
    }
}

#[test]
fn test_no_servers_in_empty_dir() {
    let dir = tempdir().unwrap();
    let config = make_config(dir.path().to_str().unwrap());
    let servers = discover_servers(&config);
    assert!(servers.is_empty());
}

#[test]
fn test_directory_without_start_sh_not_discovered() {
    let dir = tempdir().unwrap();
    std::fs::create_dir(dir.path().join("myserver")).unwrap();
    let config = make_config(dir.path().to_str().unwrap());
    let servers = discover_servers(&config);
    assert!(servers.is_empty());
}

#[test]
fn test_directory_with_start_sh_discovered() {
    let dir = tempdir().unwrap();
    let srv = dir.path().join("lobby");
    std::fs::create_dir(&srv).unwrap();
    std::fs::write(srv.join("start.sh"), "#!/bin/bash\n").unwrap();
    let config = make_config(dir.path().to_str().unwrap());
    let servers = discover_servers(&config);
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0].name, "lobby");
}

#[test]
fn test_invalid_name_ignored() {
    let dir = tempdir().unwrap();
    let srv = dir.path().join("My Server");
    std::fs::create_dir(&srv).unwrap();
    std::fs::write(srv.join("start.sh"), "#!/bin/bash\n").unwrap();
    let config = make_config(dir.path().to_str().unwrap());
    let servers = discover_servers(&config);
    assert!(servers.is_empty());
}

#[test]
fn test_valid_server_names() {
    assert!(validate_server_name("lobby").is_ok());
    assert!(validate_server_name("lobby-01").is_ok());
    assert!(validate_server_name("survival_v2").is_ok());
    assert!(validate_server_name("a").is_ok());
}

#[test]
fn test_invalid_server_names() {
    assert!(validate_server_name("My Server").is_err());
    assert!(validate_server_name("lobby!").is_err());
    assert!(validate_server_name("LOBBY").is_err());
    assert!(validate_server_name("").is_err());
}

#[test]
fn test_multiple_servers_sorted_alphabetically() {
    let dir = tempdir().unwrap();
    for name in &["zebra", "alpha", "middle"] {
        let srv = dir.path().join(name);
        std::fs::create_dir(&srv).unwrap();
        std::fs::write(srv.join("start.sh"), "#!/bin/bash\n").unwrap();
    }
    let config = make_config(dir.path().to_str().unwrap());
    let servers = discover_servers(&config);
    assert_eq!(servers.len(), 3);
    assert_eq!(servers[0].name, "alpha");
    assert_eq!(servers[1].name, "middle");
    assert_eq!(servers[2].name, "zebra");
}
