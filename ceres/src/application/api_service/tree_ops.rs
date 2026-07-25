use std::{
    collections::VecDeque,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use common::errors::MegaError;
use git_internal::{
    errors::GitError,
    internal::object::{
        ObjectTrait,
        blob::Blob,
        tree::{Tree, TreeItem, TreeItemMode},
    },
};
use jupiter::utils::converter::generate_git_keep_with_timestamp;

use crate::{
    application::api_service::ApiHandler,
    model::git::{TreeBriefItem, TreeCommitItem, TreeHashItem},
};

pub async fn get_tree_commit_info<T: ApiHandler + ?Sized>(
    handler: &T,
    path: PathBuf,
    refs: Option<&str>,
) -> Result<Vec<TreeCommitItem>, GitError> {
    // Use refs-aware commit mapping to get individual commit info for each file/directory
    // This ensures each item shows its own last modification commit, not just the tag commit
    let commit_map = handler.item_to_commit_map(path, refs).await?;
    let mut items: Vec<TreeCommitItem> = commit_map.into_iter().map(TreeCommitItem::from).collect();
    items.sort_by(|a, b| {
        a.content_type
            .cmp(&b.content_type)
            .then(a.name.cmp(&b.name))
    });
    Ok(items)
}

pub async fn get_binary_tree_by_path<T: ApiHandler + ?Sized>(
    handler: &T,
    path: &Path,
    oid: Option<String>,
) -> Result<Vec<u8>, MegaError> {
    let Some(tree) = search_tree_by_path(handler, path, None).await? else {
        return Ok(vec![]);
    };
    if let Some(oid) = oid
        && oid != tree.id._to_string()
    {
        return Ok(vec![]);
    }
    Ok(tree.to_data()?)
}

/// Searches for a tree by a given path and refs (commit SHA or tag name). If refs is None/empty, use default root.
///
/// This function takes a `path` and searches for the corresponding tree
/// in the repository. It returns a `Result` containing an `Option<Tree>`.
/// If the tree is found, it returns `Some(Tree)`. If the path does not
/// exist, it returns `None`. In case of an error, it returns a `GitError`.
///
/// # Arguments
///
/// * `path` - A reference to the `Path` to search for the tree.
/// * `refs` - Optional commit SHA or tag name to search within. If None or empty, uses the default root.
///
/// # Returns
///
/// * `Result<Option<Tree>, GitError>` - A result containing an optional tree or a Git error.
pub async fn search_tree_by_path<T: ApiHandler + ?Sized>(
    handler: &T,
    path: &Path,
    refs: Option<&str>,
) -> Result<Option<Tree>, MegaError> {
    let relative_path = handler
        .strip_relative(path)
        .map_err(|e| MegaError::Other(e.to_string()))?;
    let root_tree = handler.get_root_tree(refs).await?;
    let mut search_tree = root_tree.clone();
    for component in relative_path.components() {
        // root tree already found
        if component != Component::RootDir {
            let target_name = component.as_os_str().to_str().unwrap();
            let search_res = search_tree
                .tree_items
                .iter()
                .find(|x| x.name == target_name);
            if let Some(search_res) = search_res {
                if !search_res.is_tree() {
                    return Ok(None);
                }
                let res = handler.get_tree_by_hash(&search_res.id.to_string()).await?;
                search_tree = res.clone();
            } else {
                return Ok(None);
            }
        }
    }
    Ok(Some(search_tree))
}

/// Searches for a tree in the Git repository by its path, creating intermediate trees if necessary,
/// and returns the trees involved in the update process.
///
/// # Arguments
///
/// * `path` - A reference to the path to search for.
///
/// # Returns
///
/// A vector of trees involved in the update process.
///
/// # Errors
///
/// Returns a `MegaError` if an error occurs during the search or tree creation process.
///
/// The returned [`Blob`] is the placeholder `.gitkeep` referenced by the new leaf tree.
/// Callers **must** persist it via object storage (`save_blobs` / `put_objects`) before
/// or with the trees; otherwise clone/fetch will 404 on that object.
pub async fn search_and_create_tree<T: ApiHandler + ?Sized>(
    handler: &T,
    path: &Path,
) -> Result<(VecDeque<Tree>, Blob), MegaError> {
    let relative_path = handler.strip_relative(path)?;
    let root_tree = handler.get_root_tree(None).await?;
    let mut search_tree = root_tree.clone();
    let mut update_item_tree = VecDeque::new();
    update_item_tree.push_back((root_tree, Component::RootDir));
    let mut saving_trees = VecDeque::new();
    let mut stack: VecDeque<_> = VecDeque::new();

    for component in relative_path.components() {
        if component == Component::RootDir {
            continue;
        }

        let target_name = component.as_os_str().to_str().unwrap();
        if let Some(search_res) = search_tree
            .tree_items
            .iter()
            .find(|x| x.name == target_name)
        {
            search_tree = handler.get_tree_by_hash(&search_res.id.to_string()).await?;
            update_item_tree.push_back((search_tree.clone(), component));
        } else {
            stack.push_back(component);
        }
    }

    let blob = generate_git_keep_with_timestamp();
    let mut last_tree = Tree::from_tree_items(vec![TreeItem {
        mode: TreeItemMode::Blob,
        id: blob.id,
        name: String::from(".gitkeep"),
    }])
    .unwrap();
    let mut last_tree_name = "";
    let mut first_element = true;

    while let Some(component) = stack.pop_back() {
        if first_element {
            first_element = false;
        } else {
            last_tree = Tree::from_tree_items(vec![TreeItem {
                mode: TreeItemMode::Tree,
                id: last_tree.id,
                name: last_tree_name.to_owned(),
            }])
            .unwrap();
        }
        saving_trees.push_back(last_tree.clone());
        last_tree_name = component.as_os_str().to_str().unwrap();
    }

    if let Some((mut new_item_tree, search_name_component)) = update_item_tree.pop_back() {
        new_item_tree.tree_items.push(TreeItem {
            mode: TreeItemMode::Tree,
            id: last_tree.id,
            name: last_tree_name.to_owned(),
        });
        last_tree = Tree::from_tree_items(new_item_tree.tree_items).unwrap();
        saving_trees.push_back(last_tree.clone());

        let mut replace_hash = last_tree.id;
        let mut search_name = search_name_component.as_os_str().to_str().unwrap();
        while let Some((mut tree, component)) = update_item_tree.pop_back() {
            if let Some(index) = tree.tree_items.iter().position(|x| x.name == search_name) {
                tree.tree_items[index].id = replace_hash;
                let new_tree = Tree::from_tree_items(tree.tree_items).unwrap();
                replace_hash = new_tree.id;
                search_name = component.as_os_str().to_str().unwrap();
                saving_trees.push_back(new_tree);
            }
        }
    }

    Ok((saving_trees, blob))
}

/// return the dir's hash only
pub async fn get_tree_dir_hash<T: ApiHandler + ?Sized>(
    handler: &T,
    path: PathBuf,
    dir_name: &str,
    refs: Option<&str>,
) -> Result<Vec<TreeHashItem>, GitError> {
    match search_tree_by_path(handler, &path, refs).await? {
        Some(tree) => {
            let items: Vec<TreeHashItem> = tree
                .tree_items
                .into_iter()
                .filter(|x| x.mode == TreeItemMode::Tree && x.name == dir_name)
                .map(TreeHashItem::from)
                .collect();
            Ok(items)
        }
        None => Ok(Vec::new()),
    }
}

pub async fn get_tree_info<T: ApiHandler + ?Sized>(
    handler: &T,
    path: &Path,
    refs: Option<&str>,
) -> Result<Vec<TreeBriefItem>, GitError> {
    match search_tree_by_path(handler, path, refs).await? {
        Some(tree) => {
            let items = tree
                .tree_items
                .into_iter()
                .map(|item| {
                    let full_path = path.join(&item.name);
                    let mut info: TreeBriefItem = item.into();
                    info.path = full_path.to_str().unwrap().to_owned();
                    info
                })
                .collect();
            Ok(items)
        }
        None => Ok(vec![]),
    }
}

/// Searches for a tree in the Git repository by its path and returns the trees involved in the update and the target tree.
///
/// # Arguments
///
/// * `path` - A reference to the path to search for.
///
/// # Returns
///
/// A tuple containing:
/// - A vector of trees involved in the update process.
/// - The target tree found at the end of the search.
///
/// # Errors
///
/// Returns a `GitError` if the path does not exist.
pub async fn search_tree_for_update<T: ApiHandler + ?Sized>(
    handler: &T,
    path: &Path,
) -> Result<Vec<Arc<Tree>>, GitError> {
    // strip repo root prefix
    let relative_path = handler
        .strip_relative(path)
        .map_err(|e| GitError::CustomError(e.to_string()))?;
    let root_tree = handler.get_root_tree(None).await?;

    // init state
    let mut current_tree = Arc::new(root_tree.clone());
    let mut update_chain = vec![Arc::new(root_tree)];

    for component in relative_path.components() {
        // root tree already found
        if component != Component::RootDir {
            let target_name = component.as_os_str().to_str().unwrap();

            // lookup child
            let search_res = current_tree
                .tree_items
                .iter()
                .find(|x| x.name == target_name)
                .ok_or_else(|| {
                    GitError::CustomError(format!(
                        "Path '{}' not exist, please create path first!",
                        target_name
                    ))
                })?;
            // fetch next tree
            current_tree = Arc::new(handler.get_tree_by_hash(&search_res.id.to_string()).await?);
            update_chain.push(current_tree.clone());
        }
    }
    Ok(update_chain)
}

pub async fn get_tree_content_hash<T: ApiHandler + ?Sized>(
    handler: &T,
    path: PathBuf,
    refs: Option<&str>,
) -> Result<Vec<TreeHashItem>, GitError> {
    match search_tree_by_path(handler, &path, refs).await? {
        Some(tree) => {
            let mut items: Vec<TreeHashItem> = tree
                .tree_items
                .into_iter()
                .map(TreeHashItem::from)
                .collect();

            // sort with type and name
            items.sort_by(|a, b| {
                a.content_type
                    .cmp(&b.content_type)
                    .then(a.name.cmp(&b.name))
            });
            Ok(items)
        }
        None => Ok(Vec::new()),
    }
}
