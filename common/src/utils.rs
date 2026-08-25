use idgenerator::IdInstance;
use regex::Regex;
use serde_json::{Value, json};

pub const ZERO_ID: &str = match std::str::from_utf8(&[b'0'; 40]) {
    Ok(s) => s,
    Err(_) => panic!("can't get ZERO_ID"),
};

/// Returns true if `oid` looks like a full Git object id in hex form.
///
/// We intentionally only accept full-hex ids here (no short ids), because short ids
/// require repository-specific disambiguation.
///
/// Supported lengths:
/// - SHA-1   : 40 hex chars
/// - SHA-256 : 64 hex chars
pub fn is_full_hex_object_id(oid: &str) -> bool {
    let is_valid_len = oid.len() == 40 || oid.len() == 64;
    is_valid_len && oid.as_bytes().iter().all(|b| b.is_ascii_hexdigit())
}

pub fn generate_id() -> i64 {
    // Call `next_id` to generate a new unique id.
    IdInstance::next_id()
}

pub const MEGA_BRANCH_NAME: &str = "refs/heads/main";

pub fn generate_rich_text(content: &str) -> String {
    let json_str = r#"
    {
        "root": {
            "children": [{
                "children": [{ "detail": 0, "format": 0, "mode": "normal", "style": "", "text": "", "type": "text", "version": 1 }],
                "direction": "ltr", "format": "", "indent": 0, "type": "paragraph", "version": 1, "textFormat": 0, "textStyle": ""
            }], "direction": "ltr", "format": "", "indent": 0, "type": "root", "version": 1
        }
    }"#;
    let mut data: Value = serde_json::from_str(json_str).expect("Invalid JSON");

    if let Some(text_value) = data["root"]["children"][0]["children"][0].get_mut("text") {
        *text_value = json!(content);
    }
    serde_json::to_string_pretty(&data).expect("Failed to serialize JSON")
}

pub fn cl_ref_name(cl_link: &str) -> String {
    format!("refs/cl/{cl_link}")
}

/// Format commit message with GPG signature<br>
/// There must be a `blank line`(\n) before `message`, or remote unpack failed.<br>
/// If there is `GPG signature`,
/// `blank line` should be placed between `signature` and `message`
pub fn format_commit_msg(msg: &str, gpg_sig: Option<&str>) -> String {
    match gpg_sig {
        None => {
            format!("\n{msg}")
        }
        Some(gpg) => {
            format!("{gpg}\n\n{msg}")
        }
    }
}

/// parse commit message
pub fn parse_commit_msg(msg_gpg: &str) -> (&str, Option<&str>) {
    const SIG_PATTERN: &str = r"^gpgsig (-----BEGIN (?:PGP|SSH) SIGNATURE-----[\s\S]*?-----END (?:PGP|SSH) SIGNATURE-----)";
    const GPGSIG_PREFIX_LEN: usize = 7; // length of "gpgsig "

    let sig_regex = Regex::new(SIG_PATTERN).unwrap();
    if let Some(caps) = sig_regex.captures(msg_gpg) {
        let signature = caps.get(1).unwrap().as_str();

        let msg = &msg_gpg[signature.len() + GPGSIG_PREFIX_LEN..].trim_start();
        (msg, Some(signature))
    } else {
        (msg_gpg.trim_start(), None)
    }
}

// check if the commit message is conventional commit
// ref: https://www.conventionalcommits.org/en/v1.0.0/
pub fn check_conventional_commits_message(msg: &str) -> bool {
    let first_line = msg.lines().next().unwrap_or_default();
    #[allow(unused_variables)]
    let body_footer = msg.lines().skip(1).collect::<Vec<_>>().join("\n");

    let unicode_pattern = r"\p{L}\p{N}\p{P}\p{S}\p{Z}";
    // type only support characters&numbers, others fields support all unicode characters
    let regex_str = format!(
        r"^(?P<type>[\p{{L}}\p{{N}}_-]+)(?:\((?P<scope>[{unicode_pattern}]+)\))?!?: (?P<description>[{unicode_pattern}]+)$",
    );

    let re = Regex::new(&regex_str).unwrap();
    const RECOMMENDED_TYPES: [&str; 8] = [
        "build", "chore", "ci", "docs", "feat", "fix", "perf", "refactor",
    ];

    if let Some(captures) = re.captures(first_line) {
        let commit_type = captures.name("type").map(|m| m.as_str().to_string());
        #[allow(unused_variables)]
        let scope = captures.name("scope").map(|m| m.as_str().to_string());
        let description = captures.name("description").map(|m| m.as_str().to_string());
        if commit_type.is_none() || description.is_none() {
            return false;
        }

        let commit_type = commit_type.unwrap();
        if !RECOMMENDED_TYPES.contains(&commit_type.to_lowercase().as_str()) {
            println!(
                "`{commit_type}` is not a recommended commit type, refer to https://www.conventionalcommits.org/en/v1.0.0/ for more information"
            );
        }

        // println!("{}({}): {}\n{}", commit_type, scope.unwrap_or("None".to_string()), description.unwrap(), body_footer);

        return true;
    }
    false
}

pub fn get_current_bin_name() -> String {
    let bin_path = std::env::args().next().unwrap_or_default();
    std::path::Path::new(&bin_path)
        .file_name()
        .and_then(|os_str| os_str.to_str())
        .unwrap_or("unknown")
        .to_owned()
}

