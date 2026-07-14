//! Database migration module for the Jupiter application.
//!
//! This module provides database migration functionality using SeaORM's migration framework.
//! It contains all migration files and utilities for managing database schema changes.
//!
//! # Overview
//!
//! The migrator handles database schema evolution through versioned migration files.
//! Each migration is represented as a separate module and implements the `MigrationTrait`.
//!
//! # Migration Files
//!
//! - `m20250314_025943_init` - Initial database schema setup
//! - `m20250427_031332_add_mr_refs_tag` - Adds merge request reference tagging
//! - `m20250605_013340_alter_mega_mr_index` - Modifies merge request indexing
//! - `m20250610_000001_add_vault_storage` - Adds vault storage functionality
//! - `m20250613_033821_alter_user_id` - Alters user ID column definitions
//! - `m20250618_065050_add_label` - Adds label functionality to issues
//!
//! # Usage
//!
//! ```rust,ignore
//! use jupiter_migrate::apply_migrations;
//!
//! // Apply pending migrations
//! apply_migrations(&db, false).await?;
//!
//! // Refresh all migrations (development only)
//! apply_migrations(&db, true).await?;
//! ```
use sea_orm_migration::{prelude::*, schema::big_integer};

mod m20250314_025943_init;
mod m20250427_031332_add_mr_refs_tag;
mod m20250605_013340_alter_mega_mr_index;
mod m20250610_000001_add_vault_storage;
mod m20250613_033821_alter_user_id;
mod m20250618_065050_add_label;
mod m20250628_025312_add_username_in_conversation;
mod m20250702_072055_add_item_assignees;
mod m20250710_073119_create_reactions;
mod m20250725_103004_add_note;
mod m20250804_151214_alter_builds_end_at;
mod m20250812_022434_alter_mega_mr;
mod m20250815_075653_remove_commit_id;
mod m20250819_025231_alter_builds;
mod m20250820_102133_gpgkey;
mod m20250821_083749_add_checks;
mod m20250828_092459_remove_gpg_table;
mod m20250828_092729_create_standalone_table;
mod m20250903_013904_create_task_table;
mod m20250903_071928_add_issue_refs;
mod m20250904_074945_modify_tasks_and_builds;
mod m20250904_120000_add_commit_auths;
mod m20250905_163011_add_mr_reviewer;
mod m20250910_153212_add_username_to_reviewer;
mod m20250930_024736_mr_to_cl;
mod m20251011_091944_tasks_mr_id_to_cl_id;
mod m20251012_071700_mr_to_cl_batch;
mod m20251021_073817_rename_mr_sync_to_cl_sync;
mod m20251026_065433_drop_user_table;
mod m20251027_062734_add_metadata_to_object;
mod m20251107_025431_add_cl_commits;
mod m20251109_073000_add_merge_queue;
mod m20251117_101804_add_commit_id_in_mega_tree;
mod m20251117_181240_add_system_required_field_for_reviewer;
mod m20251119_145041_add_draft_status;
mod m20251125_135032_add_draft_conv_type;
mod m20251128_000001_create_buck_session;
mod m20251203_013745_add_dynamic_sidebar;
mod m20251210_113942_remove_unique_constraint_from_order_index;
mod m20260106_070511_add_retry_time;
mod m20260106_070515_remove_relay_mq_lfs_raw_table;
mod m20260108_085945_remove_splited_in_lfs_objects;
mod m20260108_105158_remove_storage_type_enum;
mod m20260115_000000_create_targets_table;
mod m20260119_060233_add_mega_code_review;
mod m20260127_081517_create_build_triggers;
mod m20260128_080549_add_mega_code_review_anchor_and_position;
mod m20260130_065535_refactor_orion_module;
mod m20260208_012349_change_build_events;
mod m20260209_064016_remove_default_dynamic_sidebar;
mod m20260210_062050_create_target_state_history;
mod m20260211_102158_add_username_to_mega_cl_sqlite;
mod m20260216_013852_create_group_permission_tables;
mod m20260224_142019_create_target_build_status;
mod m20260224_230000_create_notification_center;
mod m20260228_100254_change_build_target_and_add_index_for_build_event_start_at;
mod m20260302_082846_add_cla_sign_status;
mod m20260304_013434_seed_cla_sign_check_config;
mod m20260306_121829_create_bots_related_table;
mod m20260308_191753_create_webhook;
mod m20260308_220000_add_base_branch_to_mega_cl;
mod m20260308_230000_normalize_webhook_event_types;
mod m20260316_120000_add_bot_tokens_token_hash_index;
mod m20260324_024559_add_notes;
mod m20260324_033322_fix_migration;
mod m20260327_034553_drop_legacy_tasks;
mod m20260413_033315_create_artifact_tables;
mod m20260612_011232_drop_build_events_log;
mod m20260714_021900_create_user_approval_status;
mod runner;
pub use runner::apply_migrations;

/// Primary key `BIGINT` (not DB auto-increment); the application assigns `id` (e.g. `idgenerator::IdInstance::next_id`).
fn pk_bigint<T: IntoIden>(name: T) -> ColumnDef {
    big_integer(name).primary_key().take()
}

