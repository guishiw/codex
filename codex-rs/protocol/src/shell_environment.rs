use crate::config_types::EnvironmentVariablePattern;
use crate::config_types::ShellEnvironmentPolicy;
use crate::config_types::ShellEnvironmentPolicyInherit;
use std::collections::HashMap;

pub const CODEX_SESSION_ID_ENV_VAR: &str = "CODEX_SESSION_ID";
pub const CODEX_THREAD_ID_ENV_VAR: &str = "CODEX_THREAD_ID";
pub const CODEX_TOOL_CALL_ID_ENV_VAR: &str = "CODEX_TOOL_CALL_ID";
pub const CODEX_EXEC_SERVER_NOISE_AUTH_TOKEN_ENV_VAR: &str = "CODEX_EXEC_SERVER_NOISE_AUTH_TOKEN";
pub const OPENAI_FEDERATION_RULE_ID_ENV_VAR: &str = "OPENAI_FEDERATION_RULE_ID";
pub const OPENAI_IDENTITY_TOKEN_FILE_ENV_VAR: &str = "OPENAI_IDENTITY_TOKEN_FILE";
pub const OPENAI_WORKLOAD_IDENTITY_CONTEXT_ENV_VAR: &str = "OPENAI_WORKLOAD_IDENTITY_CONTEXT";

const GIT_CONFIG_COUNT_ENV_VAR: &str = "GIT_CONFIG_COUNT";
const GIT_CONFIG_KEY_ENV_VAR_PREFIX: &str = "GIT_CONFIG_KEY_";
const GIT_CONFIG_VALUE_ENV_VAR_PREFIX: &str = "GIT_CONFIG_VALUE_";
const GIT_CONFIG_COUNT_MAX: usize = i32::MAX as usize;

/// Environment variables that model-reachable child processes must not inherit.
pub const NON_INHERITABLE_ENV_VARS: &[&str] = &[
    CODEX_EXEC_SERVER_NOISE_AUTH_TOKEN_ENV_VAR,
    "NODE_REPL_AUTH_TOKEN",
    "CODEX_GUARDIAN_DECISIONS_API_KEY",
    OPENAI_FEDERATION_RULE_ID_ENV_VAR,
    OPENAI_IDENTITY_TOKEN_FILE_ENV_VAR,
    OPENAI_WORKLOAD_IDENTITY_CONTEXT_ENV_VAR,
];

pub fn is_non_inheritable_env_var(name: &str) -> bool {
    NON_INHERITABLE_ENV_VARS
        .iter()
        .any(|restricted| restricted.eq_ignore_ascii_case(name))
}

pub fn set_tool_call_id_env_var(env: &mut HashMap<String, String>, tool_call_id: Option<&str>) {
    #[cfg(windows)]
    env.retain(|name, _| !name.eq_ignore_ascii_case(CODEX_TOOL_CALL_ID_ENV_VAR));
    #[cfg(not(windows))]
    env.remove(CODEX_TOOL_CALL_ID_ENV_VAR);
    if let Some(tool_call_id) = tool_call_id.filter(|tool_call_id| !tool_call_id.contains('\0')) {
        env.insert(
            CODEX_TOOL_CALL_ID_ENV_VAR.to_string(),
            tool_call_id.to_string(),
        );
    }
}

/// Configures a child command to omit non-inheritable variables from the
/// process environment and explicit command overrides.
///
/// This prevents accidental propagation of Codex launch context; it is not a
/// filesystem security boundary for the referenced identity-token file.
pub fn scrub_non_inheritable_env_vars(command: &mut std::process::Command) {
    let configured_names = command
        .get_envs()
        .map(|(name, _)| name.to_os_string())
        .collect::<Vec<_>>();

    for name in NON_INHERITABLE_ENV_VARS {
        command.env_remove(name);
    }
    for name in std::env::vars_os()
        .map(|(name, _)| name)
        .chain(configured_names)
    {
        if name.to_str().is_some_and(is_non_inheritable_env_var) {
            command.env_remove(name);
        }
    }
}

/// Construct a shell environment from the supplied process environment and
/// shell-environment policy.
pub fn create_env(
    policy: &ShellEnvironmentPolicy,
    thread_id: Option<&str>,
) -> HashMap<String, String> {
    create_env_from_vars(std::env::vars(), policy, thread_id)
}