/// True when `parent` is a strict path-segment prefix of `child`.
///
/// Equal paths return false. `/third-party/rust` is not a prefix of
/// `/third-party/rust_v1`.
pub fn is_strict_path_prefix(parent: &str, child: &str) -> bool {
    let parent = parent.trim_end_matches('/');
    let child = child.trim_end_matches('/');
    if parent.is_empty() || child.is_empty() || parent == child {
        return false;
    }
    child.starts_with(parent) && child.as_bytes().get(parent.len()) == Some(&b'/')
}

/// If `new_path` would nest under or above any path in `existing`, return that
/// conflicting path. Same-path entries are ignored (update / idempotent create).
pub fn nested_import_repo_conflict<'a>(
    new_path: &str,
    existing: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
    let new_path = new_path.trim_end_matches('/');
    for path in existing {
        let trimmed = path.trim_end_matches('/');
        if trimmed.is_empty() || trimmed == new_path {
            continue;
        }
        if is_strict_path_prefix(trimmed, new_path) || is_strict_path_prefix(new_path, trimmed) {
            return Some(path);
        }
    }
    None
}

/// Human-readable error when creating an import repo would nest with an existing one.
pub fn nested_import_repo_conflict_message(new_path: &str, conflicting_path: &str) -> String {
    format!(
        "cannot create import repo at '{new_path}': nested with existing import repo at '{conflicting_path}'"
    )
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_is_strict_path_prefix() {
        assert!(is_strict_path_prefix(
            "/third-party/rust",
            "/third-party/rust/crates/sw/ay/swayws/1.3.0"
        ));
        assert!(is_strict_path_prefix(
            "/third-party/rust/",
            "/third-party/rust/crates"
        ));
        assert!(!is_strict_path_prefix("/third-party/rust", "/third-party/rust"));
        assert!(!is_strict_path_prefix("/third-party/rust", "/third-party/rust_v1"));
        assert!(!is_strict_path_prefix("/third-party/rust_v1", "/third-party/rust"));
        assert!(!is_strict_path_prefix("/third-party/foo", "/third-party/bar"));
    }

    #[test]
    fn test_nested_import_repo_conflict() {
        let existing = [
            "/third-party/rust/crates/sw/ay/swayws/1.3.0",
            "/third-party/rust_v1",
        ];
        assert_eq!(
            nested_import_repo_conflict("/third-party/rust", existing),
            Some("/third-party/rust/crates/sw/ay/swayws/1.3.0")
        );
        assert_eq!(
            nested_import_repo_conflict(
                "/third-party/rust/crates/to/ki/tokio/1.0.0",
                ["/third-party/rust"]
            ),
            Some("/third-party/rust")
        );
        assert!(
            nested_import_repo_conflict("/third-party/foo", existing).is_none()
        );
        assert!(
            nested_import_repo_conflict("/third-party/rust_v1", existing).is_none()
        );
        assert!(
            nested_import_repo_conflict(
                "/third-party/rust/crates/sw/ay/swayws/1.3.0",
                existing
            )
            .is_none()
        );
    }

    #[test]
    fn test_is_full_hex_object_id() {
        // Valid SHA-1 (40 hex)
        assert!(is_full_hex_object_id(&"0".repeat(40)));
        assert!(is_full_hex_object_id(&"a".repeat(40)));
        assert!(is_full_hex_object_id(&"A".repeat(40)));
        assert!(is_full_hex_object_id(&"f".repeat(40)));
        assert!(is_full_hex_object_id(&"F".repeat(40)));

        // Valid SHA-256 (64 hex)
        assert!(is_full_hex_object_id(&"0".repeat(64)));
        let sha256 = "abcdef".repeat(10) + "abcd"; // 60 + 4 = 64
        assert!(is_full_hex_object_id(&sha256));

        // Invalid lengths (we don't accept short ids)
        assert!(!is_full_hex_object_id(""));
        assert!(!is_full_hex_object_id(&"0".repeat(39)));
        assert!(!is_full_hex_object_id(&"0".repeat(41)));
        assert!(!is_full_hex_object_id(&"0".repeat(63)));
        assert!(!is_full_hex_object_id(&"0".repeat(65)));

        // Invalid characters
        assert!(!is_full_hex_object_id(
            &(String::from("g") + &"0".repeat(39))
        ));
        assert!(!is_full_hex_object_id(
            &(String::from("-") + &"0".repeat(39))
        ));
        assert!(!is_full_hex_object_id(
            &(String::from(" ") + &"0".repeat(39))
        ));
        assert!(!is_full_hex_object_id(
            &(String::from("é") + &"0".repeat(39))
        ));
    }

    #[test]
    fn test_check_conventional_commits() {
        // successfull cases
        let msg = "feat: add new feature";
        assert!(check_conventional_commits_message(msg));

        let msg = "fix(common crate): bug fix";
        assert!(check_conventional_commits_message(msg));

        let msg = "chore(范围)!: 依存関係を更新する";
        assert!(check_conventional_commits_message(msg));

        let msg = "se_lf-ty9pe(scope)!: Description\n\n여기 시체가 있어요\n\nвот нога";
        assert!(check_conventional_commits_message(msg));

        let msg = "feat(scope)!: Description\n\n\nbody one\n\nbody two\n\nfooter";
        assert!(check_conventional_commits_message(msg));

        // failed casesmsg
        let msg = "feat:add new feature"; // missing ' ' before ':'
        assert!(!check_conventional_commits_message(msg));

        let msg = "fix(common crate)bug fix"; // missing ':'
        assert!(!check_conventional_commits_message(msg));

        let msg = "類@型(common): add new feature"; // unssupported characters in type
        assert!(!check_conventional_commits_message(msg));

        let msg = "()(common): add new feature"; // unssupported characters in type
        assert!(!check_conventional_commits_message(msg));
    }
}
