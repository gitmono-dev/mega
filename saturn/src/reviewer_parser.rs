//! Parser for extracting system required reviewers from Cedar policy files.
//!
//! This module parses custom Cedar syntax extensions that specify mandatory reviewers:
//! ```cedar
//! permit(action == "code:review", principal, resource)
//!     when { resource.path.startsWith("service_a/") }
//!     to ["alice", "bob"];
//! ```

use std::{
    collections::{HashMap, HashSet},
    sync::LazyLock,
};

use regex::Regex;

static RULE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?s)permit\s*\([^)]*\)\s*when\s*\{\s*resource\.path\.startsWith\s*\(\s*"([^"]*)"\s*\)\s*\}\s*to\s*\[([^\]]+)\]"#,
    )
    .unwrap()
});
static REVIEWER_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""([^"]+)""#).unwrap());

/// Represents a reviewer rule extracted from policy file
#[derive(Debug, Clone, PartialEq)]
pub struct ReviewerRule {
    /// Path pattern (e.g., "service_a/", "src/core/")
    pub path_pattern: String,
    /// List of required reviewers
    pub reviewers: Vec<String>,
}

/// Parse policy content and extract reviewer rules
///
/// # Arguments
/// * `policy_content` - The content of the .cedar/policies.cedar file
///
/// # Returns
/// A vector of ReviewerRule extracted from the policy
pub fn parse_reviewer_rules(policy_content: &str) -> Vec<ReviewerRule> {
    let mut rules = Vec::new();

    for cap in RULE_PATTERN.captures_iter(policy_content) {
        let path_pattern = cap
            .get(1)
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();
        let reviewers_str = cap.get(2).map(|m| m.as_str()).unwrap_or_default();

        // Parse reviewer list: "alice", "bob" -> vec!["alice", "bob"]
        let reviewers: Vec<String> = REVIEWER_PATTERN
            .captures_iter(reviewers_str)
            .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
            .collect();

        // Allow empty path_pattern (matches all files)
        if !reviewers.is_empty() {
            rules.push(ReviewerRule {
                path_pattern,
                reviewers,
            });
        }
    }

    rules
}

/// Find matching reviewers for a given file path
///
/// # Arguments
/// * `rules` - List of reviewer rules
/// * `file_path` - The file path to match against
///
/// # Returns
/// A list of required reviewer usernames
pub fn find_reviewers_for_path(rules: &[ReviewerRule], file_path: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut reviewers = Vec::new();

    // Normalize path: remove leading slash for consistent matching
    let normalized_path = file_path.trim_start_matches('/');

    for rule in rules {
        let normalized_pattern = rule.path_pattern.trim_start_matches('/');

        // Match if path starts with pattern, or pattern is empty (matches all)
        if normalized_pattern.is_empty() || normalized_path.starts_with(normalized_pattern) {
            for reviewer in &rule.reviewers {
                if seen.insert(reviewer.clone()) {
                    reviewers.push(reviewer.clone());
                }
            }
        }
    }

    reviewers
}

/// Aggregate reviewers from multiple policy files with override semantics
///
/// For the same path_pattern, rules from child directories override parent directories.
/// Different path_patterns are merged (accumulated).
///
/// # Arguments
/// * `policy_contents` - List of (policy_path, content) tuples, from root to leaf
/// * `target_path` - The file path to find reviewers for
///
/// # Returns
/// Combined list of required reviewers after applying override rules
pub fn aggregate_reviewers<P: AsRef<str>>(
    policy_contents: &[(P, String)],
    target_path: &str,
) -> Vec<String> {
    // Use HashMap to store reviewers by path_pattern
    // Later entries (child directories) override earlier entries (parent directories)
    let mut pattern_reviewers: HashMap<String, Vec<String>> = HashMap::new();

    let normalized_target = target_path.trim_start_matches('/');

    for (_, content) in policy_contents {
        let rules = parse_reviewer_rules(content);

        for rule in rules {
            let normalized_pattern = rule.path_pattern.trim_start_matches('/');

            // Check if this rule matches the target path
            let matches =
                normalized_pattern.is_empty() || normalized_target.starts_with(normalized_pattern);

            if matches {
                // Override: same path_pattern from child replaces parent
                pattern_reviewers.insert(rule.path_pattern.clone(), rule.reviewers);
            }
        }
    }

    // Merge all reviewers from different patterns (deduplicated)
    let mut seen = HashSet::new();
    let mut all_reviewers = Vec::new();
    for reviewers in pattern_reviewers.values() {
        for reviewer in reviewers {
            if seen.insert(reviewer.clone()) {
                all_reviewers.push(reviewer.clone());
            }
        }
    }

    all_reviewers
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Comprehensive test for reviewer_parser module
    /// Run with: cargo test -p saturn test_reviewer_parser_comprehensive -- --nocapture
    #[test]
    fn test_reviewer_parser_comprehensive() {
        println!("\n{}", "=".repeat(70));
        println!("Cedar Reviewer Parser - Comprehensive Test");
        println!("{}", "=".repeat(70));

        // ========== 1. Policy Parsing ==========
        println!("\n[1] Policy Parsing");
        println!("{}", "-".repeat(50));

        let policy = r#"
permit(action == "code:review", principal, resource)
    when { resource.path.startsWith("service_a/") }
    to ["alice", "bob"];

permit(action == "code:review", principal, resource)
    when { resource.path.startsWith("core/") }
    to ["charlie"];
        "#;

        println!("Input policy:\n{}", policy);
        let rules = parse_reviewer_rules(policy);
        println!("\nParsed rules:");
        for (i, rule) in rules.iter().enumerate() {
            println!(
                "  [{}] pattern='{}' reviewers={:?}",
                i, rule.path_pattern, rule.reviewers
            );
        }
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].path_pattern, "service_a/");
        assert_eq!(rules[0].reviewers, vec!["alice", "bob"]);
        assert_eq!(rules[1].path_pattern, "core/");
        assert_eq!(rules[1].reviewers, vec!["charlie"]);
        println!("✓ Policy parsing OK");

        // ========== 2. Path Matching ==========
        println!("\n[2] Path Matching");
        println!("{}", "-".repeat(50));

        let test_cases = vec![
            ("service_a/src/main.rs", vec!["alice", "bob"]),
            ("core/lib.rs", vec!["charlie"]),
            ("other/file.rs", vec![]),
        ];

        for (path, expected) in test_cases {
            let result = find_reviewers_for_path(&rules, path);
            println!("  Path '{}' -> {:?}", path, result);
            assert_eq!(
                result,
                expected.iter().map(|s| s.to_string()).collect::<Vec<_>>()
            );
        }
        println!("✓ Path matching OK");

        // ========== 3. Hierarchical Override (Same Pattern) ==========
        println!("\n[3] Hierarchical Override - Same Pattern (Child Wins)");
        println!("{}", "-".repeat(50));

        let policies = vec![
            (
                "/repo".to_string(),
                r#"permit(action == "code:review", principal, resource)
                    when { resource.path.startsWith("service_a/") }
                    to ["alice"];"#
                    .to_string(),
            ),
            (
                "/repo/service_a".to_string(),
                r#"permit(action == "code:review", principal, resource)
                    when { resource.path.startsWith("service_a/") }
                    to ["bob", "charlie"];"#
                    .to_string(),
            ),
        ];

        println!("  Root policy: service_a/ -> [alice]");
        println!("  Child policy: service_a/ -> [bob, charlie]");

        let reviewers = aggregate_reviewers(&policies, "service_a/src/main.rs");
        println!("  Result for 'service_a/src/main.rs': {:?}", reviewers);

        assert!(
            !reviewers.contains(&"alice".to_string()),
            "alice should be overridden"
        );
        assert!(reviewers.contains(&"bob".to_string()));
        assert!(reviewers.contains(&"charlie".to_string()));
        println!("✓ Same pattern override OK (child wins)");

        // ========== 4. Hierarchical Merge (Different Patterns) ==========
        println!("\n[4] Hierarchical Merge - Different Patterns");
        println!("{}", "-".repeat(50));

        let policies = vec![
            (
                "/repo".to_string(),
                r#"permit(action == "code:review", principal, resource)
                    when { resource.path.startsWith("") }
                    to ["root_reviewer"];"#
                    .to_string(),
            ),
            (
                "/repo/service_a".to_string(),
                r#"permit(action == "code:review", principal, resource)
                    when { resource.path.startsWith("service_a/") }
                    to ["alice"];"#
                    .to_string(),
            ),
        ];

        println!("  Root policy: '' (global) -> [root_reviewer]");
        println!("  Child policy: service_a/ -> [alice]");

        let reviewers = aggregate_reviewers(&policies, "service_a/src/main.rs");
        println!("  Result for 'service_a/src/main.rs': {:?}", reviewers);

        assert!(reviewers.contains(&"root_reviewer".to_string()));
        assert!(reviewers.contains(&"alice".to_string()));
        assert_eq!(reviewers.len(), 2);
        println!("✓ Different patterns merge OK");

        // ========== 5. Complex Hierarchy ==========
        println!("\n[5] Complex Hierarchy - Override + Merge");
        println!("{}", "-".repeat(50));

        let policies = vec![
            (
                "/repo".to_string(),
                r#"
                permit(action == "code:review", principal, resource)
                    when { resource.path.startsWith("") }
                    to ["global_owner"];
                permit(action == "code:review", principal, resource)
                    when { resource.path.startsWith("service_a/") }
                    to ["old_service_owner"];
                "#
                .to_string(),
            ),
            (
                "/repo/service_a".to_string(),
                r#"permit(action == "code:review", principal, resource)
                    when { resource.path.startsWith("service_a/") }
                    to ["new_service_owner"];"#
                    .to_string(),
            ),
        ];

        println!("  Root policy:");
        println!("    - '' (global) -> [global_owner]");
        println!("    - service_a/ -> [old_service_owner]");
        println!("  Child policy:");
        println!("    - service_a/ -> [new_service_owner]");

        let reviewers = aggregate_reviewers(&policies, "service_a/src/main.rs");
        println!("  Result for 'service_a/src/main.rs': {:?}", reviewers);

        assert!(reviewers.contains(&"global_owner".to_string()));
        assert!(
            !reviewers.contains(&"old_service_owner".to_string()),
            "old should be overridden"
        );
        assert!(reviewers.contains(&"new_service_owner".to_string()));
        assert_eq!(reviewers.len(), 2);
        println!("✓ Complex hierarchy OK");

        // ========== Summary ==========
        println!("\n{}", "=".repeat(70));
        println!("ALL TESTS PASSED!");
        println!("{}", "=".repeat(70));
        println!("\nTested features:");
        println!("  - Parse 'to []' syntax from Cedar policy");
        println!("  - Match file path to reviewer rules");
        println!("  - Hierarchical override: same pattern, child wins");
        println!("  - Hierarchical merge: different patterns accumulate");
        println!();
    }

    /// Test edge cases in policy parsing
    /// Run with: cargo test -p saturn test_policy_parsing_edge_cases -- --nocapture
    #[test]
    fn test_policy_parsing_edge_cases() {
        println!("\n{}", "=".repeat(70));
        println!("Policy Parsing Edge Cases");
        println!("{}", "=".repeat(70));

        // Test case 1: Empty policy file
        println!("\n[1] Empty policy file");
        let rules = parse_reviewer_rules("");
        assert!(rules.is_empty());
        println!("✓ Empty policy returns no rules");

        // Test case 2: Policy with no reviewers
        println!("\n[2] Policy with comments only");
        let policy = r#"
        // This is a comment
        /* Multi-line
           comment */
        "#;
        let rules = parse_reviewer_rules(policy);
        assert!(rules.is_empty());
        println!("✓ Comments-only policy returns no rules");

        // Test case 3: Policy with single reviewer
        println!("\n[3] Single reviewer");
        let policy = r#"
        permit(action == "code:review", principal, resource)
            when { resource.path.startsWith("src/") }
            to ["single_user"];
        "#;
        let rules = parse_reviewer_rules(policy);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].reviewers, vec!["single_user"]);
        println!("✓ Single reviewer parsed correctly");

        // Test case 4: Policy with many reviewers
        println!("\n[4] Many reviewers");
        let policy = r#"
        permit(action == "code:review", principal, resource)
            when { resource.path.startsWith("critical/") }
            to ["alice", "bob", "charlie", "david", "eve"];
        "#;
        let rules = parse_reviewer_rules(policy);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].reviewers.len(), 5);
        println!("✓ Many reviewers parsed correctly");

        // Test case 5: Multiple policies in one file
        println!("\n[5] Multiple policies in one file");
        let policy = r#"
        permit(action == "code:review", principal, resource)
            when { resource.path.startsWith("frontend/") }
            to ["frontend_lead"];
        
        permit(action == "code:review", principal, resource)
            when { resource.path.startsWith("backend/") }
            to ["backend_lead"];
        
        permit(action == "code:review", principal, resource)
            when { resource.path.startsWith("infra/") }
            to ["devops_lead", "sre_lead"];
        "#;
        let rules = parse_reviewer_rules(policy);
        assert_eq!(rules.len(), 3);
        println!("✓ Multiple policies parsed correctly");

        // Test case 6: Global policy (empty path pattern)
        println!("\n[6] Global policy (empty path)");
        let policy = r#"
        permit(action == "code:review", principal, resource)
            when { resource.path.startsWith("") }
            to ["global_owner"];
        "#;
        let rules = parse_reviewer_rules(policy);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].path_pattern, "");
        let reviewers = find_reviewers_for_path(&rules, "any/path/file.rs");
        assert_eq!(reviewers, vec!["global_owner"]);
        println!("✓ Global policy matches all paths");

        println!("\n{}", "=".repeat(70));
        println!("EDGE CASE TESTS PASSED!");
        println!("{}", "=".repeat(70));
    }

    /// Test the complete flow: policy loading -> parsing -> reviewer assignment
    /// Run with: cargo test -p saturn test_complete_reviewer_flow -- --nocapture
    #[test]
    fn test_complete_reviewer_flow() {
        println!("\n{}", "=".repeat(70));
        println!("Complete Reviewer Assignment Flow Test");
        println!("{}", "=".repeat(70));

        // Simulate a real-world scenario:
        // Repository structure:
        // /
        // ├── .cedar/policies.cedar (global owner)
        // ├── service_a/
        // │   ├── .cedar/policies.cedar (service_a owners)
        // │   └── src/
        // │       └── core/
        // │           └── .cedar/policies.cedar (core module owners)
        // └── service_b/
        //     └── .cedar/policies.cedar (service_b owners)

        println!("\n[Scenario] Multi-level policy hierarchy");
        println!("  Repository structure:");
        println!("  /");
        println!("  ├── .cedar/policies.cedar -> [global_owner]");
        println!("  ├── service_a/");
        println!("  │   ├── .cedar/policies.cedar -> [alice, bob]");
        println!("  │   └── src/core/");
        println!("  │       └── .cedar/policies.cedar -> [core_expert]");
        println!("  └── service_b/");
        println!("      └── .cedar/policies.cedar -> [charlie]");

        // Simulate policy files content
        let root_policy = r#"
        permit(action == "code:review", principal, resource)
            when { resource.path.startsWith("") }
            to ["global_owner"];
        "#
        .to_string();

        let service_a_policy = r#"
        permit(action == "code:review", principal, resource)
            when { resource.path.startsWith("service_a/") }
            to ["alice", "bob"];
        "#
        .to_string();

        let core_policy = r#"
        permit(action == "code:review", principal, resource)
            when { resource.path.startsWith("service_a/src/core/") }
            to ["core_expert"];
        "#
        .to_string();

        let service_b_policy = r#"
        permit(action == "code:review", principal, resource)
            when { resource.path.startsWith("service_b/") }
            to ["charlie"];
        "#
        .to_string();

        // Test case 1: CL in service_a/src/core/
        println!("\n[1] CL path: service_a/src/core/main.rs");
        let policies = vec![
            ("/.cedar/policies.cedar".to_string(), root_policy.clone()),
            (
                "service_a/.cedar/policies.cedar".to_string(),
                service_a_policy.clone(),
            ),
            (
                "service_a/src/core/.cedar/policies.cedar".to_string(),
                core_policy.clone(),
            ),
        ];
        let reviewers = aggregate_reviewers(&policies, "service_a/src/core/main.rs");
        println!("  Expected: global_owner + alice + bob + core_expert");
        println!("  Actual: {:?}", reviewers);
        assert!(reviewers.contains(&"global_owner".to_string()));
        assert!(reviewers.contains(&"alice".to_string()));
        assert!(reviewers.contains(&"bob".to_string()));
        assert!(reviewers.contains(&"core_expert".to_string()));
        println!("✓ All hierarchical reviewers included");

        // Test case 2: CL in service_b/
        println!("\n[2] CL path: service_b/handler.rs");
        let policies = vec![
            ("/.cedar/policies.cedar".to_string(), root_policy.clone()),
            (
                "service_b/.cedar/policies.cedar".to_string(),
                service_b_policy.clone(),
            ),
        ];
        let reviewers = aggregate_reviewers(&policies, "service_b/handler.rs");
        println!("  Expected: global_owner + charlie");
        println!("  Actual: {:?}", reviewers);
        assert!(reviewers.contains(&"global_owner".to_string()));
        assert!(reviewers.contains(&"charlie".to_string()));
        assert!(!reviewers.contains(&"alice".to_string())); // service_a owner should not be included
        println!("✓ Only relevant reviewers included");

        // Test case 3: CL in root (no service-specific policy)
        println!("\n[3] CL path: README.md (root level)");
        let policies = vec![("/.cedar/policies.cedar".to_string(), root_policy.clone())];
        let reviewers = aggregate_reviewers(&policies, "README.md");
        println!("  Expected: global_owner only");
        println!("  Actual: {:?}", reviewers);
        assert_eq!(reviewers, vec!["global_owner"]);
        println!("✓ Root-level CL gets global owner only");

        println!("\n{}", "=".repeat(70));
        println!("COMPLETE FLOW TESTS PASSED!");
        println!("{}", "=".repeat(70));
    }
}
