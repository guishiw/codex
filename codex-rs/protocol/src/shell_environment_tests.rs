use std::collections::HashMap;
use std::process::Command;

use pretty_assertions::assert_eq;

use super::*;

const CHILD_MODE_ENV_VAR: &str = "CODEX_SHELL_ENVIRONMENT_SCRUBBER_TEST_MODE";
const TEST_NAME: &str =
    "shell_environment::tests::command_scrubber_removes_names_from_real_child_environment";

fn vars(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect()
}

fn inherit_all_policy() -> ShellEnvironmentPolicy {
    ShellEnvironmentPolicy {
        inherit: ShellEnvironmentPolicyInherit::All,
        ignore_default_excludes: true,
        ..Default::default()
    }
}

#[test]
fn incomplete_git_config_tuple_is_removed_after_default_excludes() {
    let policy = ShellEnvironmentPolicy {
        ignore_default_excludes: false,
        ..inherit_all_policy()
    };
    let result = populate_env(
        vars(&[
            ("GIT_CONFIG_COUNT", "1"),
            ("GIT_CONFIG_KEY_0", "core.hooksPath"),
            ("GIT_CONFIG_VALUE_0", "/tmp/codex-hooks"),
            ("SAFE", "kept"),
        ]),
        &policy,
        /*thread_id*/ None,
    );

    assert_eq!(
        result,
        HashMap::from([("SAFE".to_string(), "kept".to_string())])
    );
}

#[test]
fn complete_git_config_tuple_is_preserved() {
    let input = vars(&[
        ("GIT_CONFIG_COUNT", "2"),
        ("GIT_CONFIG_KEY_0", "core.hooksPath"),
        ("GIT_CONFIG_VALUE_0", "/tmp/codex-hooks"),
        ("GIT_CONFIG_KEY_1", "user.name"),
        // An empty value is still a present, valid member of the tuple.
        ("GIT_CONFIG_VALUE_1", ""),
        ("SAFE", "kept"),
    ]);

    assert_eq!(
        populate_env(
            input.clone(),
            &inherit_all_policy(),
            /*thread_id*/ None
        ),
        input.into_iter().collect()
    );
}

#[test]
fn zero_git_config_count_is_preserved() {
    let result = populate_env(
        vars(&[
            ("GIT_CONFIG_COUNT", "0"),
            ("GIT_CONFIG_KEY_0", "ignored.by.git"),
            ("GIT_CONFIG_VALUE_0", "also-ignored"),
            ("SAFE", "kept"),
        ]),
        &inherit_all_policy(),
        /*thread_id*/ None,
    );

    assert_eq!(
        result,
        HashMap::from([
            ("GIT_CONFIG_COUNT".to_string(), "0".to_string()),
            ("GIT_CONFIG_KEY_0".to_string(), "ignored.by.git".to_string(),),
            ("GIT_CONFIG_VALUE_0".to_string(), "also-ignored".to_string(),),
            ("SAFE".to_string(), "kept".to_string()),
        ])
    );
}

#[test]
fn zero_git_config_count_remains_valid_after_default_excludes() {
    let policy = ShellEnvironmentPolicy {
        ignore_default_excludes: false,
        ..inherit_all_policy()
    };
    let result = populate_env(
        vars(&[
            ("GIT_CONFIG_COUNT", "0"),
            ("GIT_CONFIG_KEY_0", "ignored.by.git"),
            ("GIT_CONFIG_VALUE_0", "also-ignored"),
            ("SAFE", "kept"),
        ]),
        &policy,
        /*thread_id*/ None,
    );

    assert_eq!(
        result,
        HashMap::from([
            ("GIT_CONFIG_COUNT".to_string(), "0".to_string()),
            ("GIT_CONFIG_VALUE_0".to_string(), "also-ignored".to_string()),
            ("SAFE".to_string(), "kept".to_string()),
        ])
    );
}

