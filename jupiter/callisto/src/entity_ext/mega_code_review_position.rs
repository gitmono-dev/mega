use crate::{
    entity_ext::generate_id,
    mega_code_review_position,
    sea_orm_active_enums::{DiffSideEnum, PositionStatusEnum},
};

impl mega_code_review_position::Model {
    pub fn new(
        anchor_id: i64,
        commit_sha: &str,
        file_path: &str,
        diff_side: &DiffSideEnum,
        line_number: i32,
        confidence: i32,
        position_status: PositionStatusEnum,
    ) -> Self {
        let now = chrono::Utc::now().naive_utc();

        Self {
            id: generate_id(),
            anchor_id,
            commit_sha: commit_sha.to_owned(),
            file_path: file_path.to_owned(),
            diff_side: diff_side.to_owned(),
            line_number,
            confidence,
            position_status,
            created_at: now,
            updated_at: now,
        }
    }
}
