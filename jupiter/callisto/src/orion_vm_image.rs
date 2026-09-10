//! `SeaORM` Entity for Orion scheduler VM image catalog.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "orion_vm_image")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    #[sea_orm(unique)]
    pub digest: String,
    #[sea_orm(column_type = "Text")]
    pub object_key: String,
    #[sea_orm(column_type = "Text")]
    pub info_object_key: Option<String>,
    pub image_name: Option<String>,
    pub built_at: Option<String>,
    pub rust: Option<String>,
    pub buck2: Option<String>,
    pub python: Option<String>,
    pub kernel: Option<String>,
    pub size_bytes: Option<i64>,
    pub label: Option<String>,
    pub created_at: DateTime,
}

impl ActiveModelBehavior for ActiveModel {}
