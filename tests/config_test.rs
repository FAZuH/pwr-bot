use lazy_static::lazy_static;
use pwr_bot::config::Config;
use std::env;
use std::sync::Mutex;
use std::time::Duration;

lazy_static! {
    static ref ENV_MUTEX: Mutex<()> = Mutex::new(());
}

#[test]
fn test_config_new_defaults() {
    let _guard = ENV_MUTEX.lock().unwrap();
    unsafe {
        env::remove_var("POLL_INTERVAL");
        env::remove_var("DATABASE_URL");
        env::remove_var("DATABASE_PATH");
        env::set_var("DISCORD_TOKEN", "test_token");
        env::set_var("ADMIN_ID", "12345");
    }

    let config = Config::new();

    assert_eq!(config.poll_interval, Duration::new(60, 0));
    assert_eq!(config.db_url, "sqlite://data.db");
    assert_eq!(config.db_path, "data.db");
    assert_eq!(config.discord_token, "test_token");
    assert_eq!(config.admin_id, "12345");
}

#[test]
fn test_config_new_with_env_vars() {
    let _guard = ENV_MUTEX.lock().unwrap();
    unsafe {
        env::set_var("POLL_INTERVAL", "120");
        env::set_var("DATABASE_URL", "test_db_url");
        env::set_var("DATABASE_PATH", "test_db_path");
        env::set_var("DISCORD_TOKEN", "env_token");
        env::set_var("ADMIN_ID", "54321");
    }

    let config = Config::new();

    assert_eq!(config.poll_interval, Duration::new(120, 0));
    assert_eq!(config.db_url, "test_db_url");
    assert_eq!(config.db_path, "test_db_path");
    assert_eq!(config.discord_token, "env_token");
    assert_eq!(config.admin_id, "54321");
}
