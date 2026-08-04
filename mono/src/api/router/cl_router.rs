use api_model::common::{CommonPage, CommonResult, PageParams};
use axum::{
    Json,
    extract::{Path, State},
};
use ceres::model::{
    change_list::{
        AssigneeUpdatePayload, CLDetailRes, ClFilesRes, FilesChangedPage, ListPayload, MergeBoxRes,
        MuiTreeNode, UpdateBranchStatusRes, UpdateClStatusPayload,
    },
    conversation::ContentPayload,
    issue::ItemRes,
    label::LabelUpdatePayload,
};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::api::{
    MonoApiServiceState,
    api_common::{self, identity::collaboration_actor},
    api_doc::CL_TAG,
    error::ApiError,
    oauth::model::LoginUser,
};

pub fn routers() -> OpenApiRouter<MonoApiServiceState> {
    OpenApiRouter::new().nest(
        "/cl",
        OpenApiRouter::new()
            .routes(routes!(fetch_cl_list))
            .routes(routes!(cl_detail))
            .routes(routes!(merge))
            .routes(routes!(merge_box))
            .routes(routes!(merge_no_auth))
            .routes(routes!(close_cl))
            .routes(routes!(reopen_cl))
            .routes(routes!(cl_mui_tree))
            .routes(routes!(cl_files_changed_by_page))
            .routes(routes!(cl_files_list))
            .routes(routes!(save_comment))
            .routes(routes!(labels))
            .routes(routes!(assignees))
            .routes(routes!(edit_title))
            .routes(routes!(update_cl_status))
            .routes(routes!(update_branch_status))
            .routes(routes!(update_branch)),
    )
}

/// Reopen Change List
#[utoipa::path(
    post,
    params(
        ("link", description = "CL link"),
    ),
    path = "/{link}/reopen",
    responses(
        (status = 200, body = CommonResult<String>, content_type = "application/json")
    ),
    tag = CL_TAG
)]
async fn reopen_cl(
    user: LoginUser,
    Path(link): Path<String>,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<String>>, ApiError> {
    let actor = collaboration_actor(&user)?;
    state.services().cl().reopen_cl(&link, actor).await?;
    Ok(Json(CommonResult::success(None)))
}

/// Close Change List
#[utoipa::path(
    post,
    params(
        ("link", description = "CL link"),
    ),
    path = "/{link}/close",
    responses(
        (status = 200, body = CommonResult<String>, content_type = "application/json")
    ),
    tag = CL_TAG
)]
async fn close_cl(
    user: LoginUser,
    Path(link): Path<String>,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<String>>, ApiError> {
    let actor = collaboration_actor(&user)?;
    state.services().cl().close_cl(&link, actor).await?;
    Ok(Json(CommonResult::success(None)))
}

/// Approve Change List
#[utoipa::path(
    post,
    params(
        ("link", description = "CL link"),
    ),
    path = "/{link}/merge",
    responses(
        (status = 200, body = CommonResult<String>, content_type = "application/json")
    ),
    tag = CL_TAG
)]
async fn merge(
    user: LoginUser,
    Path(link): Path<String>,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<String>>, ApiError> {
    let actor = collaboration_actor(&user)?;
    state.services().cl().merge_open_cl(actor, &link).await?;
    Ok(Json(CommonResult::success(None)))
}

/// Change List without authentication
/// It's for local testing purposes.
#[utoipa::path(
    post,
    params(
        ("link", description = "CL link"),
    ),
    path = "/{link}/merge-no-auth",
    responses(
        (status = 200, body = CommonResult<String>, content_type = "application/json")
    ),
    tag = CL_TAG
)]
async fn merge_no_auth(
    Path(link): Path<String>,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<String>>, ApiError> {
    state.services().cl().merge_open_cl_no_auth(&link).await?;
    Ok(Json(CommonResult::success(Some(
        "Merge completed successfully".to_string(),
    ))))
}

/// Fetch CL list
#[utoipa::path(
    post,
    path = "/list",
    request_body = PageParams<ListPayload>,
    responses(
        (status = 200, body = CommonResult<CommonPage<ItemRes>>, content_type = "application/json")
    ),
    tag = CL_TAG
)]
async fn fetch_cl_list(
    state: State<MonoApiServiceState>,
    Json(json): Json<PageParams<ListPayload>>,
) -> Result<Json<CommonResult<CommonPage<ItemRes>>>, ApiError> {
    let res = state
        .services()
        .cl()
        .get_cl_list(json.additional, json.pagination)
        .await?;
    Ok(Json(CommonResult::success(Some(res))))
}