pub fn create_env_from_vars<I>(
    vars: I,
    policy: &ShellEnvironmentPolicy,
    thread_id: Option<&str>,
) -> HashMap<String, String>
where
    I: IntoIterator<Item = (String, String)>,
{
    let mut env_map = populate_env(vars, policy, thread_id);

    if cfg!(target_os = "windows") {
        // This is a workaround to address the failures we are seeing in the
        // following tests when run via Bazel on Windows:
        //
        // ```
        // suite::shell_command::unicode_output::with_login
        // suite::shell_command::unicode_output::without_login
        // ```
        //
        // Currently, we can only reproduce these failures in CI, which makes
        // iteration times long, so we include this quick fix for now to unblock
        // getting the Windows Bazel build running.
        if !env_map.keys().any(|k| k.eq_ignore_ascii_case("PATHEXT")) {
            env_map.insert("PATHEXT".to_string(), ".COM;.EXE;.BAT;.CMD".to_string());
        }
    }
    env_map
}

pub fn populate_env<I>(
    vars: I,
    policy: &ShellEnvironmentPolicy,
    thread_id: Option<&str>,
) -> HashMap<String, String>
where
    I: IntoIterator<Item = (String, String)>,
{
    // Step 1 - determine the starting set of variables based on the
    // `inherit` strategy.
    let mut env_map: HashMap<String, String> = match policy.inherit {
        ShellEnvironmentPolicyInherit::All => vars.into_iter().collect(),
        ShellEnvironmentPolicyInherit::None => HashMap::new(),
        ShellEnvironmentPolicyInherit::Core => {
            #[cfg(not(target_os = "windows"))]
            let core_env_vars = UNIX_CORE_ENV_VARS;
            #[cfg(target_os = "windows")]
            let core_env_vars = WINDOWS_CORE_ENV_VARS;

            vars.into_iter()
                .filter(|(k, _)| {
                    core_env_vars
                        .iter()
                        .any(|allowed| allowed.eq_ignore_ascii_case(k))
                })
                .collect()
        }
    };

    let matches_any = |name: &str, patterns: &[EnvironmentVariablePattern]| -> bool {
        patterns.iter().any(|pattern| pattern.matches(name))
    };

    // Step 2 - Apply the default exclude if not disabled.
    if !policy.ignore_default_excludes {
        let default_excludes = vec![
            EnvironmentVariablePattern::new_case_insensitive("*KEY*"),
            EnvironmentVariablePattern::new_case_insensitive("*SECRET*"),
            EnvironmentVariablePattern::new_case_insensitive("*TOKEN*"),
        ];
        env_map.retain(|k, _| !matches_any(k, &default_excludes));
    }

    // Step 3 - Apply custom excludes.
    if !policy.exclude.is_empty() {
        env_map.retain(|k, _| !matches_any(k, &policy.exclude));
    }

    // Step 4 - Apply user-provided overrides.
    for (key, val) in &policy.r#set {
        #[cfg(windows)]
        env_map.retain(|existing, _| !existing.eq_ignore_ascii_case(key));
        env_map.insert(key.clone(), val.clone());
    }

    // Step 5 - If include_only is non-empty, keep only the matching vars.
    if !policy.include_only.is_empty() {
        env_map.retain(|k, _| matches_any(k, &policy.include_only));
    }

    // Step 6 - Git treats GIT_CONFIG_COUNT and each indexed KEY/VALUE pair as
    // a single unit. Any shell-environment policy stage can otherwise leave a
    // partial tuple that makes every Git command fail, so drop an incomplete
    // tuple conservatively after all user-configurable filtering and
    // overrides.
    remove_incomplete_git_config_env(&mut env_map);

    // Step 7 - Populate the thread ID environment variable when provided.
    if let Some(thread_id) = thread_id {
        env_map.insert(CODEX_THREAD_ID_ENV_VAR.to_string(), thread_id.to_string());
    }

    // Restricted launch context cannot be restored through user-provided shell
    // environment overrides.
    env_map.retain(|name, _| !is_non_inheritable_env_var(name));

    env_map
}

