use crate::{
    entity_ext::{generate_hash_content, generate_id, normalize},
    mega_code_review_anchor,
    sea_orm_active_enums::DiffSideEnum,
};

impl mega_code_review_anchor::Model {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        thread_id: i64,
        file_path: &str,
        diff_side: &DiffSideEnum,
        anchor_commit_sha: &str,
        original_line_number: i32,
        normalized_content: &str,
        context_before: &str,
        context_after: &str,
    ) -> Self {
        let now = chrono::Utc::now().naive_utc();

        Self {
            id: generate_id(),
            thread_id,
            file_path: file_path.to_owned(),
            diff_side: diff_side.to_owned(),
            anchor_commit_sha: anchor_commit_sha.to_owned(),
            original_line_number,
            normalized_content: normalized_content.to_owned(),
            normalized_hash: generate_hash_content(&normalize(normalized_content)),
            context_before: context_before.to_owned(),
            context_before_hash: generate_hash_content(&normalize(context_before)),
            context_after: context_after.to_owned(),
            context_after_hash: generate_hash_content(&normalize(context_after)),
            created_at: now,
        }
    }
}