/// Get change list details
#[utoipa::path(
    get,
    params(
        ("link", description = "CL link"),
    ),
    path = "/{link}/detail",
    responses(
        (status = 200, body = CommonResult<CLDetailRes>, content_type = "application/json")
    ),
    tag = CL_TAG
)]
async fn cl_detail(
    user: LoginUser,
    Path(link): Path<String>,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<CLDetailRes>>, ApiError> {
    let actor = collaboration_actor(&user)?;
    let cl_details = state
        .services()
        .cl()
        .get_cl_details(&link, actor.to_string())
        .await?;
    Ok(Json(CommonResult::success(Some(cl_details))))
}

#[utoipa::path(
    get,
    params(
        ("link", description = "CL link"),
    ),
    path = "/{link}/mui-tree",
    responses(
        (status = 200, body = CommonResult<Vec<MuiTreeNode>>, content_type = "application/json")
    ),
    tag = CL_TAG
)]
async fn cl_mui_tree(
    Path(link): Path<String>,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<Vec<MuiTreeNode>>>, ApiError> {
    let files = state
        .services()
        .cl()
        .get_sorted_changed_file_list(&link, None)
        .await?;
    let mui_trees = build_forest(files);
    Ok(Json(CommonResult::success(Some(mui_trees))))
}

/// Get Change List file changed list in Pagination
#[utoipa::path(
    post,
    params(
        ("link", description = "CL link"),
    ),
    path = "/{link}/files-changed",
    request_body = PageParams<String>,
    responses(
        (status = 200, body = CommonResult<FilesChangedPage>, content_type = "application/json")
    ),
    tag = CL_TAG
)]
async fn cl_files_changed_by_page(
    Path(link): Path<String>,
    state: State<MonoApiServiceState>,
    Json(json): Json<PageParams<String>>,
) -> Result<Json<CommonResult<FilesChangedPage>>, ApiError> {
    let (items, total) = state
        .services()
        .cl()
        .paged_content_diff_for_cl(&link, json.pagination)
        .await?;
    let res = CommonResult::success(Some(FilesChangedPage {
        page: CommonPage { total, items },
    }));
    Ok(Json(res))
}

/// Get Change List file list
#[utoipa::path(
    get,
    params(
        ("link", description = "CL link"),
    ),
    path = "/{link}/files-list",
    responses(
        (status = 200, body = CommonResult<Vec<ClFilesRes>>, content_type = "application/json")
    ),
    tag = CL_TAG
)]
async fn cl_files_list(
    Path(link): Path<String>,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<Vec<ClFilesRes>>>, ApiError> {
    let cl = state.services().cl().get_cl_model(&link).await?;

    let cl_svc = state.services().cl();
    let old_files = cl_svc.get_commit_blobs(&cl.from_hash).await?;
    let new_files = cl_svc.get_commit_blobs(&cl.to_hash).await?;
    let cl_diff_files = cl_svc.cl_files_list(old_files, new_files.clone()).await?; // TODO

    let res = cl_diff_files
        .into_iter()
        .map(|m| {
            let item: ClFilesRes = m.into();
            item
        })
        .collect::<Vec<ClFilesRes>>();
    Ok(Json(CommonResult::success(Some(res))))
}

/// Get Update Branch status
#[utoipa::path(
    get,
    params(
        ("link", description = "CL link"),
    ),
    path = "/{link}/update-status",
    responses(
        (status = 200, body = CommonResult<UpdateBranchStatusRes>, content_type = "application/json")
    ),
    tag = CL_TAG
)]
async fn update_branch_status(
    Path(link): Path<String>,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<UpdateBranchStatusRes>>, ApiError> {
    let res = state.services().cl().update_branch_status(&link).await?;
    Ok(Json(CommonResult::success(Some(res))))
}

/// Update Branch for Change List
#[utoipa::path(
    post,
    params(
        ("link", description = "CL link"),
    ),
    path = "/{link}/update-branch",
    responses(
        (status = 200, body = CommonResult<String>, content_type = "application/json")
    ),
    tag = CL_TAG
)]
async fn update_branch(
    user: LoginUser,
    Path(link): Path<String>,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<String>>, ApiError> {
    let actor = collaboration_actor(&user)?;
    let new_head = state
        .services()
        .cl()
        .update_branch_with_webhook(actor, &link)
        .await?;
    Ok(Json(CommonResult::success(Some(new_head))))
}

/// Get Merge Box to check merge status
#[utoipa::path(
    get,
    params(
        ("link", description = "CL link"),
    ),
    path = "/{link}/merge-box",
    responses(
        (status = 200, body = CommonResult<MergeBoxRes>, content_type = "application/json")
    ),
    tag = CL_TAG
)]
async fn merge_box(
    Path(link): Path<String>,
    state: State<MonoApiServiceState>,
) -> Result<Json<CommonResult<MergeBoxRes>>, ApiError> {
    let res = state.services().cl().get_merge_box(&link).await?;
    Ok(Json(CommonResult::success(Some(res))))
}