#[test]
fn git_compatible_config_count_spellings_are_accepted() {
    for count in [
        "", "0", "00", "+0", "-0", "-00", "  -0", "1", "+1", "01", "\t 1",
    ] {
        let input = vars(&[
            ("GIT_CONFIG_COUNT", count),
            ("GIT_CONFIG_KEY_0", "user.name"),
            ("GIT_CONFIG_VALUE_0", "Codex"),
            ("SAFE", "kept"),
        ]);
        assert_eq!(
            populate_env(
                input.clone(),
                &inherit_all_policy(),
                /*thread_id*/ None,
            ),
            input.into_iter().collect(),
            "count: {count:?}"
        );
    }
}

#[test]
fn complete_tuple_ignores_and_preserves_indices_beyond_count() {
    let input = vars(&[
        ("GIT_CONFIG_COUNT", "1"),
        ("GIT_CONFIG_KEY_0", "user.name"),
        ("GIT_CONFIG_VALUE_0", "Codex"),
        ("GIT_CONFIG_KEY_9", "ignored.by.git"),
        ("GIT_CONFIG_VALUE_9", "also-ignored"),
    ]);

    assert_eq!(
        populate_env(
            input.clone(),
            &inherit_all_policy(),
            /*thread_id*/ None,
        ),
        input.into_iter().collect()
    );
}

#[test]
fn sparse_or_incomplete_declared_git_config_entries_remove_the_family() {
    for remaining in [
        vars(&[
            ("GIT_CONFIG_KEY_0", "user.name"),
            ("GIT_CONFIG_VALUE_0", "Codex"),
            ("GIT_CONFIG_VALUE_1", "codex@example.com"),
        ]),
        vars(&[
            ("GIT_CONFIG_KEY_0", "user.name"),
            ("GIT_CONFIG_VALUE_0", "Codex"),
            ("GIT_CONFIG_KEY_1", "user.email"),
        ]),
    ] {
        let result = populate_env(
            std::iter::once(("GIT_CONFIG_COUNT".to_string(), "2".to_string()))
                .chain(remaining)
                .chain(std::iter::once(("SAFE".to_string(), "kept".to_string()))),
            &inherit_all_policy(),
            /*thread_id*/ None,
        );

        assert_eq!(
            result,
            HashMap::from([("SAFE".to_string(), "kept".to_string())])
        );
    }
}

#[test]
fn invalid_git_config_count_removes_the_indexed_family() {
    for count in [
        " ",
        "+",
        "-1",
        "1 ",
        "1x",
        "not-a-number",
        "2147483648",
        "184467440737095516160",
    ] {
        let result = populate_env(
            vars(&[
                ("GIT_CONFIG_COUNT", count),
                ("GIT_CONFIG_KEY_0", "core.hooksPath"),
                ("GIT_CONFIG_VALUE_0", "/tmp/codex-hooks"),
                ("GIT_CONFIG_KEY_backup", "similar-but-not-indexed"),
                ("GIT_CONFIG_VALUE_", "similar-but-not-indexed"),
                ("SAFE", "kept"),
            ]),
            &inherit_all_policy(),
            /*thread_id*/ None,
        );

        assert_eq!(
            result,
            HashMap::from([
                (
                    "GIT_CONFIG_KEY_backup".to_string(),
                    "similar-but-not-indexed".to_string(),
                ),
                (
                    "GIT_CONFIG_VALUE_".to_string(),
                    "similar-but-not-indexed".to_string(),
                ),
                ("SAFE".to_string(), "kept".to_string()),
            ]),
            "count: {count:?}"
        );
    }
}

#[test]
fn indexed_git_config_vars_without_count_are_unchanged() {
    let input = vars(&[
        ("GIT_CONFIG_KEY_0", "core.hooksPath"),
        ("GIT_CONFIG_VALUE_0", "/tmp/codex-hooks"),
        ("SAFE", "kept"),
    ]);

    assert_eq!(
        populate_env(
            input.clone(),
            &inherit_all_policy(),
            /*thread_id*/ None,
        ),
        input.into_iter().collect()
    );
}

