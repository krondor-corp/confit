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
        .args(["run", "app", "--upper", "--", "sh", "-c", "echo $API_KEY"])
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

// --- sources ---

#[test]
fn source_string_shorthand_resolves_field() {
    let dir = setup(
        r#"
[sources]
bag = "printf 'FOO=hello\nBAR=world\n'"

[app]
foo = "bag://FOO"
bar = "bag://BAR"
"#,
    );
    confit()
        .args(["resolve", "app.foo"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout("hello");
    confit()
        .args(["resolve", "app.bar"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout("world");
}

#[test]
fn source_table_form_resolves_field() {
    let dir = setup(
        r#"
[sources.vault]
load = "printf 'TOKEN=abc123\n'"

[app]
token = "vault://TOKEN"
"#,
    );
    confit()
        .args(["resolve", "app.token"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout("abc123");
}

#[test]
fn source_secret_flag_masks_output() {
    let dir = setup(
        r#"
[sources.vault]
load   = "printf 'PASS=hunter2\n'"
secret = true

[creds]
password = "vault://PASS"
"#,
    );
    confit()
        .args(["show", "creds"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("password=***"));
}

#[test]
fn source_secret_flag_revealed() {
    let dir = setup(
        r#"
[sources.vault]
load   = "printf 'PASS=hunter2\n'"
secret = true

[creds]
password = "vault://PASS"
"#,
    );
    confit()
        .args(["show", "creds", "--reveal"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("password=hunter2"));
}

#[test]
fn source_secret_prefix_composes() {
    let dir = setup(
        r#"
[sources]
plain = "printf 'TOKEN=abc123\n'"

[creds]
token = "secret://plain://TOKEN"
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
fn source_missing_field_errors() {
    let dir = setup(
        r#"
[sources]
bag = "printf 'FOO=hello\n'"

[app]
val = "bag://NOPE"
"#,
    );
    confit()
        .args(["resolve", "app.val"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("NOPE"));
}

#[test]
fn source_vars_interpolation() {
    let dir = setup(
        r#"
[vars]
stage = "prod"

[sources]
mysrc = "printf 'STAGE={vars.stage}\n'"

[app]
val = "mysrc://STAGE"
"#,
    );
    confit()
        .args(["resolve", "app.val"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout("prod");
}

#[test]
fn env_source_builtin() {
    let dir = setup(
        r#"
[app]
path = "env://PATH"
"#,
    );
    confit()
        .args(["resolve", "app.path"])
        .current_dir(dir.path())
        .assert()
        .success();
}

#[test]
fn env_source_missing_var_errors() {
    let dir = setup(
        r#"
[app]
val = "env://DEFINITELY_NOT_SET_CONFIT_XYZ_123"
"#,
    );
    confit()
        .args(["resolve", "app.val"])
        .env_remove("DEFINITELY_NOT_SET_CONFIT_XYZ_123")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "DEFINITELY_NOT_SET_CONFIT_XYZ_123",
        ));
}

#[test]
fn source_show_multiple_keys() {
    let dir = setup(
        r#"
[sources]
bag = "printf 'A=alpha\nB=beta\n'"

[app]
a = "bag://A"
b = "bag://B"
"#,
    );
    let out = confit()
        .args(["show", "app"])
        .current_dir(dir.path())
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("a=alpha"));
    assert!(stdout.contains("b=beta"));
}

// --- export ---

fn git_init(dir: &std::path::Path) {
    std::process::Command::new("git")
        .arg("init")
        .current_dir(dir)
        .output()
        .unwrap();
}

#[test]
fn export_profile_dotenv() {
    let dir = setup(
        r#"
[credentials.app]
service_secret = "shh"
[accessories.postgres]
url = "postgres://localhost/db"
[env.dev]
SERVICE_SECRET = "{credentials.app.service_secret}"
POSTGRES_URL = "{accessories.postgres.url}"
HOST_NAME = "http://localhost:8000"
"#,
    );
    let out = confit()
        .args(["export", "--profile", "dev"])
        .current_dir(dir.path())
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("SERVICE_SECRET='shh'"), "got: {stdout}");
    assert!(stdout.contains("POSTGRES_URL='postgres://localhost/db'"));
    assert!(stdout.contains("HOST_NAME='http://localhost:8000'"));
}

#[test]
fn export_multi_section_later_wins() {
    let dir = setup(
        r#"
[base]
HOST = "base-host"
PORT = "1"
[override]
HOST = "override-host"
"#,
    );
    let out = confit()
        .args(["export", "base", "override"])
        .current_dir(dir.path())
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("HOST='override-host'"), "got: {stdout}");
    assert!(stdout.contains("PORT='1'"));
    assert!(!stdout.contains("base-host"));
}

#[test]
fn export_shell_format() {
    let dir = setup("[app]\nfoo = \"bar\"\n");
    confit()
        .args(["export", "app", "--format", "shell"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("export foo='bar'"));
}

#[test]
fn export_json_format() {
    let dir = setup("[app]\nhost = \"localhost\"\n");
    confit()
        .args(["export", "app", "--format", "json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("\"host\": \"localhost\""));
}

#[test]
fn export_quotes_embedded_single_quote() {
    let dir = setup("[app]\nmsg = \"it's fine\"\n");
    confit()
        .args(["export", "app"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("msg='it'\\''s fine'"));
}

#[test]
fn export_upper_and_prefix() {
    let dir = setup("[app]\nkey = \"v\"\n");
    confit()
        .args(["export", "app", "--upper", "--prefix", "app_"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("APP_KEY='v'"));
}

#[test]
fn export_refuses_secret_without_reveal() {
    let dir = setup("[creds]\ntoken = \"secret://hunter2\"\n");
    confit()
        .args(["export", "creds"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("--reveal"));
}

#[test]
fn export_reveals_secret_with_flag() {
    let dir = setup("[creds]\ntoken = \"secret://hunter2\"\n");
    confit()
        .args(["export", "creds", "--reveal"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("token='hunter2'"));
}

#[test]
fn export_no_source_errors() {
    let dir = setup("[app]\nx = \"1\"\n");
    confit()
        .arg("export")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("nothing to export"));
}

#[test]
fn export_out_refuses_non_gitignored() {
    let dir = setup("[app]\nx = \"1\"\n");
    git_init(dir.path());
    confit()
        .args(["export", "app", "--out", ".env.dev"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("not gitignored").or(predicate::str::contains("gitignore")),
        );
    assert!(!dir.path().join(".env.dev").exists());
}

#[test]
fn export_out_writes_gitignored_file() {
    let dir = setup("[app]\nhost = \"localhost\"\n");
    git_init(dir.path());
    fs::write(dir.path().join(".gitignore"), ".env.dev\n").unwrap();
    let out = confit()
        .args(["export", "app", "--out", ".env.dev"])
        .current_dir(dir.path())
        .assert()
        .success();
    assert!(out.get_output().stdout.is_empty());
    let stderr = String::from_utf8(out.get_output().stderr.clone()).unwrap();
    assert!(stderr.contains("wrote 1 vars"), "got: {stderr}");
    let path = dir.path().join(".env.dev");
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("host='localhost'"));
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "expected 0600, got {mode:o}");
}

#[test]
fn export_out_force_overrides_guard() {
    let dir = setup("[app]\nhost = \"localhost\"\n");
    git_init(dir.path());
    confit()
        .args(["export", "app", "--out", ".env.dev", "--force"])
        .current_dir(dir.path())
        .assert()
        .success();
    assert!(dir.path().join(".env.dev").exists());
}

#[test]
fn export_out_warns_outside_git_repo() {
    let dir = setup("[app]\nhost = \"localhost\"\n");
    confit()
        .args(["export", "app", "--out", ".env.dev"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("could not confirm"));
    assert!(dir.path().join(".env.dev").exists());
}

#[test]
fn export_out_secret_requires_reveal() {
    let dir = setup("[creds]\ntoken = \"secret://hunter2\"\n");
    fs::write(dir.path().join(".gitignore"), ".env.dev\n").unwrap();
    confit()
        .args(["export", "creds", "--out", ".env.dev"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("--reveal"));
    assert!(!dir.path().join(".env.dev").exists());
}

#[test]
fn export_profile_pins_vars() {
    // The profile pins stage=development via dotted `vars.stage`; the provider
    // template uses {stage}, so it must resolve without --set on the CLI.
    let dir = setup(
        r#"
[providers.echo]
cmd = "printf %s {stage}-{path}"
[secrets]
token = "echo://abc"
[env.dev]
vars.stage = "development"
TOKEN = "{secrets.token}"
"#,
    );
    confit()
        .args(["export", "--profile", "dev"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("TOKEN='development-abc'"));
}

#[test]
fn export_profile_vars_overridden_by_set() {
    // Sub-table form ([env.dev.vars]) is equivalent to dotted keys; covered here.
    let dir = setup(
        r#"
[providers.echo]
cmd = "printf %s {stage}"
[secrets]
token = "echo://x"
[env.dev]
STAGE = "{secrets.token}"
[env.dev.vars]
stage = "development"
"#,
    );
    confit()
        .args(["--set", "stage=staging", "export", "--profile", "dev"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("STAGE='staging'"));
}

#[test]
fn export_profile_from_source() {
    // A profile composes fields from a [sources] bulk loader (loads once).
    let dir = setup(
        r#"
[sources]
bag = "printf 'DB=postgres://x\nAPI=k3y\n'"
[env.dev]
DATABASE_URL = "bag://DB"
API_KEY = "bag://API"
"#,
    );
    confit()
        .args(["export", "--profile", "dev"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(
            predicate::str::contains("DATABASE_URL='postgres://x'")
                .and(predicate::str::contains("API_KEY='k3y'")),
        );
}
