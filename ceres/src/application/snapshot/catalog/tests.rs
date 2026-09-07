use std::sync::Arc;

use callisto::{
    git_commit, git_repo, git_tree, import_refs, mega_refs, mega_tree,
    sea_orm_active_enums::RefTypeEnum,
};
use git_internal::internal::{
    metadata::EntryMeta,
    object::{
        blob::Blob,
        commit::Commit,
        tree::{Tree, TreeItem, TreeItemMode},
    },
};
use jupiter::utils::converter::IntoGitModel;
use sea_orm::{ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter, sea_query::Expr};
use tempfile::TempDir;

use super::*;

async fn fixture() -> (TempDir, SourceCatalog) {
    let dir = TempDir::new().unwrap();
    let connection = jupiter::tests::test_db_connection(dir.path()).await;
    jupiter_migrate::apply_migrations(&connection, true)
        .await
        .unwrap();
    (
        dir,
        SourceCatalog::from_base(BaseStorage::new(Arc::new(connection))),
    )
}

fn path(value: &str) -> RepoPath {
    RepoPath::new(value).unwrap()
}
fn relative(value: &str) -> RelativePath {
    RelativePath::new(value).unwrap()
}

fn ref_selector(id: &SourceId, scope: &str) -> SourceSelector {
    SourceSelector::SourceRef {
        source_id: id.clone(),
        scope_path: path(scope),
        ref_name: crate::model::snapshot::RefName::new("refs/heads/main").unwrap(),
    }
}

fn fixed(source: &SourceSnapshot) -> SourceSelector {
    SourceSelector::SourceCommit {
        source_id: source.source_id.clone(),
        scope_path: source.scope_path.clone(),
        commit_oid: source.commit_oid.clone(),
    }
}

fn file_tree(content: &str) -> Tree {
    Tree::from_tree_items(vec![TreeItem {
        mode: TreeItemMode::Blob,
        name: "file.txt".into(),
        id: Blob::from_content(content).id,
    }])
    .unwrap()
}

async fn native_commit(catalog: &SourceCatalog, tree: Tree) -> Commit {
    let commit = Commit::from_tree_id(tree.id, vec![], "snapshot fixture");
    catalog
        .mono
        .save_mega_trees(vec![tree], commit.id, None)
        .await
        .unwrap();
    catalog
        .mono
        .save_mega_commits(vec![commit.clone()], None)
        .await
        .unwrap();
    commit
}

async fn native_ref(catalog: &SourceCatalog, scope: &str, commit: &Commit) {
    let now = chrono::Utc::now().naive_utc();
    catalog
        .mono
        .save_refs(
            mega_refs::Model {
                id: common::utils::generate_id(),
                path: scope.into(),
                ref_name: "refs/heads/main".into(),
                ref_commit_hash: commit.id.to_string(),
                ref_tree_hash: commit.tree_id.to_string(),
                created_at: now,
                updated_at: now,
                is_cl: false,
            },
            None,
        )
        .await
        .unwrap();
}

async fn import_repo(catalog: &SourceCatalog, repo_id: i64, scope: &str) {
    let now = chrono::Utc::now().naive_utc();
    catalog
        .imports
        .save_git_repo(git_repo::Model {
            id: repo_id,
            repo_path: scope.into(),
            repo_name: format!("r{repo_id}"),
            created_at: now,
            updated_at: now,
        })
        .await
        .unwrap();
}

async fn import_commit(catalog: &SourceCatalog, repo_id: i64, content: &str) -> (Commit, Tree) {
    let tree = file_tree(content);
    let commit = Commit::from_tree_id(tree.id, vec![], content);
    let mut tree_model = tree.clone().into_git_model(EntryMeta::new());
    tree_model.repo_id = repo_id;
    git_tree::Entity::insert(tree_model.into_active_model())
        .exec(catalog.imports.get_connection())
        .await
        .unwrap();
    let mut commit_model = commit.clone().into_git_model(EntryMeta::new());
    commit_model.repo_id = repo_id;
    git_commit::Entity::insert(commit_model.into_active_model())
        .exec(catalog.imports.get_connection())
        .await
        .unwrap();
    (commit, tree)
}

