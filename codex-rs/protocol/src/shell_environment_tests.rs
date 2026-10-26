use std::collections::HashMap;
use std::process::Command;

use pretty_assertions::assert_eq;

use super::*;

const CHILD_MODE_ENV_VAR: &str = "CODEX_SHELL_ENVIRONMENT_SCRUBBER_TEST_MODE";
const TEST_NAME: &str =
    "shell_environment::tests::command_scrubber_removes_names_from_real_child_environment";

#[test]
fn default_excludes_preserve_git_config_override_tuple() {
    let vars = [
        ("GIT_CONFIG_COUNT".to_string(), "1".to_string()),
        ("GIT_CONFIG_KEY_0".to_string(), "core.hooksPath".to_string()),
        (
            "GIT_CONFIG_VALUE_0".to_string(),
            "/tmp/codex-hooks".to_string(),
        ),
        ("API_KEY".to_string(), "filtered-key".to_string()),
        ("CLIENT_SECRET".to_string(), "filtered-secret".to_string()),
        ("ACCESS_TOKEN".to_string(), "filtered-token".to_string()),
        ("SAFE".to_string(), "preserved".to_string()),
    ];
    let policy = ShellEnvironmentPolicy {
        inherit: ShellEnvironmentPolicyInherit::All,
        ignore_default_excludes: false,
        ..Default::default()
    };

    assert_eq!(
        populate_env(vars, &policy, /*thread_id*/ None),
        HashMap::from([
            ("GIT_CONFIG_COUNT".to_string(), "1".to_string()),
            ("GIT_CONFIG_KEY_0".to_string(), "core.hooksPath".to_string(),),
            (
                "GIT_CONFIG_VALUE_0".to_string(),
                "/tmp/codex-hooks".to_string(),
            ),
            ("SAFE".to_string(), "preserved".to_string()),
        ])
    );
}

#[test]
fn default_excludes_only_exempt_strict_git_config_key_names() {
    let vars = [
        ("GIT_CONFIG_KEY_0".to_string(), "preserved-zero".to_string()),
        (
            "GIT_CONFIG_KEY_123".to_string(),
            "preserved-many".to_string(),
        ),
        ("GIT_CONFIG_KEY_".to_string(), "filtered-empty".to_string()),
        (
            "GIT_CONFIG_KEY_-1".to_string(),
            "filtered-negative".to_string(),
        ),
        (
            "GIT_CONFIG_KEY_1A".to_string(),
            "filtered-suffix".to_string(),
        ),
        (
            "GIT_CONFIG_KEY_0_EXTRA".to_string(),
            "filtered-extra".to_string(),
        ),
        (
            "GIT_CONFIG_KEY_１２".to_string(),
            "filtered-unicode-digits".to_string(),
        ),
        (
            "PREFIX_GIT_CONFIG_KEY_0".to_string(),
            "filtered-prefix".to_string(),
        ),
        ("MONKEY".to_string(), "filtered-generic".to_string()),
    ];
    let policy = ShellEnvironmentPolicy {
        inherit: ShellEnvironmentPolicyInherit::All,
        ignore_default_excludes: false,
        ..Default::default()
    };

    assert_eq!(
        populate_env(vars, &policy, /*thread_id*/ None),
        HashMap::from([
            ("GIT_CONFIG_KEY_0".to_string(), "preserved-zero".to_string()),
            (
                "GIT_CONFIG_KEY_123".to_string(),
                "preserved-many".to_string(),
            ),
        ])
    );
}

#[cfg(not(windows))]
#[test]
fn git_config_key_exception_uses_unix_case_sensitive_env_semantics() {
    let vars = [("git_config_key_0".to_string(), "filtered".to_string())];
    let policy = ShellEnvironmentPolicy {
        inherit: ShellEnvironmentPolicyInherit::All,
        ignore_default_excludes: false,
        ..Default::default()
    };

    assert_eq!(
        populate_env(vars, &policy, /*thread_id*/ None),
        HashMap::new()
    );
}

#[test]
fn git_config_key_exception_only_applies_to_default_excludes() {
    let vars = [
        (
            "GIT_CONFIG_KEY_0".to_string(),
            "filtered-custom".to_string(),
        ),
        (
            "GIT_CONFIG_VALUE_0".to_string(),
            "filtered-by-include-only".to_string(),
        ),
        ("SAFE".to_string(), "preserved".to_string()),
    ];
    let policy = ShellEnvironmentPolicy {
        inherit: ShellEnvironmentPolicyInherit::All,
        ignore_default_excludes: false,
        exclude: vec![EnvironmentVariablePattern::new("GIT_CONFIG_KEY_0")],
        include_only: vec![EnvironmentVariablePattern::new("SAFE")],
        ..Default::default()
    };

    assert_eq!(
        populate_env(vars, &policy, /*thread_id*/ None),
        HashMap::from([("SAFE".to_string(), "preserved".to_string())])
    );
}

#[cfg(windows)]
#[test]
fn git_config_key_exception_uses_windows_case_insensitive_env_semantics() {
    let vars = [
        ("Git_Config_Key_0".to_string(), "preserved".to_string()),
        (
            "Git_Config_Key_Not_An_Index".to_string(),
            "filtered".to_string(),
        ),
    ];
    let policy = ShellEnvironmentPolicy {
        inherit: ShellEnvironmentPolicyInherit::All,
        ignore_default_excludes: false,
        ..Default::default()
    };

    assert_eq!(
        populate_env(vars, &policy, /*thread_id*/ None),
        HashMap::from([("Git_Config_Key_0".to_string(), "preserved".to_string())])
    );
}

