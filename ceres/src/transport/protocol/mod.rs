use core::fmt;
use std::{
    collections::HashSet,
    path::PathBuf,
    str::FromStr,
    sync::{Arc, Mutex},
};

use callisto::sea_orm_active_enums::RefTypeEnum;
use common::{
    errors::{MegaError, ProtocolError},
    utils::nested_import_repo_conflict_message,
};
use import_refs::{CommandType, RefCommand};
use jupiter::redis::lock::RedLock;
use repo::Repo;
use tokio::sync::RwLock;

use crate::{
    bus::TransportRuntime,
    transport::pack::{
        RepoHandler,
        import_repo::ImportRepo,
        monorepo::{BranchTip, MonoRepo},
    },
};

pub mod import_refs;
pub mod repo;
pub mod smart;

pub use common::utils::ZERO_ID;

#[derive(Clone, Debug)]
pub struct PushUserInfo {
    pub username: String,
}

#[derive(Clone, Debug)]
pub struct AuthContext {
    /// The actor username associated with the protocol operation (if available).
    pub username: Option<String>,
    /// The authenticated push user info (if available).
    pub authenticated_user: Option<PushUserInfo>,
}

#[derive(Clone, Debug)]
pub struct SmartSession {
    pub repo_path: PathBuf,
    pub service_type: ServiceType,
    pub transport_protocol: TransportProtocol,
    pub auth: AuthContext,
    pub capabilities: HashSet<Capability>,
}

#[derive(Debug, PartialEq, Clone, Copy, Default)]
pub enum TransportProtocol {
    Local,
    #[default]
    Http,
    Ssh,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ServiceType {
    UploadPack,
    ReceivePack,
}

impl fmt::Display for ServiceType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ServiceType::UploadPack => write!(f, "git-upload-pack"),
            ServiceType::ReceivePack => write!(f, "git-receive-pack"),
        }
    }
}

impl FromStr for ServiceType {
    type Err = MegaError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "git-upload-pack" => Ok(ServiceType::UploadPack),
            "git-receive-pack" => Ok(ServiceType::ReceivePack),
            _ => Err(MegaError::Other(format!("Invalid service name: {}", s))),
        }
    }
}

// TODO: Additional Capabilitys need to be supplemented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    MultiAck,
    MultiAckDetailed,
    NoDone,
    SideBand,
    SideBand64k,
    ReportStatus,
    ReportStatusv2,
    OfsDelta,
    DeepenSince,
    DeepenNot,
}

impl FromStr for Capability {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "report-status" => Ok(Capability::ReportStatus),
            "report-status-v2" => Ok(Capability::ReportStatusv2),
            "side-band" => Ok(Capability::SideBand),
            "side-band-64k" => Ok(Capability::SideBand64k),
            "ofs-delta" => Ok(Capability::OfsDelta),
            "multi_ack" => Ok(Capability::MultiAck),
            "multi_ack_detailed" => Ok(Capability::MultiAckDetailed),
            "no-done" => Ok(Capability::NoDone),
            "deepen-since" => Ok(Capability::DeepenSince),
            "deepen-not" => Ok(Capability::DeepenNot),
            _ => Err(()),
        }
    }
}

pub enum SideBind {
    // sideband 1 will contain packfile data,
    PackfileData,
    // sideband 2 will be used for progress information that the client will generally print to stderr and
    ProgressInfo,
    // sideband 3 is used for error information.
    Error,
}

impl SideBind {
    pub fn value(&self) -> u8 {
        match self {
            Self::PackfileData => b'\x01',
            Self::ProgressInfo => b'\x02',
            Self::Error => b'\x03',
        }
    }
}
pub struct RefUpdateRequest {
    pub commands: Vec<RefCommand>,
}

impl SmartSession {
    pub fn new(
        repo_path: PathBuf,
        service_type: ServiceType,
        transport_protocol: TransportProtocol,
    ) -> Self {
        SmartSession {
            repo_path,
            service_type,
            transport_protocol,
            auth: AuthContext {
                username: None,
                authenticated_user: None,
            },
            capabilities: HashSet::new(),
        }
    }