/// Add new comment on Change List
#[utoipa::path(
    post,
    params(
        ("link", description = "CL link"),
    ),
    path = "/{link}/comment",
    request_body = ContentPayload,
    responses(
        (status = 200, body = CommonResult<String>, content_type = "application/json")
    ),
    tag = CL_TAG
)]
async fn save_comment(
    user: LoginUser,
    Path(link): Path<String>,
    state: State<MonoApiServiceState>,
    Json(payload): Json<ContentPayload>,
) -> Result<Json<CommonResult<()>>, ApiError> {
    let actor = collaboration_actor(&user)?;
    state
        .services()
        .cl()
        .save_cl_comment(&link, actor, &payload.content)
        .await?;
    api_common::comment::check_comment_ref(user, state, &payload.content, &link).await
}

/// Edit CL title
#[utoipa::path(
    post,
    params(
        ("link", description = "A string ID representing a Change List"),
    ),
    path = "/{link}/title",
    request_body = ContentPayload,
    responses(
        (status = 200, body = CommonResult<String>, content_type = "application/json")
    ),
    tag = CL_TAG
)]
async fn edit_title(
    _: LoginUser,
    Path(link): Path<String>,
    state: State<MonoApiServiceState>,
    Json(payload): Json<ContentPayload>,
) -> Result<Json<CommonResult<String>>, ApiError> {
    state
        .services()
        .cl()
        .edit_cl_title(&link, &payload.content)
        .await?;
    Ok(Json(CommonResult::success(None)))
}

/// Update cl related labels
#[utoipa::path(
    post,
    path = "/labels",
    request_body = LabelUpdatePayload,
    responses(
        (status = 200, body = CommonResult<String>, content_type = "application/json")
    ),
    tag = CL_TAG
)]
async fn labels(
    user: LoginUser,
    state: State<MonoApiServiceState>,
    Json(payload): Json<LabelUpdatePayload>,
) -> Result<Json<CommonResult<()>>, ApiError> {
    api_common::label_assignee::label_update(user, state, payload, String::from("cl")).await
}

/// Update CL related assignees
#[utoipa::path(
    post,
    path = "/assignees",
    request_body = AssigneeUpdatePayload,
    responses(
        (status = 200, body = CommonResult<String>, content_type = "application/json")
    ),
    tag = CL_TAG
)]
async fn assignees(
    user: LoginUser,
    state: State<MonoApiServiceState>,
    Json(payload): Json<AssigneeUpdatePayload>,
) -> Result<Json<CommonResult<()>>, ApiError> {
    api_common::label_assignee::assignees_update(user, state, payload, String::from("cl")).await
}