#[test]
fn tool_call_id_replaces_platform_equivalent_names() {
    let representable = format!("exec-雪-{}", "x".repeat(512));
    for (call_id, expected_call) in [
        (None, None),
        (Some(""), Some("")),
        (Some("invalid\0call"), None),
        (Some("current-call"), Some("current-call")),
        (Some(representable.as_str()), Some(representable.as_str())),
    ] {
        let mut env = HashMap::from([
            ("CODEX_TOOL_CALL_ID".to_string(), "stale-call".to_string()),
            (
                "Codex_Tool_Call_Id".to_string(),
                "mixed-case-call".to_string(),
            ),
            (
                "codex_tool_call_id".to_string(),
                "lowercase-call".to_string(),
            ),
            ("OTHER".to_string(), "unchanged".to_string()),
        ]);

        set_tool_call_id_env_var(&mut env, call_id);

        let mut expected = HashMap::from([("OTHER".to_string(), "unchanged".to_string())]);
        #[cfg(not(windows))]
        expected.extend([
            (
                "Codex_Tool_Call_Id".to_string(),
                "mixed-case-call".to_string(),
            ),
            (
                "codex_tool_call_id".to_string(),
                "lowercase-call".to_string(),
            ),
        ]);
        if let Some(expected_call) = expected_call {
            expected.insert("CODEX_TOOL_CALL_ID".to_string(), expected_call.to_string());
        }
        assert_eq!(env, expected, "call_id: {call_id:?}");
    }
}

#[test]
fn non_inheritable_environment_is_removed_after_policy_overrides() {
    let vars = [
        ("SAFE".to_string(), "inherited".to_string()),
        (
            "codex_guardian_decisions_api_key".into(),
            "synthetic-inherited-key".into(),
        ),
        ("node_repl_auth_token".into(), "inherited-token".into()),
        (
            "openai_federation_rule_id".to_string(),
            "inherited-rule".to_string(),
        ),
        (
            "OPENAI_WORKLOAD_IDENTITY_CONTEXT".to_string(),
            r#"{"instance_id":"box-one"}"#.to_string(),
        ),
        (
            "codex_exec_server_noise_auth_token".to_string(),
            "inherited-noise-token".to_string(),
        ),
    ];
    let policy = ShellEnvironmentPolicy {
        inherit: ShellEnvironmentPolicyInherit::All,
        ignore_default_excludes: true,
        r#set: HashMap::from([
            ("SAFE".to_string(), "override".to_string()),
            (
                "CODEX_GUARDIAN_DECISIONS_API_KEY".into(),
                "synthetic-override-key".into(),
            ),
            ("Node_Repl_Auth_Token".into(), "configured-token".into()),
            (
                "OpenAI_Identity_Token_File".to_string(),
                "/run/identity-token".to_string(),
            ),
            (
                "Codex_Exec_Server_Noise_Auth_Token".to_string(),
                "configured-noise-token".to_string(),
            ),
        ]),
        ..Default::default()
    };

    assert_eq!(
        populate_env(vars, &policy, /*thread_id*/ None),
        HashMap::from([("SAFE".to_string(), "override".to_string())])
    );
}

#[test]
fn command_scrubber_removes_names_from_real_child_environment() {
    if std::env::var_os(CHILD_MODE_ENV_VAR).is_none() {
        let output = Command::new(std::env::current_exe().expect("locate current test binary"))
            .args([TEST_NAME, "--exact", "--nocapture"])
            .env(CHILD_MODE_ENV_VAR, "1")
            .env(
                "Codex_Guardian_Decisions_Api_Key",
                "synthetic-inherited-key",
            )
            .env("Node_Repl_Auth_Token", "inherited-token")
            .env("OpenAI_Federation_Rule_Id", "inherited-rule")
            .env(
                "OpenAI_Workload_Identity_Context",
                r#"{"instance_id":"box-one"}"#,
            )
            .env(
                "Codex_Exec_Server_Noise_Auth_Token",
                "inherited-noise-token",
            )
            .output()
            .expect("run inherited-environment test process");
        assert!(
            output.status.success(),
            "child failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        return;
    }

    let mut command = environment_command();
    command
        .env("NODE_REPL_AUTH_TOKEN", "configured-token")
        .env("openai_identity_token_file", "/run/identity-token")
        .env(
            CODEX_EXEC_SERVER_NOISE_AUTH_TOKEN_ENV_VAR,
            "configured-noise-token",
        )
        .env("CODEX_GUARDIAN_DECISIONS_API_KEY", "synthetic-override-key")
        .env("SAFE", "value");
    scrub_non_inheritable_env_vars(&mut command);
    let output = command.output().expect("read child environment");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let restricted_names = stdout
        .lines()
        .filter_map(|line| line.split_once('=').map(|(name, _)| name))
        .filter(|name| is_non_inheritable_env_var(name))
        .collect::<Vec<_>>();
    assert_eq!(restricted_names, Vec::<&str>::new());
}

#[cfg(windows)]
fn environment_command() -> Command {
    let mut command = Command::new("cmd.exe");
    command.args(["/D", "/C", "set"]);
    command
}

#[cfg(not(windows))]
fn environment_command() -> Command {
    Command::new("env")
}
