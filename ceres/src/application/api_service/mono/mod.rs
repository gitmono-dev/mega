//! Monorepo API implementation (`MonoApiService` and domain modules).

pub mod admin;
pub mod app_services;
pub mod context;
pub mod logic;
pub mod service;
pub mod types;

pub mod buck;
pub mod cl;
pub mod cl_list;
pub mod cla;
pub mod code_review;
pub mod commit;
pub mod conversation;
pub mod dynamic_sidebar;
pub mod edit;
pub mod git_ops;
pub mod gpg;
pub mod issue;
pub mod label_assignee;
pub mod lfs;
pub mod note;
pub mod reviewer;
pub mod stack;
pub mod sync;
pub mod tag;
pub mod user;
pub mod user_approval;

pub use admin::{ADMIN_FILE, EffectiveResourcePermission};
pub use app_services::MonoAppServices;
pub use cl::merge_strategy as cl_merge;
pub use context::{
    AdminApplicationService, ClApplicationService, CodeReviewApplicationService,
    ConversationApplicationService, IssueApplicationService, LfsApplicationService,
    NoteApplicationService, ReviewerApplicationService, SidebarApplicationService,
    UserApplicationService, WebhookApplicationService,
};
pub use git_ops::GitOpsPort;
pub use logic::MonoServiceLogic;
pub use service::{GitApplicationService, MonoApiService};
pub use stack::build_mono_stack;
pub use types::{RefUpdate, TreeUpdateResult};