    pub async fn repo_handler_with_commands(
        &self,
        state: &TransportRuntime,
        commands: Vec<RefCommand>,
    ) -> Result<Arc<dyn RepoHandler>, ProtocolError> {
        let config = state.storage.config();
        let import_dir = config.monorepo.import_dir.clone();

        if self.repo_path.starts_with(import_dir.clone()) {
            let storage = state.storage.git_db_storage();
            let path_str = self.repo_path.to_str().unwrap();
            let model = storage.find_git_repo_exact_match(path_str).await.unwrap();
            let repo = if let Some(repo) = model {
                repo.into()
            } else {
                match self.service_type {
                    ServiceType::UploadPack => {
                        return Err(ProtocolError::NotFound("Repository not found.".to_owned()));
                    }
                    ServiceType::ReceivePack => {
                        if let Some(conflict) = storage
                            .find_nested_import_repo_conflict(path_str)
                            .await
                            .map_err(|e| {
                                ProtocolError::InvalidInput(format!(
                                    "failed to check nested import repo conflict: {e}"
                                ))
                            })?
                        {
                            return Err(ProtocolError::InvalidInput(
                                nested_import_repo_conflict_message(path_str, &conflict.repo_path),
                            ));
                        }
                        let repo = Repo::new(self.repo_path.clone(), false);
                        storage
                            .save_git_repo(repo.clone().into())
                            .await
                            .map_err(|e| {
                                ProtocolError::InvalidInput(format!(
                                    "failed to create import repo: {e}"
                                ))
                            })?;
                        repo
                    }
                }
            };

            // Deleting the current default branch would leave the repository
            // with a dangling HEAD: ref discovery advertises a zero id and
            // import APIs unwrap the now-missing default ref. Reject before
            // any persistence (tags included) happens.
            if let Some(default_ref) = storage
                .get_ref(repo.repo_id)
                .await
                .map_err(|e| ProtocolError::InvalidInput(format!("failed to load refs: {e}")))?
                .into_iter()
                .find(|r| r.default_branch)
                && commands.iter().any(|c| {
                    c.ref_type == RefTypeEnum::Branch
                        && c.command_type == CommandType::Delete
                        && c.ref_name == default_ref.ref_name
                })
            {
                return Err(ProtocolError::InvalidInput(format!(
                    "cannot delete the current default branch {}",
                    default_ref.ref_name
                )));
            }

            let unpack_redlock = Arc::new(RedLock::new(
                state.git_object_cache.connection.clone(),
                // Serialize monorepo root mega_refs update across concurrent import attaches.
                // Filepath updates and per-repo work should not be blocked by this lock.
                "git:receive-pack:lock:monorepo-root".to_string(),
                30_000, // 30s TTL
            ));
            Ok(Arc::new(ImportRepo {
                git_object_cache: state.git_object_cache.clone(),
                storage: state.storage.clone(),
                repo,
                command_list: Mutex::new(commands),
                unpack_redlock,
                application: state.application.clone(),
                receive_pack_extra_timings_ms: Mutex::new(Vec::new()),
            }) as Arc<dyn RepoHandler>)
        } else {
            // Metadata must describe a surviving change: a deletion's zero
            // new_id would otherwise flow into finalize events even when the
            // deletion itself is rejected and other updates land.
            let tip = commands
                .iter()
                .find(|x| {
                    x.ref_type == RefTypeEnum::Branch && x.command_type != CommandType::Delete
                })
                .map(|command| BranchTip {
                    base_branch: command
                        .ref_name
                        .strip_prefix("refs/heads/")
                        .unwrap_or(command.ref_name.as_str())
                        .to_string(),
                    from_hash: command.old_id.clone(),
                    to_hash: command.new_id.clone(),
                })
                .unwrap_or_else(|| BranchTip {
                    base_branch: "main".to_string(),
                    from_hash: String::new(),
                    to_hash: String::new(),
                });
            let res = MonoRepo {
                git_object_cache: state.git_object_cache.clone(),
                storage: state.storage.clone(),
                path: self.repo_path.clone(),
                tip: Mutex::new(tip),
                current_commit: Arc::new(RwLock::new(None)),
                cl_link: Arc::new(RwLock::new(None)),
                application: state.application.clone(),
                username: self.auth.username.clone(),
                command_list: Mutex::new(commands.clone()),
            };
            Ok(Arc::new(res) as Arc<dyn RepoHandler>)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{Capability, ServiceType, SideBind};

    #[test]
    fn service_type_from_str_parses_known_services() {
        assert_eq!(
            ServiceType::from_str("git-upload-pack").unwrap(),
            ServiceType::UploadPack
        );
        assert_eq!(
            ServiceType::from_str("git-receive-pack").unwrap(),
            ServiceType::ReceivePack
        );
        assert!(ServiceType::from_str("git-invalid").is_err());
    }

    #[test]
    fn capability_from_str_parses_known_values() {
        assert_eq!(
            Capability::from_str("report-status-v2").unwrap(),
            Capability::ReportStatusv2
        );
        assert!(Capability::from_str("unknown-cap").is_err());
    }

    #[test]
    fn side_bind_values_match_git_sideband_codes() {
        assert_eq!(SideBind::PackfileData.value(), 1);
        assert_eq!(SideBind::ProgressInfo.value(), 2);
        assert_eq!(SideBind::Error.value(), 3);
    }
}
