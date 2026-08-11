use std::sync::Arc;

use async_trait::async_trait;
use common::errors::MegaError;
use jupiter::{model::cl_dto::ClInfoDto, storage::Storage};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    application::member_identity,
    merge_checker::{CheckResult, CheckType, Checker, ConditionResult},
};

pub struct ClaSignChecker {
    pub storage: Arc<Storage>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ClaSignParams {
    username: String,
}

impl ClaSignParams {
    fn from_value(v: &serde_json::Value) -> anyhow::Result<Self> {
        Ok(serde_json::from_value(v.clone())?)
    }
}

#[async_trait]
impl Checker for ClaSignChecker {
    async fn run(&self, params: &Value) -> CheckResult {
        let params = ClaSignParams::from_value(params).expect("parse params err");
        let mut res = CheckResult {
            check_type_code: CheckType::ClaSign,
            status: ConditionResult::FAILED,
            message: String::new(),
        };

        let aliases = member_identity::aliases_for_actor(&self.storage, &params.username).await;
        match self.storage.cla_storage().any_signed(&aliases).await {
            Ok(true) => {
                res.status = ConditionResult::PASSED;
                res.message = "CLA signed".to_string();
            }
            Ok(false) => {
                res.status = ConditionResult::FAILED;
                res.message = "CLA_NOT_SIGNED: You have not signed the CLA yet.".to_string();
            }
            Err(e) => {
                res.status = ConditionResult::FAILED;
                res.message = format!("CLA check failed: {e}");
            }
        }

        res
    }

    async fn build_params(&self, cl_info: &ClInfoDto) -> Result<Value, MegaError> {
        Ok(serde_json::json!({
            "username": cl_info.username,
        }))
    }
}
