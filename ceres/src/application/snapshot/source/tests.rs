use std::sync::Arc;

use callisto::{
    git_commit, git_repo, git_tag, git_tree, import_refs, sea_orm_active_enums::RefTypeEnum,
};
use git_internal::internal::{
    metadata::EntryMeta,
    object::{
        blob::Blob,
        commit::Commit,
        tree::{Tree, TreeItem, TreeItemMode},
    },
};
use jupiter::{
    storage::base_storage::{BaseStorage, StorageConnector},
    utils::converter::IntoGitModel,
};
use sea_orm::{ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter, sea_query::Expr};
use tempfile::TempDir;

use super::*;

#[tokio::test]
async fn corrupt_or_missing_root_is_an_error_not_an_empty_directory() {
    let (_dir, storage) = fixture().await;
    let (a, _) = commit(&storage, 101, "A").await;
    let source = resolve_import_commit(&storage, 101, Some(&a.id.to_string()))
        .await
        .unwrap();
    git_tree::Entity::update_many()
        .col_expr(git_tree::Column::SubTrees, Expr::value(Vec::<u8>::new()))
        .filter(git_tree::Column::RepoId.eq(101))
        .exec(storage.get_connection())
        .await
        .unwrap();
    assert!(matches!(
        read_import_root(&storage, &source).await,
        Err(MegaError::Unavailable(_))
    ));
    git_tree::Entity::delete_many()
        .filter(git_tree::Column::RepoId.eq(101))
        .exec(storage.get_connection())
        .await
        .unwrap();
    assert!(matches!(
        read_import_root(&storage, &source).await,
        Err(MegaError::NotFound(_))
    ));
}

#[tokio::test]
async fn annotated_tag_depth_is_bounded() {
    let (_dir, storage) = fixture().await;
    let (a, _) = commit(&storage, 101, "A").await;
    let mut target = a.id.to_string();
    for n in 1..=33 {
        let id = format!("{n:040x}");
        tag(
            &storage,
            &id,
            &target,
            if n == 1 { "commit" } else { "tag" },
        )
        .await;
        target = id;
        if n == 32 {
            reference(&storage, 101, "refs/tags/allowed", &target, false).await;
        }
    }
    reference(&storage, 101, "refs/tags/too-deep", &target, false).await;
    assert_eq!(
        resolve_import_commit(&storage, 101, Some("allowed"))
            .await
            .unwrap()
            .commit_oid,
        a.id.to_string()
    );
    assert!(matches!(
        resolve_import_commit(&storage, 101, Some("too-deep")).await,
        Err(MegaError::BadRequest(_))
    ));
}

async fn fixture() -> (TempDir, GitDbStorage) {
    let dir = TempDir::new().unwrap();
    let connection = jupiter::tests::test_db_connection(dir.path()).await;
    // Do not depend on feature unification to migrate this test database.
    jupiter_migrate::apply_migrations(&connection, true)
        .await
        .unwrap();
    let storage = GitDbStorage {
        base: BaseStorage::new(Arc::new(connection)),
    };
    let now = chrono::Utc::now().naive_utc();
    for id in [101, 102] {
        storage
            .save_git_repo(git_repo::Model {
                id,
                repo_path: format!("/third-party/r{id}"),
                repo_name: format!("r{id}"),
                created_at: now,
                updated_at: now,
            })
            .await
            .unwrap();
    }
    (dir, storage)
}

async fn commit(storage: &GitDbStorage, repo_id: i64, content: &str) -> (Commit, Tree) {
    let tree = Tree::from_tree_items(vec![TreeItem {
        mode: TreeItemMode::Blob,
        id: Blob::from_content(content).id,
        name: "file.txt".into(),
    }])
    .unwrap();
    let commit = Commit::from_tree_id(tree.id, vec![], content);
    let mut tree_model = tree.clone().into_git_model(EntryMeta::new());
    tree_model.repo_id = repo_id;
    git_tree::Entity::insert(tree_model.into_active_model())
        .exec(storage.get_connection())
        .await
        .unwrap();
    let mut commit_model = commit.clone().into_git_model(EntryMeta::new());
    commit_model.repo_id = repo_id;
    git_commit::Entity::insert(commit_model.into_active_model())
        .exec(storage.get_connection())
        .await
        .unwrap();
    (commit, tree)
}

async fn reference(storage: &GitDbStorage, repo_id: i64, name: &str, oid: &str, default: bool) {
    let now = chrono::Utc::now().naive_utc();
    storage
        .save_ref(
            repo_id,
            import_refs::Model {
                id: common::utils::generate_id(),
                repo_id,
                ref_name: name.into(),
                ref_git_id: oid.into(),
                ref_type: if name.starts_with("refs/tags/") {
                    RefTypeEnum::Tag
                } else {
                    RefTypeEnum::Branch
                },
                default_branch: default,
                created_at: now,
                updated_at: now,
            },
        )
        .await
        .unwrap();
}

