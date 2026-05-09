use std::fs;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn confit() -> Command {
    Command::cargo_bin("confit").unwrap()
}

fn setup(toml: &str) -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("confit.toml"), toml).unwrap();
    dir
}

// --- no config ---

#[test]
fn no_config_shows_error() {
    let dir = TempDir::new().unwrap();
    confit()
        .arg("resolve")
        .arg("anything")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("confit.toml not found"));
}

// --- init ---

#[test]
fn init_creates_config() {
    let dir = TempDir::new().unwrap();
    confit()
        .arg("init")
        .current_dir(dir.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("created confit.toml"));
    assert!(dir.path().join("confit.toml").exists());
    let content = fs::read_to_string(dir.path().join("confit.toml")).unwrap();
    assert!(content.contains("[vars]"));
}

#[test]
fn init_refuses_if_exists() {
    let dir = setup("[vars]\nx = \"1\"");
    confit()
        .arg("init")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

// --- version ---

#[test]
fn version_prints_version() {
    let dir = TempDir::new().unwrap();
    confit()
        .arg("version")
        .current_dir(dir.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("confit"));
}

// --- resolve ---

#[test]
fn resolve_plain_value() {
    let dir = setup(
        r#"
[app]
name = "myapp"
port = 3000
"#,
    );
    confit()
        .args(["resolve", "app.name"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout("myapp");
}

#[test]
fn resolve_integer() {
    let dir = setup(
        r#"
[app]
port = 3000
"#,
    );
    confit()
        .args(["resolve", "app.port"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout("3000");
}

#[test]
fn resolve_interpolated() {
    let dir = setup(
        r#"
[vars]
env = "prod"
[app]
name = "myapp-{vars.env}"
"#,
    );
    confit()
        .args(["resolve", "app.name"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout("myapp-prod");
}

#[test]
fn resolve_with_set_override() {
    let dir = setup(
        r#"
[vars]
env = "dev"
[app]
name = "myapp-{vars.env}"
"#,
    );
    confit()
        .args(["--set", "env=staging", "resolve", "app.name"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout("myapp-staging");
}

#[test]
fn resolve_shell_eval() {
    let dir = setup(
        r#"
[build]
hash = "$(echo abc123)"
"#,
    );
    confit()
        .args(["resolve", "build.hash"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout("abc123");
}

#[test]
fn resolve_no_eval_skips_shell() {
    let dir = setup(
        r#"
[build]
hash = "$(echo abc123)"
"#,
    );
    confit()
        .args(["resolve", "build.hash", "--no-eval"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout("$(echo abc123)");
}

#[test]
fn resolve_secret_masked() {
    let dir = setup(
        r#"
[credentials]
key = "secret://hunter2"
"#,
    );
    confit()
        .args(["resolve", "credentials.key"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout("***");
}

#[test]
fn resolve_secret_revealed() {
    let dir = setup(
        r#"
[credentials]
key = "secret://hunter2"
"#,
    );
    confit()
        .args(["resolve", "credentials.key", "--reveal"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout("hunter2");
}

#[test]
fn resolve_missing_path() {
    let dir = setup("[app]\nname = \"x\"");
    confit()
        .args(["resolve", "app.missing"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn resolve_section_not_value() {
    let dir = setup(
        r#"
[app]
name = "x"
port = 3000
"#,
    );
    confit()
        .args(["resolve", "app"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("section"));
}

#[test]
fn resolve_file_provider() {
    let dir = setup(
        r#"
[db]
password = "file://secret.txt"
"#,
    );
    fs::write(dir.path().join("secret.txt"), "s3cret\n").unwrap();
    confit()
        .args(["resolve", "db.password"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout("s3cret");
}

#[test]
fn resolve_custom_provider() {
    let dir = setup(
        r#"
[providers.echo]
cmd = "echo resolved-{path}"
[app]
value = "echo://test"
"#,
    );
    confit()
        .args(["resolve", "app.value"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout("resolved-test");
}

#[test]
fn resolve_provider_missing_var() {
    let dir = setup(
        r#"
[providers.tf]
cmd = "echo {stage}-{path}"
[infra]
ip = "tf://server_ip"
"#,
    );
    confit()
        .args(["resolve", "infra.ip"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("stage"));
}

#[test]
fn resolve_circular_reference() {
    let dir = setup(
        r#"
[app]
a = "{app.b}"
b = "{app.a}"
"#,
    );
    confit()
        .args(["resolve", "app.a"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("Circular"));
}

// --- show (env format) ---

#[test]
fn show_env_format() {
    let dir = setup(
        r#"
[app]
host = "localhost"
port = 3000
"#,
    );
    let out = confit()
        .args(["show", "app"])
        .current_dir(dir.path())
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("host=localhost"));
    assert!(stdout.contains("port=3000"));
}

#[test]
fn show_export_upper() {
    let dir = setup(
        r#"
[app]
api_key = "abc"
"#,
    );
    confit()
        .args(["show", "app", "--export", "--upper"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("export API_KEY=abc"));
}

#[test]
fn show_secret_masked() {
    let dir = setup(
        r#"
[creds]
token = "secret://hunter2"
"#,
    );
    confit()
        .args(["show", "creds"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("token=***"));
}

#[test]
fn show_secret_revealed() {
    let dir = setup(
        r#"
[creds]
token = "secret://hunter2"
"#,
    );
    confit()
        .args(["show", "creds", "--reveal"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("token=hunter2"));
}

#[test]
fn show_upper_collision() {
    let dir = setup(
        r#"
[app]
api_key = "a"
API_KEY = "b"
"#,
    );
    confit()
        .args(["show", "app", "--upper"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("collide"));
}

// --- show (yaml format) ---

#[test]
fn show_yaml() {
    let dir = setup(
        r#"
[app]
name = "myapp"
port = 3000
"#,
    );
    let out = confit()
        .args(["show", "app", "--yaml"])
        .current_dir(dir.path())
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("name: myapp"));
    assert!(stdout.contains("port: 3000"));
}

#[test]
fn show_yaml_wrap() {
    let dir = setup(
        r#"
[app]
name = "myapp"
"#,
    );
    confit()
        .args(["show", "app", "--yaml", "--wrap", "config"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("config:"));
}

// --- keys ---

#[test]
fn keys_lists_keys() {
    let dir = setup(
        r#"
[services]
[services.web]
port = 3000
[services.api]
port = 4000
"#,
    );
    let out = confit()
        .args(["keys", "services"])
        .current_dir(dir.path())
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("web"));
    assert!(stdout.contains("api"));
}

#[test]
fn keys_nested() {
    let dir = setup(
        r#"
[services.web]
host = "localhost"
port = 3000
"#,
    );
    let out = confit()
        .args(["keys", "services.web"])
        .current_dir(dir.path())
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("host"));
    assert!(stdout.contains("port"));
}

#[test]
fn keys_not_a_section() {
    let dir = setup("[app]\nname = \"x\"");
    confit()
        .args(["keys", "app.name"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("not a section"));
}

// --- run ---

#[test]
fn run_injects_env() {
    let dir = setup(
        r#"
[app]
greeting = "hello"
"#,
    );
    confit()
        .args(["run", "app", "--", "sh", "-c", "echo $greeting"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("hello"));
}

#[test]
fn run_upper() {
    let dir = setup(
        r#"
[app]
api_key = "abc"
"#,
    );
    confit()
        .args([
            "run", "app", "--upper", "--", "sh", "-c", "echo $API_KEY",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("abc"));
}

#[test]
fn run_secrets_are_real_values() {
    let dir = setup(
        r#"
[creds]
token = "secret://hunter2"
"#,
    );
    confit()
        .args(["run", "creds", "--", "sh", "-c", "echo $token"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("hunter2"));
}

#[test]
fn run_upper_collision() {
    let dir = setup(
        r#"
[app]
api_key = "a"
API_KEY = "b"
"#,
    );
    confit()
        .args(["run", "app", "--upper", "--", "echo", "nope"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("collide"));
}

// --- validate ---

#[test]
fn validate_all_pass() {
    let dir = setup(
        r#"
[app]
name = "myapp"
port = 3000
"#,
    );
    confit()
        .arg("validate")
        .current_dir(dir.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("ok"));
}

#[test]
fn validate_section() {
    let dir = setup(
        r#"
[app]
name = "myapp"
[db]
host = "localhost"
"#,
    );
    confit()
        .args(["validate", "app"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("app.name"));
}

#[test]
fn validate_failure() {
    let dir = setup(
        r#"
[app]
name = "{missing.ref}"
"#,
    );
    confit()
        .arg("validate")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("failed"));
}

// --- log ---

#[test]
fn log_info() {
    let dir = TempDir::new().unwrap();
    confit()
        .args(["log", "hello world"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("hello world"));
}

#[test]
fn log_ok() {
    let dir = TempDir::new().unwrap();
    confit()
        .args(["log", "--ok", "done"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("done"));
}

#[test]
fn log_err() {
    let dir = TempDir::new().unwrap();
    confit()
        .args(["log", "--err", "failed"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("failed"));
}

// --- confit.toml discovery walks up ---

#[test]
fn config_found_in_parent() {
    let dir = setup(
        r#"
[app]
name = "found"
"#,
    );
    let child = dir.path().join("src/deep/nested");
    fs::create_dir_all(&child).unwrap();
    confit()
        .args(["resolve", "app.name"])
        .current_dir(&child)
        .assert()
        .success()
        .stdout("found");
}

// --- interpolation edge cases ---

#[test]
fn interpolation_chain() {
    let dir = setup(
        r#"
[vars]
region = "us-east"
[app]
prefix = "myapp-{vars.region}"
full = "{app.prefix}-server"
"#,
    );
    confit()
        .args(["resolve", "app.full"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout("myapp-us-east-server");
}

#[test]
fn array_value() {
    let dir = setup(
        r#"
[deploy]
tags = ["latest", "v1"]
"#,
    );
    confit()
        .args(["resolve", "deploy.tags"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout("latest v1");
}

// --- env var override ---

#[test]
fn env_var_overrides_toml() {
    let dir = setup(
        r#"
[vars]
region = "default"
[app]
endpoint = "https://{vars.region}.example.com"
"#,
    );
    confit()
        .args(["resolve", "app.endpoint"])
        .env("CONFIT_VAR_REGION", "eu-west")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout("https://eu-west.example.com");
}

#[test]
fn set_overrides_env_var() {
    let dir = setup(
        r#"
[vars]
region = "default"
[app]
endpoint = "{vars.region}"
"#,
    );
    confit()
        .args(["--set", "region=from-cli", "resolve", "app.endpoint"])
        .env("CONFIT_VAR_REGION", "from-env")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout("from-cli");
}
