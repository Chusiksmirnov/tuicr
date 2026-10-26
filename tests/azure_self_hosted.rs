#![cfg(unix)]

use std::path::Path;
use std::process::Command;
use tuicr::forge::azure::AzureDevOpsBackend;
use tuicr::forge::traits::{ForgeBackend, ForgeRepository, PullRequestDetails};

fn git(root: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn azure_missing_commits_do_not_trigger_fetch() {
    const CHILD: &str = "TUICR_TEST_NO_FETCH_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let root = std::env::var_os("TUICR_TEST_REPO").unwrap();
        let repository = ForgeRepository::azure("dev.azure.com", "org/project", "repo");
        let backend = AzureDevOpsBackend::new(Some(repository.clone()))
            .with_local_checkout(Some(root.into()));
        let pr = PullRequestDetails {
            repository,
            number: 1,
            title: "test".into(),
            url: "https://dev.azure.com/org/project/_git/repo/pullrequest/1".into(),
            state: "OPEN".into(),
            is_draft: false,
            author: None,
            head_ref_name: "feature".into(),
            base_ref_name: "main".into(),
            head_sha: "1".repeat(40),
            base_sha: "2".repeat(40),
            body: String::new(),
            updated_at: None,
            closed: false,
            merged_at: None,
            diff_start_sha: None,
        };
        assert!(backend.get_pull_request_diff(&pr).is_err());
        assert!(
            backend
                .get_pull_request_commit_range_diff(&pr, &pr.base_sha, &pr.head_sha)
                .is_err()
        );
        assert_eq!(backend.resolve_diff_base_sha(&pr), None);
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("repo");
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    git(&root, &["init", "--quiet"]);
    git(
        &root,
        &[
            "remote",
            "add",
            "origin",
            "https://dev.azure.com/org/project/_git/repo",
        ],
    );
    let real_git = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .unwrap();
    assert!(real_git.status.success());
    let real_git = String::from_utf8(real_git.stdout).unwrap();
    let trace = dir.path().join("git.log");
    let wrapper = bin.join("git");
    std::fs::write(
        &wrapper,
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$TUICR_TEST_GIT_TRACE"
for arg in "$@"; do
    if [ "$arg" = fetch ]; then exit 1; fi
done
exec "$TUICR_TEST_REAL_GIT" "$@"
"#,
    )
    .unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut paths = vec![bin];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
    let out = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "azure_missing_commits_do_not_trigger_fetch",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .env("TUICR_TEST_REPO", &root)
        .env("TUICR_TEST_GIT_TRACE", &trace)
        .env("TUICR_TEST_REAL_GIT", real_git.trim())
        .env("PATH", std::env::join_paths(paths).unwrap())
        .env_remove("AZURE_DEVOPS_SERVER_URL")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let commands = std::fs::read_to_string(trace).unwrap();
    assert!(
        commands.contains("cat-file"),
        "fixture must exercise local object checks: {commands}"
    );
    assert!(
        !commands
            .lines()
            .any(|line| line.split_whitespace().any(|arg| arg == "fetch")),
        "must not fetch: {commands}"
    );
}

#[test]
fn self_hosted_azure_routes_through_the_shared_forge_parser() {
    const CHILD: &str = "TUICR_TEST_AZURE_ROUTING_CHILD";
    if let Some(configured) = std::env::var_os(CHILD) {
        use tuicr::forge::parse_any_remote_url;
        let url = "https://tfs.example.test/tfs/Collection/Project/_git/repo";
        let expected = ForgeRepository::azure("tfs.example.test", "tfs/Collection/Project", "repo");
        let parsed = parse_any_remote_url(url).unwrap();
        if configured == "yes" {
            assert_eq!(parsed, expected);
        } else {
            assert_ne!(
                parsed.kind,
                ForgeRepository::azure("dev.azure.com", "o/p", "r").kind
            );
        }
        assert_eq!(
            parse_any_remote_url("https://dev.azure.com/org/project/_git/repo"),
            Some(ForgeRepository::azure(
                "dev.azure.com",
                "org/project",
                "repo"
            ))
        );
        assert_eq!(
            parse_any_remote_url("https://github.com/owner/repo"),
            Some(ForgeRepository::github("github.com", "owner", "repo"))
        );
        return;
    }
    for configured in ["yes", "no", "other"] {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "self_hosted_azure_routes_through_the_shared_forge_parser",
                "--nocapture",
            ])
            .env(CHILD, configured)
            .env_remove("AZURE_DEVOPS_SERVER_URL");
        if configured == "yes" {
            command.env(
                "AZURE_DEVOPS_SERVER_URL",
                "https://tfs.example.test/tfs/Collection",
            );
        }
        if configured == "other" {
            command.env("AZURE_DEVOPS_SERVER_URL", "https://different.example.test");
        }
        let out = command.output().unwrap();
        assert!(
            out.status.success(),
            "{configured}: {}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
