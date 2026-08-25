use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};

use async_trait::async_trait;
use common::{
    config::PackConfig,
    errors::{MegaError, ProtocolError},
    utils::ZERO_ID,
};
use futures::{TryStreamExt, future::join_all};
use git_internal::{
    errors::GitError,
    hash::ObjectHash,
    internal::{
        metadata::{EntryMeta, MetaAttached},
        object::{
            blob::Blob,
            tree::{Tree, TreeItemMode},
        },
        pack::{Pack, entry::Entry},
    },
};
use io_orbit::object_storage::MultiObjectByteStream;
use sysinfo::System;
use tokio::sync::{Semaphore, mpsc::UnboundedReceiver};
use tokio_stream::wrappers::ReceiverStream;

use crate::transport::protocol::import_refs::{RefCommand, Refs};

pub mod import_repo;
pub mod monorepo;

pub use crate::infra::pack_stream::{PackByteStream, PackStreamError, into_pack_byte_stream};

#[async_trait]
pub trait RepoHandler: Send + Sync + 'static {
    fn is_monorepo(&self) -> bool;

    /// Concurrency limit for `save_entry` batches during receive-pack.
    /// `0` means unbounded.
    fn save_entry_concurrency(&self) -> usize {
        1
    }

    /// Optional extra timing key-values to be included in the final receive-pack report.
    /// Implementations may return an empty vec.
    ///
    /// Intended for repo-type-specific breakdown (e.g. import attach lock waits).
    fn receive_pack_extra_timings_ms(&self) -> Vec<(String, u128)> {
        Vec::new()
    }

    /// Replace the handler's ref-command snapshot after unpack and the protocol status loop
    /// (`default_branch`, `status`, etc.). The handler is built from an early `commands.clone()`,
    /// so without this, metadata updated in `git_receive_pack_stream` would be stale at finalize.
    fn sync_commands_after_unpack(&self, _commands: &[RefCommand]) {}

    async fn refs_with_head_hash(&self) -> (String, Vec<Refs>);

    async fn receiver_handler(
        self: Arc<Self>,
        mut rx: UnboundedReceiver<MetaAttached<Entry, EntryMeta>>,
        _rx_pack_id: UnboundedReceiver<ObjectHash>,
    ) -> Result<(), MegaError> {
        let t0 = Instant::now();
        let mut entry_list = vec![];
        let limit = self.save_entry_concurrency();
        let semaphore = if limit == 0 {
            None
        } else {
            Some(Arc::new(Semaphore::new(limit)))
        };
        let mut join_tasks = vec![];

        //let temp_pack_id = Uuid::new_v4().to_string();
        let temp_pack_id = String::new();

        let mut total_entries: usize = 0;
        let mut flush_batches: usize = 0;
        while let Some(mut entry) = rx.recv().await {
            self.check_entry(&entry.inner).await?;
            entry.meta.set_pack_id(temp_pack_id.clone());
            entry_list.push(entry);
            total_entries += 1;
            if entry_list.len() >= 1000 {
                flush_batches += 1;
                let entries = std::mem::take(&mut entry_list);
                let shared = self.clone();
                let sem = semaphore.clone();
                let handle = tokio::spawn(async move {
                    let _permit = match sem {
                        Some(s) => Some(s.acquire_owned().await.expect("semaphore closed")),
                        None => None,
                    };
                    let t = Instant::now();
                    let len = entries.len();
                    shared.save_entry(entries).await.map(|_| {
                        tracing::debug!(
                            service = "git-receive-pack",
                            batch_entries = len,
                            elapsed_ms = t.elapsed().as_millis(),
                            "save_entry batch completed"
                        );
                    })
                });
                join_tasks.push(handle);
            }
        }
        // process left entries
        if !entry_list.is_empty() {
            flush_batches += 1;
            let handler = self.clone();
            let entries = std::mem::take(&mut entry_list);
            let sem = semaphore.clone();
            let handle = tokio::spawn(async move {
                let _permit = match sem {
                    Some(s) => Some(s.acquire_owned().await.expect("semaphore closed")),
                    None => None,
                };
                let t = Instant::now();
                let len = entries.len();
                handler.save_entry(entries).await.map(|_| {
                    tracing::debug!(
                        service = "git-receive-pack",
                        batch_entries = len,
                        elapsed_ms = t.elapsed().as_millis(),
                        "save_entry batch completed"
                    );
                })
            });
            join_tasks.push(handle);
        }

        let results = join_all(join_tasks).await;
        for (i, res) in results.into_iter().enumerate() {
            match res {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    tracing::error!("Task {} save_entry Err: {:?}", i, e);
                    return Err(MegaError::Other(format!(
                        "Failed to save entry in repository in task {}: {}",
                        i, e
                    )));
                }
                Err(join_err) => {
                    tracing::error!("Task {} panic or cancle: {:?}", i, join_err);
                }
            }
        }

        // The feature of updating pack id has performance issues. Temporarily disabled
        // // receive pack_id and update it
        // if let Some(real_pack_id) = rx_pack_id.recv().await {
        //     let real_pack_id_str = real_pack_id.to_string();
        //     tracing::debug!(
        //         "Received real pack_id: {}, updating database from temp_pack_id: {}",
        //         real_pack_id_str,
        //         temp_pack_id
        //     );
        //
        //     通过数据库操作更新 pack_id
        //     if let Err(e) = self.update_pack_id(&temp_pack_id, &real_pack_id_str).await {
        //         tracing::error!("Failed to update pack_id in database: {:?}", e);
        //         return Err(GitError::CustomError(format!(
        //             "Failed to update pack_id: {:?}",
        //             e
        //         )));
        //     }
        // }

        tracing::debug!(
            service = "git-receive-pack",
            total_entries = total_entries,
            flush_batches = flush_batches,
            save_entry_concurrency = limit,
            elapsed_ms = t0.elapsed().as_millis(),
            "receiver_handler done"
        );
        Ok(())
    }

    /// After unpack succeeds: persist branch metadata, run repo-specific side effects, then the
    /// protocol layer appends ref report lines. Tags are already persisted earlier.
    ///
    /// - **Import**: one DB transaction for `import_refs` + monorepo attach (under unpack lock).
    /// - **Mono**: one DB transaction for all branch CL `mega_refs` updates; CL / conversations /
    ///   build hooks run afterward (not in that transaction).
    async fn finalize_receive_pack(&self) -> Result<(), MegaError>;

    async fn save_entry(
        &self,
        entry_list: Vec<MetaAttached<Entry, EntryMeta>>,
    ) -> Result<(), MegaError>;

    async fn update_pack_id(&self, temp_pack_id: &str, pack_id: &str) -> Result<(), MegaError>;

    async fn check_entry(&self, entry: &Entry) -> Result<(), GitError>;

    /// Asynchronously retrieves the full pack data for the specified repository path.
    /// This function collects commits and nodes from the storage and packs them into
    /// a single binary vector. There is no need to build the entire tree; the function
    /// only sends all the data related to this repository.
    ///
    /// # Returns
    /// * `Result<Vec<u8>, GitError>` - The packed binary data as a vector of bytes.
    ///
    async fn full_pack(&self, want: Vec<String>) -> Result<ReceiverStream<Vec<u8>>, GitError>;

    async fn incremental_pack(
        &self,
        want: Vec<String>,
        have: Vec<String>,
    ) -> Result<ReceiverStream<Vec<u8>>, GitError>;

    async fn get_trees_by_hashes(&self, hashes: Vec<String>) -> Result<Vec<Tree>, MegaError>;

    async fn get_blobs_by_hashes(
        &self,
        hashes: Vec<String>,
    ) -> Result<MultiObjectByteStream<'_>, MegaError>;

    async fn get_blob_metadata_by_hashes(
        &self,
        hashes: Vec<String>,
    ) -> Result<HashMap<String, EntryMeta>, MegaError>;

    async fn update_refs(&self, refs: &RefCommand) -> Result<(), GitError>;

    async fn check_commit_exist(&self, hash: &str) -> bool;

    /// Whether the repository stores any object with this id (tag, commit,
    /// tree, or blob) that a pushed lightweight or annotated tag could
    /// reference.
    async fn check_object_exist(&self, hash: &str) -> bool;

    async fn check_default_branch(&self) -> bool;

    fn find_head_hash(&self, refs: Vec<Refs>) -> (String, Vec<Refs>) {
        let mut head_hash = ZERO_ID.to_string();
        for git_ref in refs.iter() {
            if git_ref.default_branch {
                head_hash.clone_from(&git_ref.ref_hash);
            }
        }
        (head_hash, refs)
    }

    async fn unpack_stream(
        &self,
        pack_config: &PackConfig,
        stream: PackByteStream,
    ) -> Result<
        (
            UnboundedReceiver<MetaAttached<Entry, EntryMeta>>,
            UnboundedReceiver<ObjectHash>,
        ),
        ProtocolError,
    > {
        let total_mem = || {
            let sys = System::new_all();
            Ok(sys.total_memory() as usize)
        };

        let cache_mem =
            match PackConfig::get_size_from_str(&pack_config.pack_decode_mem_size, total_mem) {
                Ok(mem) => mem,
                Err(err) => return Err(ProtocolError::InvalidInput(err)),
            };

        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let (pack_id_sender, pack_id_receiver) = tokio::sync::mpsc::unbounded_channel();

        let p = Pack::new(
            None,
            Some(cache_mem),
            Some(pack_config.pack_decode_cache_path.clone()),
            pack_config.clean_cache_after_decode,
        );
        let decode_stream = stream.map_err(crate::infra::map_decode_stream_error);
        p.decode_stream(decode_stream, sender, Some(pack_id_sender))
            .await;
        Ok((receiver, pack_id_receiver))
    }

    async fn traverse_for_count(
        &self,
        tree: Tree,
        exist_objs: &HashSet<String>,
        counted_obj: &mut HashSet<String>,
        obj_num: &AtomicUsize,
    ) {
        let mut search_tree_ids = vec![];
        let mut search_blob_ids = vec![];
        for item in &tree.tree_items {
            let hash = item.id.to_string();
            if !exist_objs.contains(&hash) && counted_obj.insert(hash.clone()) {
                if item.mode == TreeItemMode::Tree {
                    search_tree_ids.push(hash.clone())
                } else {
                    search_blob_ids.push(hash.clone());
                }
            }
        }
        obj_num.fetch_add(search_blob_ids.len(), Ordering::SeqCst);
        let trees = self.get_trees_by_hashes(search_tree_ids).await.unwrap();
        for t in trees {
            self.traverse_for_count(t, exist_objs, counted_obj, obj_num)
                .await;
        }
        obj_num.fetch_add(1, Ordering::SeqCst);
    }

    /// Traverse a tree structure asynchronously.
    ///
    /// This function traverses a given tree, keeps track of processed objects, and optionally sends
    /// traversal data to a provided sender. The function will:
    /// 1. Traverse the tree and calculate the quantities of tree and blob items.
    /// 2. If a sender is provided, send blob and tree data via the sender.
    ///
    /// # Parameters
    /// - `tree`: The tree structure to traverse.
    /// - `exist_objs`: A mutable reference to a set containing already processed object IDs.
    /// - `sender`: An optional sender for sending traversal data.
    ///
    /// # Details
    /// - The function processes tree items, distinguishing between tree and blob items.
    /// - It collects IDs of items that have not been processed yet.
    /// - It retrieves and sends blob data if a sender is provided.
    /// - It recursively traverses sub-trees.
    /// - It sends the entire tree data if a sender is provided.
    async fn traverse(
        &self,
        tree: Tree,
        exist_objs: &mut HashSet<String>,
        sender: Option<&tokio::sync::mpsc::Sender<MetaAttached<Entry, EntryMeta>>>,
    ) -> Result<(), MegaError> {
        let mut search_tree_ids = vec![];
        let mut search_blob_ids = vec![];

        for item in &tree.tree_items {
            let hash = item.id.to_string();
            if exist_objs.insert(hash.clone()) {
                if item.mode == TreeItemMode::Tree {
                    search_tree_ids.push(hash);
                } else {
                    search_blob_ids.push(hash);
                }
            }
        }

        if let Some(sender) = sender {
            let blobs = self.get_blobs_by_hashes(search_blob_ids.clone()).await?;
            let blobs_ext_data = self
                .get_blob_metadata_by_hashes(search_blob_ids.clone())
                .await?;

            let default_meta = EntryMeta::default();
            blobs
                .try_for_each_concurrent(16, |(_, stream, _)| async {
                    let data = stream
                        .try_fold(Vec::new(), |mut acc, bytes| async move {
                            acc.extend_from_slice(&bytes);
                            Ok(acc)
                        })
                        .await?;
                    let blob = Blob::from_content_bytes(data);
                    let ext_data = blobs_ext_data
                        .get(&blob.id.to_string())
                        .unwrap_or(&default_meta);
                    sender
                        .send(MetaAttached {
                            inner: blob.into(),
                            meta: ext_data.to_owned(),
                        })
                        .await
                        .unwrap();

                    Ok(())
                })
                .await?;
        }

        let trees = self.get_trees_by_hashes(search_tree_ids).await?;
        for t in trees {
            self.traverse(t, exist_objs, sender).await?;
        }

        if let Some(sender) = sender {
            sender
                .send(MetaAttached {
                    inner: tree.into(),
                    meta: EntryMeta::new(),
                })
                .await
                .unwrap();
        }
        Ok(())
    }

    async fn traverses_tree_and_update_filepath(&self) -> Result<(), MegaError>;
}