fn remove_incomplete_git_config_env(env_map: &mut HashMap<String, String>) {
    let count_values = env_map
        .iter()
        .filter(|(name, _)| env_name_eq(name, GIT_CONFIG_COUNT_ENV_VAR))
        .map(|(_, value)| value.clone())
        .collect::<Vec<_>>();
    let count = match count_values.as_slice() {
        [] => return,
        [count] => match parse_git_config_count(count) {
            Ok(count) => count,
            Err(_) => {
                remove_git_config_env(env_map);
                return;
            }
        },
        // Multiple case-equivalent count variables on Windows do not form an
        // unambiguous tuple.
        _ => {
            remove_git_config_env(env_map);
            return;
        }
    };

    if count == 0 {
        return;
    }

    // Each configured entry requires both a key and a value. Reject counts
    // that cannot possibly fit in the current environment before iterating so
    // a hostile or corrupt count cannot cause an unbounded loop.
    if count > env_map.len() / 2 {
        remove_git_config_env(env_map);
        return;
    }

    let is_complete = (0..count).all(|index| {
        let key_name = format!("{GIT_CONFIG_KEY_ENV_VAR_PREFIX}{index}");
        let value_name = format!("{GIT_CONFIG_VALUE_ENV_VAR_PREFIX}{index}");
        env_contains_key(env_map, &key_name) && env_contains_key(env_map, &value_name)
    });

    if !is_complete {
        remove_git_config_env(env_map);
    }
}

fn remove_git_config_env(env_map: &mut HashMap<String, String>) {
    env_map.retain(|name, _| {
        !env_name_eq(name, GIT_CONFIG_COUNT_ENV_VAR)
            && !is_indexed_git_config_env_var(name, GIT_CONFIG_KEY_ENV_VAR_PREFIX)
            && !is_indexed_git_config_env_var(name, GIT_CONFIG_VALUE_ENV_VAR_PREFIX)
    });
}

fn parse_git_config_count(value: &str) -> Result<usize, ()> {
    // Git parses this variable with strtoul(3), then rejects values above
    // INT_MAX. Match its accepted spellings, including an empty string (zero),
    // leading ASCII whitespace, an optional sign, and leading zeroes. A minus
    // sign is only valid when the parsed value is zero.
    if value.is_empty() {
        return Ok(0);
    }

    let bytes = value.as_bytes();
    let mut index = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .ok_or(())?;
    let is_negative = bytes[index] == b'-';
    if bytes[index] == b'+' || is_negative {
        index += 1;
    }
    if index == bytes.len() || !bytes[index].is_ascii_digit() {
        return Err(());
    }

    let mut count = 0usize;
    for byte in &bytes[index..] {
        if !byte.is_ascii_digit() {
            return Err(());
        }
        count = count
            .checked_mul(10)
            .and_then(|count| count.checked_add(usize::from(byte - b'0')))
            .filter(|count| *count <= GIT_CONFIG_COUNT_MAX)
            .ok_or(())?;
    }
    if is_negative && count != 0 {
        Err(())
    } else {
        Ok(count)
    }
}

fn is_indexed_git_config_env_var(name: &str, prefix: &str) -> bool {
    env_name_strip_prefix(name, prefix).is_some_and(|suffix| {
        !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
    })
}

#[cfg(not(windows))]
fn env_contains_key(env_map: &HashMap<String, String>, name: &str) -> bool {
    env_map.contains_key(name)
}

#[cfg(windows)]
fn env_contains_key(env_map: &HashMap<String, String>, name: &str) -> bool {
    env_map.keys().any(|candidate| env_name_eq(candidate, name))
}

#[cfg(not(windows))]
fn env_name_eq(name: &str, expected: &str) -> bool {
    name == expected
}

#[cfg(windows)]
fn env_name_eq(name: &str, expected: &str) -> bool {
    name.eq_ignore_ascii_case(expected)
}

#[cfg(not(windows))]
fn env_name_strip_prefix<'a>(name: &'a str, prefix: &str) -> Option<&'a str> {
    name.strip_prefix(prefix)
}

#[cfg(windows)]
fn env_name_strip_prefix<'a>(name: &'a str, prefix: &str) -> Option<&'a str> {
    let candidate = name.as_bytes().get(..prefix.len())?;
    candidate
        .eq_ignore_ascii_case(prefix.as_bytes())
        .then(|| &name[prefix.len()..])
}

#[cfg(not(target_os = "windows"))]
const UNIX_CORE_ENV_VARS: &[&str] = &[
    "PATH", "SHELL", "TMPDIR", "TEMP", "TMP", "HOME", "LANG", "LC_ALL", "LC_CTYPE", "LOGNAME",
    "USER",
];

