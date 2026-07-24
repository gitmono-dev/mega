use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use jupiter::redis::lock::RedLock;

use crate::transport::protocol::import_refs::RefCommand;

#[derive(Clone)]
pub enum TransportEvent {
    MonoReceivePackFinalized {
        repo_path: PathBuf,
        base_branch: String,
        from_hash: String,
        to_hash: String,
        username: Option<String>,
        /// Link already allocated while writing `refs/cl/*` during pack receive.
        /// Must be reused when creating the mega_cl row so merge can find the ref.
        cl_link: Option<String>,
    },
    ImportReceivePackFinalized {
        repo_path: PathBuf,
        repo_id: i64,
        commands: Vec<RefCommand>,
        unpack_redlock: Arc<RedLock>,
        extra_timings: Arc<Mutex<Vec<(String, u128)>>>,
    },
}