#[cfg(not(windows))]
#[test]
fn git_config_tuple_names_are_case_sensitive_off_windows() {
    let result = populate_env(
        vars(&[
            ("GIT_CONFIG_COUNT", "1"),
            ("git_config_key_0", "user.name"),
            ("GIT_CONFIG_VALUE_0", "Codex"),
            ("SAFE", "kept"),
        ]),
        &inherit_all_policy(),
        /*thread_id*/ None,
    );

    assert_eq!(
        result,
        HashMap::from([
            ("git_config_key_0".to_string(), "user.name".to_string()),
            ("SAFE".to_string(), "kept".to_string()),
        ])
    );
}

#[test]
fn custom_excludes_cannot_leave_an_incomplete_git_config_tuple() {
    let policy = ShellEnvironmentPolicy {
        exclude: vec![EnvironmentVariablePattern::new_case_insensitive(
            "GIT_CONFIG_VALUE_1",
        )],
        ..inherit_all_policy()
    };
    let result = populate_env(
        vars(&[
            ("GIT_CONFIG_COUNT", "2"),
            ("GIT_CONFIG_KEY_0", "user.name"),
            ("GIT_CONFIG_VALUE_0", "Codex"),
            ("GIT_CONFIG_KEY_1", "user.email"),
            ("GIT_CONFIG_VALUE_1", "codex@example.com"),
            ("SAFE", "kept"),
        ]),
        &policy,
        /*thread_id*/ None,
    );

    assert_eq!(
        result,
        HashMap::from([("SAFE".to_string(), "kept".to_string())])
    );
}

#[test]
fn policy_set_can_restore_a_complete_git_config_tuple() {
    let policy = ShellEnvironmentPolicy {
        ignore_default_excludes: false,
        r#set: HashMap::from([("GIT_CONFIG_KEY_0".to_string(), "core.hooksPath".to_string())]),
        ..inherit_all_policy()
    };
    let result = populate_env(
        vars(&[
            ("GIT_CONFIG_COUNT", "1"),
            ("GIT_CONFIG_KEY_0", "filtered-before-set"),
            ("GIT_CONFIG_VALUE_0", "/tmp/codex-hooks"),
        ]),
        &policy,
        /*thread_id*/ None,
    );

    assert_eq!(
        result,
        HashMap::from([
            ("GIT_CONFIG_COUNT".to_string(), "1".to_string()),
            ("GIT_CONFIG_KEY_0".to_string(), "core.hooksPath".to_string(),),
            (
                "GIT_CONFIG_VALUE_0".to_string(),
                "/tmp/codex-hooks".to_string(),
            ),
        ])
    );
}

#[test]
fn include_only_cannot_leave_an_incomplete_git_config_tuple() {
    let policy = ShellEnvironmentPolicy {
        include_only: vec![
            EnvironmentVariablePattern::new_case_insensitive("GIT_CONFIG_COUNT"),
            EnvironmentVariablePattern::new_case_insensitive("GIT_CONFIG_VALUE_0"),
        ],
        ..inherit_all_policy()
    };
    let result = populate_env(
        vars(&[
            ("GIT_CONFIG_COUNT", "1"),
            ("GIT_CONFIG_KEY_0", "core.hooksPath"),
            ("GIT_CONFIG_VALUE_0", "/tmp/codex-hooks"),
            ("SAFE", "filtered"),
        ]),
        &policy,
        /*thread_id*/ None,
    );

    assert_eq!(result, HashMap::new());
}

#[cfg(windows)]
#[test]
fn git_config_tuple_names_are_case_insensitive_on_windows() {
    let complete = vars(&[
        ("Git_Config_Count", "1"),
        ("git_config_key_0", "user.name"),
        ("GIT_CONFIG_VALUE_0", "Codex"),
    ]);
    assert_eq!(
        populate_env(
            complete.clone(),
            &inherit_all_policy(),
            /*thread_id*/ None,
        ),
        complete.into_iter().collect()
    );

    let incomplete = populate_env(
        vars(&[
            ("Git_Config_Count", "1"),
            ("gIt_CoNfIg_VaLuE_0", "Codex"),
            ("SAFE", "kept"),
        ]),
        &inherit_all_policy(),
        /*thread_id*/ None,
    );
    assert_eq!(
        incomplete,
        HashMap::from([("SAFE".to_string(), "kept".to_string())])
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