/// The main migrator struct that implements the migration trait.
///
/// This struct is responsible for managing all database migrations in the correct order.
pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20250314_025943_init::Migration),
            Box::new(m20250427_031332_add_mr_refs_tag::Migration),
            Box::new(m20250605_013340_alter_mega_mr_index::Migration),
            Box::new(m20250610_000001_add_vault_storage::Migration),
            Box::new(m20250613_033821_alter_user_id::Migration),
            Box::new(m20250618_065050_add_label::Migration),
            Box::new(m20250628_025312_add_username_in_conversation::Migration),
            Box::new(m20250702_072055_add_item_assignees::Migration),
            Box::new(m20250710_073119_create_reactions::Migration),
            Box::new(m20250725_103004_add_note::Migration),
            Box::new(m20250804_151214_alter_builds_end_at::Migration),
            Box::new(m20250812_022434_alter_mega_mr::Migration),
            Box::new(m20250815_075653_remove_commit_id::Migration),
            Box::new(m20250819_025231_alter_builds::Migration),
            Box::new(m20250820_102133_gpgkey::Migration),
            Box::new(m20250821_083749_add_checks::Migration),
            Box::new(m20250828_092459_remove_gpg_table::Migration),
            Box::new(m20250828_092729_create_standalone_table::Migration),
            Box::new(m20250903_013904_create_task_table::Migration),
            Box::new(m20250903_071928_add_issue_refs::Migration),
            Box::new(m20250904_074945_modify_tasks_and_builds::Migration),
            Box::new(m20250904_120000_add_commit_auths::Migration),
            Box::new(m20250905_163011_add_mr_reviewer::Migration),
            Box::new(m20250910_153212_add_username_to_reviewer::Migration),
            Box::new(m20250930_024736_mr_to_cl::Migration),
            Box::new(m20251011_091944_tasks_mr_id_to_cl_id::Migration),
            Box::new(m20251012_071700_mr_to_cl_batch::Migration),
            Box::new(m20251021_073817_rename_mr_sync_to_cl_sync::Migration),
            Box::new(m20251026_065433_drop_user_table::Migration),
            Box::new(m20251027_062734_add_metadata_to_object::Migration),
            Box::new(m20251107_025431_add_cl_commits::Migration),
            Box::new(m20251109_073000_add_merge_queue::Migration),
            Box::new(m20251117_101804_add_commit_id_in_mega_tree::Migration),
            Box::new(m20251117_181240_add_system_required_field_for_reviewer::Migration),
            Box::new(m20251119_145041_add_draft_status::Migration),
            Box::new(m20251125_135032_add_draft_conv_type::Migration),
            Box::new(m20251128_000001_create_buck_session::Migration),
            Box::new(m20251203_013745_add_dynamic_sidebar::Migration),
            Box::new(m20251210_113942_remove_unique_constraint_from_order_index::Migration),
            Box::new(m20260106_070511_add_retry_time::Migration),
            Box::new(m20260106_070515_remove_relay_mq_lfs_raw_table::Migration),
            Box::new(m20260108_085945_remove_splited_in_lfs_objects::Migration),
            Box::new(m20260108_105158_remove_storage_type_enum::Migration),
            Box::new(m20260115_000000_create_targets_table::Migration),
            Box::new(m20260119_060233_add_mega_code_review::Migration),
            Box::new(m20260127_081517_create_build_triggers::Migration),
            Box::new(m20260128_080549_add_mega_code_review_anchor_and_position::Migration),
            Box::new(m20260130_065535_refactor_orion_module::Migration),
            Box::new(m20260208_012349_change_build_events::Migration),
            Box::new(m20260209_064016_remove_default_dynamic_sidebar::Migration),
            Box::new(m20260210_062050_create_target_state_history::Migration),
            Box::new(m20260211_102158_add_username_to_mega_cl_sqlite::Migration),
            Box::new(m20260216_013852_create_group_permission_tables::Migration),
            Box::new(m20260224_142019_create_target_build_status::Migration),
            Box::new(m20260224_230000_create_notification_center::Migration),
            Box::new(m20260228_100254_change_build_target_and_add_index_for_build_event_start_at::Migration),
            Box::new(m20260302_082846_add_cla_sign_status::Migration),
            Box::new(m20260304_013434_seed_cla_sign_check_config::Migration),
            Box::new(m20260306_121829_create_bots_related_table::Migration),
            Box::new(m20260308_191753_create_webhook::Migration),
            Box::new(m20260308_220000_add_base_branch_to_mega_cl::Migration),
            Box::new(m20260308_230000_normalize_webhook_event_types::Migration),
            Box::new(m20260316_120000_add_bot_tokens_token_hash_index::Migration),
            Box::new(m20260324_024559_add_notes::Migration),
            Box::new(m20260324_033322_fix_migration::Migration),
            Box::new(m20260327_034553_drop_legacy_tasks::Migration),
            Box::new(m20260413_033315_create_artifact_tables::Migration),
            Box::new(m20260612_011232_drop_build_events_log::Migration),
            Box::new(m20260714_021900_create_user_approval_status::Migration),
        ]
    }
}