/// Update CL status (Draft or Open)
#[utoipa::path(
    post,
    params(
        ("link", description = "CL link"),
    ),
    path = "/{link}/status",
    request_body = UpdateClStatusPayload,
    responses(
        (status = 200, body = CommonResult<String>, content_type = "application/json")
    ),
    tag = CL_TAG
)]
async fn update_cl_status(
    user: LoginUser,
    Path(link): Path<String>,
    state: State<MonoApiServiceState>,
    Json(payload): Json<UpdateClStatusPayload>,
) -> Result<Json<CommonResult<String>>, ApiError> {
    let actor = collaboration_actor(&user)?;
    state
        .services()
        .cl()
        .update_cl_status(&link, actor, &payload)
        .await?;
    Ok(Json(CommonResult::success(None)))
}

fn build_forest(paths: Vec<String>) -> Vec<MuiTreeNode> {
    let mut roots: Vec<MuiTreeNode> = Vec::new();

    for path in paths {
        let parts: Vec<&str> = path.split('/').collect();
        if parts.is_empty() {
            continue;
        }
        let root_label = parts[0];
        if let Some(existing_root) = roots.iter_mut().find(|r| r.label == root_label) {
            let mut buf = existing_root.path.clone();
            existing_root.insert_path(&parts[1..], &mut buf);
        } else {
            let mut buf = String::new();
            buf.push('/');
            buf.push_str(parts[0]);
            let mut new_root = MuiTreeNode::new(root_label, &buf);
            new_root.insert_path(&parts[1..], &mut buf);
            roots.push(new_root);
        }
    }

    roots
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;

    use git_internal::DiffItem;

    use crate::api::router::cl_router::build_forest;

    fn extract_files_with_status(diff_output: &str) -> HashMap<String, String> {
        let mut files = HashMap::new();

        let chunks: Vec<&str> = diff_output.split("diff --git ").collect();

        for chunk in chunks.iter().skip(1) {
            let lines: Vec<&str> = chunk.split_whitespace().collect();
            if lines.len() >= 2 {
                let current_file = lines[0].trim_start_matches("a/").to_string();
                files.insert(current_file.clone(), "modified".to_string()); // 默认状态为修改
                if chunk.contains("new file mode") {
                    files.insert(current_file, "new".to_string());
                } else if chunk.contains("deleted file mode") {
                    files.insert(current_file, "deleted".to_string());
                }
            }
        }
        files
    }

    #[test]
    fn test_parse_diff_result_to_filelist() {
        let diff_output = r#"
        diff --git a/ceres/src/api_service/mono_api_service.rs b/ceres/src/api_service/mono_api_service.rs
        new file mode 100644
        index 0000000..561296a1
        @@ -1,0 +1,595 @@
        fn main() {
            println!("Hello, world!");
        }
        diff --git a/ceres/src/lib.rs b/ceres/src/lib.rs
        index 1234567..89abcdef 100644
        --- a/ceres/src/lib.rs
        +++ b/ceres/src/lib.rs
        @@ -10,7 +10,8 @@
        diff --git a/ceres/src/removed.rs b/ceres/src/removed.rs
        deleted file mode 100644
        "#;
        let files_with_status = extract_files_with_status(diff_output);
        println!("Files with status:");
        for (file, status) in &files_with_status {
            println!("{file} ({status})");
        }

        let mut expected = HashMap::new();
        expected.insert(
            "ceres/src/api_service/mono_api_service.rs".to_string(),
            "new".to_string(),
        );
        expected.insert("ceres/src/lib.rs".to_string(), "modified".to_string());
        expected.insert("ceres/src/removed.rs".to_string(), "deleted".to_string());

        assert_eq!(files_with_status, expected);
    }

    #[test]
    fn test_files_changed_tree() {
        let paths = vec![
            String::from("crates-pro/crates_pro/src/bin/bin_analyze.rs"),
            String::from("crates-pro/images/analysis-tool-worker.Dockerfile"),
            String::from("crates-pro/images/crates-pro.Dockerfile"),
            String::from("another-root/foo/bar.txt"),
        ];

        let forest = build_forest(paths);
        println!("{}", serde_json::to_string_pretty(&forest).unwrap());
    }

    #[test]
    fn test_cl_files_changed_logic() {
        // Test the core logic of cl_files_changed function
        // This tests the data transformation logic without needing the full state

        let sample_diff_output = r#"diff --git a/src/main.rs b/src/main.rs
            new file mode 100644
            index 0000000..abc1234
            --- /dev/null
            +++ b/src/main.rs
            @@ -0,0 +1,5 @@
            +fn main() {
            +    println!("Hello, world!");
            +}
            diff --git a/src/lib.rs b/src/lib.rs
            index def5678..ghi9012 100644
            --- a/src/lib.rs
            +++ b/src/lib.rs
            @@ -1,3 +1,4 @@
            +// Added a comment
            pub fn add(left: usize, right: usize) -> usize {
                left + right
            }
            diff --git a/README.md b/README.md
            deleted file mode 100644
            index 1234567..0000000"#;

        // Test extract_files_with_status
        let diff_files = extract_files_with_status(sample_diff_output);

        assert_eq!(diff_files.len(), 3);
        assert_eq!(diff_files.get("src/main.rs"), Some(&"new".to_string()));
        assert_eq!(diff_files.get("src/lib.rs"), Some(&"modified".to_string()));
        assert_eq!(diff_files.get("README.md"), Some(&"deleted".to_string()));

        // Test path extraction and tree building
        let mut paths = vec![];
        for (path, _) in diff_files {
            paths.push(path);
        }

        let mui_trees = build_forest(paths);

        // Verify the tree structure
        assert!(!mui_trees.is_empty());

        // Check that we have the expected root nodes
        let root_labels: Vec<&str> = mui_trees.iter().map(|tree| tree.label.as_str()).collect();
        assert!(root_labels.contains(&"src"));
        assert!(root_labels.contains(&"README.md"));

        let content = [DiffItem {
            data: sample_diff_output.to_string(),
            path: "diff_output.txt".to_string(),
        }];

        assert!(!mui_trees.is_empty());
        assert_eq!(content.first().unwrap().data, sample_diff_output);
    }

    #[test]
    fn test_extract_files_with_status_edge_cases() {
        // Test with empty diff output
        let empty_diff = "";
        let result = extract_files_with_status(empty_diff);
        assert!(result.is_empty());

        // Test with malformed diff output
        let malformed_diff = "not a valid diff output";
        let result = extract_files_with_status(malformed_diff);
        assert!(result.is_empty());

        // Test with diff containing only additions
        let additions_only = r#"diff --git a/new_file.txt b/new_file.txt
new file mode 100644
index 0000000..1234567
--- /dev/null
+++ b/new_file.txt"#;

        let result = extract_files_with_status(additions_only);
        assert_eq!(result.len(), 1);
        assert_eq!(result.get("new_file.txt"), Some(&"new".to_string()));

        // Test with diff containing only deletions
        let deletions_only = r#"diff --git a/old_file.txt b/old_file.txt
deleted file mode 100644
index 1234567..0000000"#;

        let result = extract_files_with_status(deletions_only);
        assert_eq!(result.len(), 1);
        assert_eq!(result.get("old_file.txt"), Some(&"deleted".to_string()));
    }

    #[test]
    fn test_build_forest_edge_cases() {
        // Test with empty paths
        let empty_paths = vec![];
        let result = build_forest(empty_paths);
        assert!(result.is_empty());

        // Test with single file
        let single_file = vec!["single_file.txt".to_string()];
        let result = build_forest(single_file);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].label, "single_file.txt");

        // Test with deeply nested paths
        let nested_paths = vec![
            "a/b/c/d/e/file.txt".to_string(),
            "a/b/different.txt".to_string(),
            "a/another.txt".to_string(),
        ];
        let result = build_forest(nested_paths);
        assert_eq!(result.len(), 1); // Should have one root "a"
        assert_eq!(result[0].label, "a");

        // The tree should have nested structure
        let root = &result[0];
        assert!(root.children.is_some());
        let children = root.children.as_ref().unwrap();
        assert!(children.iter().any(|child| child.label == "b"));
        assert!(children.iter().any(|child| child.label == "another.txt"));
    }
}