#[cfg(target_os = "windows")]
pub const WINDOWS_CORE_ENV_VARS: &[&str] = &[
    // Core path resolution
    "PATH",
    "PATHEXT",
    // Shell and system roots
    "SHELL",
    "COMSPEC",
    "SYSTEMROOT",
    "WINDIR",
    "SYSTEMDRIVE",
    // User context and profiles
    "USERNAME",
    "USERDOMAIN",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    // Program locations
    "PROGRAMFILES",
    "PROGRAMFILES(X86)",
    "PROGRAMW6432",
    "PROGRAMDATA",
    // App data and caches
    "LOCALAPPDATA",
    "APPDATA",
    // Temp locations
    "TEMP",
    "TMP",
    "TMPDIR",
    // Common shells/pwsh hints
    "POWERSHELL",
    "PWSH",
];

#[cfg(test)]
#[path = "shell_environment_tests.rs"]
mod tests;

#[cfg(all(test, target_os = "windows"))]
mod windows_tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn make_vars(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn core_inherit_preserves_windows_startup_vars_case_insensitively() {
        let vars = make_vars(&[
            ("Shell", "C:\\Program Files\\Git\\bin\\bash.exe"),
            ("SystemRoot", "C:\\Windows"),
            ("WinDir", "C:\\Windows"),
            ("AppData", "C:\\Users\\codex\\AppData\\Roaming"),
            ("TmpDir", "C:\\Temp\\custom"),
            ("OPENAI_API_KEY", "secret"),
        ]);

        let policy = ShellEnvironmentPolicy {
            inherit: ShellEnvironmentPolicyInherit::Core,
            ignore_default_excludes: true,
            ..Default::default()
        };

        // Check a few sample vars instead of the full Windows core list.
        let result = populate_env(vars, &policy, /*thread_id*/ None);
        let expected = HashMap::from([
            (
                "Shell".to_string(),
                "C:\\Program Files\\Git\\bin\\bash.exe".to_string(),
            ),
            ("SystemRoot".to_string(), "C:\\Windows".to_string()),
            ("WinDir".to_string(), "C:\\Windows".to_string()),
            (
                "AppData".to_string(),
                "C:\\Users\\codex\\AppData\\Roaming".to_string(),
            ),
            ("TmpDir".to_string(), "C:\\Temp\\custom".to_string()),
        ]);

        assert_eq!(result, expected);

        let policy = ShellEnvironmentPolicy {
            inherit: ShellEnvironmentPolicyInherit::All,
            ignore_default_excludes: true,
            r#set: HashMap::from([("gh_host".to_string(), "github.trusted.example".to_string())]),
            ..Default::default()
        };
        assert_eq!(
            populate_env(
                make_vars(&[("GH_HOST", "github.stale.example")]),
                &policy,
                /*thread_id*/ None,
            ),
            HashMap::from([("gh_host".to_string(), "github.trusted.example".to_string(),)]),
        );
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn create_env_inserts_pathext_on_windows_when_missing() {
        let policy = ShellEnvironmentPolicy {
            inherit: ShellEnvironmentPolicyInherit::None,
            ignore_default_excludes: true,
            ..Default::default()
        };

        let result = create_env_from_vars(Vec::new(), &policy, /*thread_id*/ None);
        let expected = HashMap::from([("PATHEXT".to_string(), ".COM;.EXE;.BAT;.CMD".to_string())]);

        assert_eq!(result, expected);
    }
}

#[cfg(all(test, not(target_os = "windows")))]
mod non_windows_tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn make_vars(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    fn core_inherit_preserves_non_windows_core_vars_case_insensitively() {
        let vars = make_vars(&[
            ("path", "/usr/bin"),
            ("home", "/home/codex"),
            ("TmpDir", "/tmp/custom"),
            ("OPENAI_API_KEY", "secret"),
        ]);

        let policy = ShellEnvironmentPolicy {
            inherit: ShellEnvironmentPolicyInherit::Core,
            ignore_default_excludes: true,
            ..Default::default()
        };

        let result = populate_env(vars, &policy, /*thread_id*/ None);
        let expected = HashMap::from([
            ("path".to_string(), "/usr/bin".to_string()),
            ("home".to_string(), "/home/codex".to_string()),
            ("TmpDir".to_string(), "/tmp/custom".to_string()),
        ]);

        assert_eq!(result, expected);
    }
}