async fn tag(storage: &GitDbStorage, id: &str, target: &str, kind: &str) {
    storage
        .insert_tag(git_tag::Model {
            id: common::utils::generate_id(),
            repo_id: 101,
            tag_id: id.into(),
            object_id: target.into(),
            object_type: kind.into(),
            tag_name: id.into(),
            tagger: "fixture <fixture@example.test> 0 +0000".into(),
            message: "fixture".into(),
            created_at: chrono::Utc::now().naive_utc(),
            pack_id: String::new(),
            pack_offset: 0,
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn fixed_import_commit_and_resolved_ref_survive_branch_advance() {
    let (_dir, storage) = fixture().await;
    let (a, tree_a) = commit(&storage, 101, "A").await;
    let (b, tree_b) = commit(&storage, 101, "B").await;
    reference(&storage, 101, "refs/heads/main", &a.id.to_string(), true).await;
    let pinned = resolve_import_commit(&storage, 101, Some("refs/heads/main"))
        .await
        .unwrap();
    storage
        .update_ref(101, "refs/heads/main", &b.id.to_string())
        .await
        .unwrap();
    assert_eq!(
        read_import_root(&storage, &pinned).await.unwrap().id,
        tree_a.id
    );
    let explicit_a = resolve_import_commit(&storage, 101, Some(&a.id.to_string()))
        .await
        .unwrap();
    assert_eq!(
        read_import_root(&storage, &explicit_a).await.unwrap().id,
        tree_a.id
    );
    let latest = resolve_import_commit(&storage, 101, None).await.unwrap();
    assert_eq!(
        read_import_root(&storage, &latest).await.unwrap().id,
        tree_b.id
    );
}

#[tokio::test]
async fn missing_or_foreign_revision_never_falls_back_to_default() {
    let (_dir, storage) = fixture().await;
    let (a, _) = commit(&storage, 101, "A").await;
    let (foreign, _) = commit(&storage, 102, "foreign").await;
    reference(&storage, 101, "refs/heads/main", &a.id.to_string(), true).await;
    for selector in [
        "missing-tag".to_owned(),
        foreign.id.to_string(),
        "0".repeat(40),
    ] {
        assert!(matches!(
            resolve_import_commit(&storage, 101, Some(&selector)).await,
            Err(MegaError::NotFound(_))
        ));
    }
}

#[tokio::test]
async fn fully_qualified_branch_and_legacy_tag_are_unambiguous() {
    let (_dir, storage) = fixture().await;
    let (a, _) = commit(&storage, 101, "A").await;
    let (b, _) = commit(&storage, 101, "B").await;
    reference(&storage, 101, "refs/heads/release", &b.id.to_string(), true).await;
    reference(&storage, 101, "refs/tags/release", &a.id.to_string(), false).await;
    for selector in ["release", "refs/tags/release"] {
        assert_eq!(
            resolve_import_commit(&storage, 101, Some(selector))
                .await
                .unwrap()
                .commit_oid,
            a.id.to_string()
        );
    }
    assert_eq!(
        resolve_import_commit(&storage, 101, Some("refs/heads/release"))
            .await
            .unwrap()
            .commit_oid,
        b.id.to_string()
    );
}

#[tokio::test]
async fn annotated_tags_peel_only_from_a_tag_ref_and_never_follow_a_moved_tag() {
    let (_dir, storage) = fixture().await;
    let (a, _) = commit(&storage, 101, "A").await;
    let (b, _) = commit(&storage, 101, "B").await;
    let tag_id = "1".repeat(40);
    tag(&storage, &tag_id, &a.id.to_string(), "commit").await;
    reference(&storage, 101, "refs/tags/v1", &tag_id, false).await;
    let pinned = resolve_import_commit(&storage, 101, Some("refs/tags/v1"))
        .await
        .unwrap();
    assert_eq!(pinned.commit_oid, a.id.to_string());
    assert!(matches!(
        resolve_import_commit(&storage, 101, Some(&tag_id)).await,
        Err(MegaError::NotFound(_))
    ));
    storage
        .update_ref(101, "refs/tags/v1", &b.id.to_string())
        .await
        .unwrap();
    assert_eq!(
        resolve_import_commit(&storage, 101, Some("v1"))
            .await
            .unwrap()
            .commit_oid,
        b.id.to_string()
    );
    assert_eq!(pinned.commit_oid, a.id.to_string());
}

#[tokio::test]
async fn cyclic_and_non_commit_annotated_tags_fail_explicitly() {
    let (_dir, storage) = fixture().await;
    let first = "1".repeat(40);
    let second = "2".repeat(40);
    tag(&storage, &first, &second, "tag").await;
    tag(&storage, &second, &first, "tag").await;
    reference(&storage, 101, "refs/tags/cycle", &first, false).await;
    assert!(matches!(
        resolve_import_commit(&storage, 101, Some("cycle")).await,
        Err(MegaError::BadRequest(_))
    ));
    let third = "3".repeat(40);
    tag(&storage, &third, &"4".repeat(40), "tree").await;
    reference(&storage, 101, "refs/tags/tree", &third, false).await;
    assert!(matches!(
        resolve_import_commit(&storage, 101, Some("tree")).await,
        Err(MegaError::BadRequest(_))
    ));
}

#[tokio::test]
async fn absent_and_ambiguous_defaults_fail_without_panicking() {
    let (_dir, storage) = fixture().await;
    assert!(matches!(
        resolve_import_commit(&storage, 101, None).await,
        Err(MegaError::NotFound(_))
    ));
    let (a, _) = commit(&storage, 101, "A").await;
    reference(&storage, 101, "refs/heads/main", &a.id.to_string(), true).await;
    reference(&storage, 101, "refs/heads/other", &a.id.to_string(), true).await;
    assert!(matches!(
        resolve_import_commit(&storage, 101, None).await,
        Err(MegaError::Conflict(_))
    ));
    // A valid explicit commit does not depend on an ambiguous default.
    assert_eq!(
        resolve_import_commit(&storage, 101, Some(&a.id.to_string()))
            .await
            .unwrap()
            .commit_oid,
        a.id.to_string()
    );
}