async fn import_ref(catalog: &SourceCatalog, repo_id: i64, commit: &Commit) {
    let now = chrono::Utc::now().naive_utc();
    catalog
        .imports
        .save_ref(
            repo_id,
            import_refs::Model {
                id: common::utils::generate_id(),
                repo_id,
                ref_name: "refs/heads/main".into(),
                ref_git_id: commit.id.to_string(),
                ref_type: RefTypeEnum::Branch,
                default_branch: true,
                created_at: now,
                updated_at: now,
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn native_projection_keeps_root_provenance_and_survives_ref_cleanup() {
    let (_dir, catalog) = fixture().await;
    let child = file_tree("old");
    let child_commit = native_commit(&catalog, child.clone()).await;
    let root = Tree::from_tree_items(vec![TreeItem {
        mode: TreeItemMode::Tree,
        id: child.id,
        name: "pkg".into(),
    }])
    .unwrap();
    let root_commit = native_commit(&catalog, root).await;
    native_ref(&catalog, "/", &root_commit).await;
    native_ref(&catalog, "/pkg", &child_commit).await;
    let id = catalog.register_native().await.unwrap();
    let pinned = catalog.resolve(&ref_selector(&id, "/")).await.unwrap();
    let projected = catalog
        .project_native(&pinned, &path("/pkg"))
        .await
        .unwrap();
    assert_eq!(projected.commit_oid, pinned.commit_oid);
    assert_eq!(projected.root_tree_oid.as_str(), child.id.to_string());
    assert_ne!(projected.commit_oid.as_str(), child_commit.id.to_string());
    let scoped = catalog.resolve(&ref_selector(&id, "/pkg")).await.unwrap();
    assert_eq!(scoped.root_tree_oid, projected.root_tree_oid);
    assert_ne!(scoped.id(), projected.id());

    mega_refs::Entity::delete_many()
        .exec(catalog.mono.get_connection())
        .await
        .unwrap();
    let reloaded = SourceCatalog::from_base(catalog.mono.base.clone());
    assert_eq!(
        reloaded.resolve(&fixed(&projected)).await.unwrap(),
        projected
    );
    let file = reloaded
        .locate(&projected, &relative("file.txt"))
        .await
        .unwrap();
    assert_eq!(file.oid.as_str(), child.tree_items[0].id.to_string());
    assert!(matches!(
        reloaded.locate(&projected, &relative("pkg/file.txt")).await,
        Err(MegaError::NotFound(_))
    ));
    assert!(
        reloaded
            .project_native(&projected, &path("/pkgs"))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn native_commit_cannot_be_reinterpreted_as_an_unproven_scope() {
    let (_dir, catalog) = fixture().await;
    let child_commit = native_commit(&catalog, file_tree("scoped")).await;
    native_ref(&catalog, "/project", &child_commit).await;
    let id = catalog.register_native().await.unwrap();
    let pinned = catalog
        .resolve(&ref_selector(&id, "/project"))
        .await
        .unwrap();
    let wrong = SourceSelector::SourceCommit {
        source_id: id,
        scope_path: path("/"),
        commit_oid: pinned.commit_oid.clone(),
    };
    assert!(
        matches!(catalog.resolve(&wrong).await, Err(MegaError::BadRequest(message)) if message.contains("SCOPE_UNKNOWN"))
    );
    let forged = SourceSnapshot {
        scope_path: path("/"),
        ..pinned
    };
    assert!(
        catalog
            .locate(&forged, &relative("file.txt"))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn import_old_source_survives_move_delete_and_path_reuse() {
    let (_dir, catalog) = fixture().await;
    import_repo(&catalog, 101, "/third-party/r").await;
    let (a, a_tree) = import_commit(&catalog, 101, "A").await;
    let (b, _) = import_commit(&catalog, 101, "B").await;
    import_ref(&catalog, 101, &a).await;
    let id = catalog
        .register_import(&path("/third-party/r"))
        .await
        .unwrap();
    let old = catalog
        .resolve(&ref_selector(&id, "/third-party/r"))
        .await
        .unwrap();
    catalog
        .imports
        .update_ref(101, "refs/heads/main", &b.id.to_string())
        .await
        .unwrap();
    git_repo::Entity::update_many()
        .col_expr(
            git_repo::Column::RepoPath,
            Expr::value("/third-party/moved"),
        )
        .filter(git_repo::Column::Id.eq(101))
        .exec(catalog.imports.get_connection())
        .await
        .unwrap();
    let new = catalog
        .resolve(&ref_selector(&id, "/third-party/moved"))
        .await
        .unwrap();
    assert_eq!(new.source_id, old.source_id);
    assert_ne!(new.commit_oid, old.commit_oid);
    assert_eq!(catalog.resolve(&fixed(&old)).await.unwrap(), old);
    assert!(
        catalog
            .resolve(&ref_selector(&id, "/third-party/r"))
            .await
            .is_err()
    );

    catalog
        .imports
        .remove_ref(101, "refs/heads/main")
        .await
        .unwrap();
    git_repo::Entity::delete_by_id(101)
        .exec(catalog.imports.get_connection())
        .await
        .unwrap();
    import_repo(&catalog, 102, "/third-party/r").await;
    let replacement_id = catalog
        .register_import(&path("/third-party/r"))
        .await
        .unwrap();
    assert_ne!(replacement_id, old.source_id);
    assert_eq!(catalog.resolve(&fixed(&old)).await.unwrap(), old);
    let old_file = catalog.locate(&old, &relative("file.txt")).await.unwrap();
    assert_eq!(old_file.oid.as_str(), a_tree.tree_items[0].id.to_string());
    let forged = SourceSnapshot {
        source_id: replacement_id,
        ..old
    };
    assert!(
        catalog
            .locate(&forged, &relative("file.txt"))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn source_membership_binds_path_kind_and_oid_not_global_cas_presence() {
    let (_dir, catalog) = fixture().await;
    import_repo(&catalog, 101, "/r").await;
    let (a, tree) = import_commit(&catalog, 101, "A").await;
    import_ref(&catalog, 101, &a).await;
    let id = catalog.register_import(&path("/r")).await.unwrap();
    let source = catalog.resolve(&ref_selector(&id, "/r")).await.unwrap();
    let file_oid = ObjectId::new(tree.tree_items[0].id.to_string()).unwrap();
    catalog
        .prove_object(&source, &relative("file.txt"), ObjectKind::Blob, &file_oid)
        .await
        .unwrap();
    assert!(
        catalog
            .prove_object(&source, &relative("file.txt"), ObjectKind::Tree, &file_oid)
            .await
            .is_err()
    );
    assert!(
        catalog
            .prove_object(&source, &relative("missing"), ObjectKind::Blob, &file_oid)
            .await
            .is_err()
    );
    assert!(
        catalog
            .prove_object(
                &source,
                &relative("file.txt"),
                ObjectKind::Blob,
                &source.root_tree_oid
            )
            .await
            .is_err()
    );
    let bytes = catalog
        .read_tree_payload(&source, &relative(""), &source.root_tree_oid)
        .await
        .unwrap();
    object::verify_object(ObjectKind::Tree, &source.root_tree_oid, &bytes).unwrap();
    let foreign = ObjectId::new(Blob::from_content("foreign").id.to_string()).unwrap();
    let forged = SourceSnapshot {
        root_tree_oid: foreign,
        ..source
    };
    assert!(catalog.locate(&forged, &relative("")).await.is_err());
}

#[tokio::test]
async fn inconsistent_native_ref_and_corrupt_root_do_not_create_proofs() {
    let (_dir, catalog) = fixture().await;
    let commit = native_commit(&catalog, file_tree("A")).await;
    native_ref(&catalog, "/", &commit).await;
    let id = catalog.register_native().await.unwrap();
    mega_refs::Entity::update_many()
        .col_expr(mega_refs::Column::RefTreeHash, Expr::value("1".repeat(40)))
        .exec(catalog.mono.get_connection())
        .await
        .unwrap();
    assert!(matches!(
        catalog.resolve(&ref_selector(&id, "/")).await,
        Err(MegaError::Unavailable(_))
    ));
    mega_refs::Entity::update_many()
        .col_expr(
            mega_refs::Column::RefTreeHash,
            Expr::value(commit.tree_id.to_string()),
        )
        .exec(catalog.mono.get_connection())
        .await
        .unwrap();
    mega_tree::Entity::update_many()
        .col_expr(mega_tree::Column::SubTrees, Expr::value(Vec::<u8>::new()))
        .exec(catalog.mono.get_connection())
        .await
        .unwrap();
    assert!(matches!(
        catalog.resolve(&ref_selector(&id, "/")).await,
        Err(MegaError::Unavailable(_))
    ));
    assert!(
        catalog
            .proofs
            .scope(id.as_str(), "/", &commit.id.to_string())
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn native_symlink_and_gitlink_are_not_traversed_as_directories() {
    let (_dir, catalog) = fixture().await;
    let tree = Tree::from_tree_items(vec![
        TreeItem {
            mode: TreeItemMode::Link,
            name: "link".into(),
            id: Blob::from_content("../outside").id,
        },
        TreeItem {
            mode: TreeItemMode::Commit,
            name: "submodule".into(),
            id: Blob::from_content("commit-shaped fixture").id,
        },
    ])
    .unwrap();
    let commit = native_commit(&catalog, tree).await;
    native_ref(&catalog, "/", &commit).await;
    let id = catalog.register_native().await.unwrap();
    let source = catalog.resolve(&ref_selector(&id, "/")).await.unwrap();
    let link = catalog.locate(&source, &relative("link")).await.unwrap();
    assert_eq!(link.kind, EntryKind::Symlink);
    catalog
        .prove_object(&source, &relative("link"), ObjectKind::Blob, &link.oid)
        .await
        .unwrap();
    let sub = catalog
        .locate(&source, &relative("submodule"))
        .await
        .unwrap();
    assert_eq!(sub.kind, EntryKind::Gitlink);
    assert!(
        catalog
            .prove_object(&source, &relative("submodule"), ObjectKind::Blob, &sub.oid)
            .await
            .is_err()
    );
    assert!(
        catalog
            .locate(&source, &relative("link/child"))
            .await
            .is_err()
    );
    assert!(
        catalog
            .locate(&source, &relative("submodule/child"))
            .await
            .is_err()
    );
}
