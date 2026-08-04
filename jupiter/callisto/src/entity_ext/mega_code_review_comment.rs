use crate::{entity_ext::generate_id, mega_code_review_comment};

impl mega_code_review_comment::Model {
    pub fn new(
        thread_id: i64,
        parent_id: Option<i64>,
        campsite_user_id: String,
        content: Option<String>,
    ) -> Self {
        let now = chrono::Utc::now().naive_utc();

        Self {
            id: generate_id(),
            thread_id,
            parent_id,
            campsite_user_id,
            content,
            created_at: now,
            updated_at: now,
        }
    }
}
