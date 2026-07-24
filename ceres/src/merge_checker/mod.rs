use std::{collections::HashMap, fmt, str::FromStr, sync::Arc};

use async_trait::async_trait;
use callisto::{check_result, sea_orm_active_enums::CheckTypeEnum};
use common::errors::MegaError;
use jupiter::{model::cl_dto::ClInfoDto, storage::Storage};
use serde::Serialize;
use utoipa::ToSchema;

use crate::merge_checker::{
    cl_sync_checker::ClSyncChecker, cla_sign_checker::ClaSignChecker,
    commit_message_checker::CommitMessageChecker, gpg_signature_checker::GpgSignatureChecker,
};

pub mod cl_sync_checker;
mod cla_sign_checker;
mod code_review_checker;
mod commit_message_checker;
pub(crate) mod gpg_signature_checker;

#[async_trait]
pub trait Checker: Send + Sync {
    async fn run(&self, params: &serde_json::Value) -> CheckResult;

    async fn build_params(&self, cl_info: &ClInfoDto) -> Result<serde_json::Value, MegaError>;
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, ToSchema)]
pub enum CheckType {
    GpgSignature,
    BranchProtection,
    CommitMessage,
    ClSync,
    MergeConflict,
    CiStatus,
    CodeReview,
    ClaSign,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub enum ConditionResult {
    FAILED,
    PASSED,
}

impl fmt::Display for ConditionResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            ConditionResult::FAILED => "FAILED",
            ConditionResult::PASSED => "PASSED",
        };
        write!(f, "{}", s)
    }
}

impl FromStr for ConditionResult {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "PASSED" => Ok(ConditionResult::PASSED),
            "FAILED" => Ok(ConditionResult::FAILED),
            _ => Err(()),
        }
    }
}

impl CheckType {
    pub fn display_name(&self) -> &'static str {
        match self {
            CheckType::GpgSignature => "Gpg signature",
            CheckType::BranchProtection => "Branch protection",
            CheckType::CommitMessage => "Commit message",
            CheckType::ClSync => "Cl sync",
            CheckType::MergeConflict => "Merge conflict",
            CheckType::CiStatus => "Ci status",
            CheckType::CodeReview => "Code review",
            CheckType::ClaSign => "CLA sign",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            CheckType::GpgSignature => {
                "Verify whether the commit has a valid GPG signature and the key is trusted"
            }
            CheckType::BranchProtection => {
                "Ensure the merge target complies with branch protection policies, such as no direct merges to main and requiring squash or rebase"
            }
            CheckType::CommitMessage => {
                "Verify whether the commit message follows Conventional Commits or the internal agreed-upon format"
            }
            CheckType::ClSync => {
                "Ensure the CL is based on the latest commit of the target branch and determine whether a rebase is required"
            }
            CheckType::MergeConflict => {
                "The pull request must not have any unresolved merge conflicts"
            }
            CheckType::CiStatus => {
                "Verify that all required continuous integration pipelines have passed"
            }
            CheckType::CodeReview => {
                "Ensure the required reviewers have approved the merge request"
            }
            CheckType::ClaSign => {
                "Report whether the CL author has signed CLA (advisory; does not block merge)"
            }
        }
    }
}

impl From<CheckTypeEnum> for CheckType {
    fn from(value: CheckTypeEnum) -> Self {
        match value {
            CheckTypeEnum::GpgSignature => CheckType::GpgSignature,
            CheckTypeEnum::BranchProtection => CheckType::BranchProtection,
            CheckTypeEnum::CommitMessage => CheckType::CommitMessage,
            CheckTypeEnum::ClSync => CheckType::ClSync,
            CheckTypeEnum::MergeConflict => CheckType::MergeConflict,
            CheckTypeEnum::CiStatus => CheckType::CiStatus,
            CheckTypeEnum::CodeReview => CheckType::CodeReview,
            CheckTypeEnum::ClaSign => CheckType::ClaSign,
        }
    }
}

impl From<CheckType> for CheckTypeEnum {
    fn from(value: CheckType) -> Self {
        match value {
            CheckType::GpgSignature => CheckTypeEnum::GpgSignature,
            CheckType::BranchProtection => CheckTypeEnum::BranchProtection,
            CheckType::CommitMessage => CheckTypeEnum::CommitMessage,
            CheckType::ClSync => CheckTypeEnum::ClSync,
            CheckType::MergeConflict => CheckTypeEnum::MergeConflict,
            CheckType::CiStatus => CheckTypeEnum::CiStatus,
            CheckType::CodeReview => CheckTypeEnum::CodeReview,
            CheckType::ClaSign => CheckTypeEnum::ClaSign,
        }
    }
}

#[derive(Debug)]
pub struct CheckResult {
    pub check_type_code: CheckType,
    pub status: ConditionResult,
    pub message: String,
}

pub struct CheckerRegistry {
    checkers: HashMap<CheckType, Box<dyn Checker>>,
    storage: Arc<Storage>,
    #[allow(dead_code)]
    username: String,
}

impl CheckerRegistry {
    pub fn new(storage: Arc<Storage>, username: String) -> Self {
        let mut r = CheckerRegistry {
            checkers: HashMap::new(),
            storage: storage.clone(),
            username,
        };
        r.register(
            CheckType::ClSync,
            Box::new(ClSyncChecker {
                storage: storage.clone(),
            }),
        );
        r.register(
            CheckType::GpgSignature,
            Box::new(GpgSignatureChecker {
                storage: storage.clone(),
            }),
        );
        r.register(
            CheckType::CodeReview,
            Box::new(code_review_checker::CodeReviewChecker {
                storage: storage.clone(),
            }),
        );
        r.register(
            CheckType::ClaSign,
            Box::new(ClaSignChecker {
                storage: storage.clone(),
            }),
        );
        r.register(CheckType::CommitMessage, Box::new(CommitMessageChecker));

        r
    }

    pub fn register(&mut self, check_type: CheckType, checker: Box<dyn Checker>) {
        self.checkers.insert(check_type, checker);
    }

    pub async fn run_checks(&self, cl_info: ClInfoDto) -> Result<(), MegaError> {
        let check_configs = self
            .storage
            .cl_storage()
            .get_checks_config_by_path(&cl_info.path)
            .await?;
        let mut save_models = vec![];

        for c_config in check_configs {
            if let Some(checker) = self.checkers.get(&c_config.check_type_code.into()) {
                let params = checker.build_params(&cl_info).await?;
                let res = checker.run(&params).await;
                let model = check_result::Model::new(
                    &cl_info.path,
                    &cl_info.link,
                    &cl_info.to_hash,
                    res.check_type_code.into(),
                    &res.status.to_string(),
                    &res.message,
                );
                save_models.push(model);
            }
        }
        self.storage
            .cl_storage()
            .save_check_results(save_models)
            .await?;
        Ok(())
    }
}
