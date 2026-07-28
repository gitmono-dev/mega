use api_model::common::Pagination;
use common::errors::MegaError;

use super::context::IssueApplicationService;
use crate::model::{
    change_list::ListPayload,
    issue::{IssueDetailRes, IssueSuggestions, ItemRes},
    label::LabelItem,
};

impl IssueApplicationService {
    pub async fn get_issue_details(
        &self,
        link: &str,
        username: String,
    ) -> Result<IssueDetailRes, MegaError> {
        let details = self
            .ctx
            .storage()
            .issue_service
            .get_issue_details(link, username)
            .await?;
        let mut detail: IssueDetailRes = details.into();
        let mut names: Vec<String> = detail
            .conversations
            .iter()
            .map(|c| c.username.clone())
            .collect();
        names.push(detail.author.clone());
        let bot_names = self
            .ctx
            .storage()
            .bots_storage()
            .bot_names_among(&names)
            .await?;
        detail.author_is_bot = bot_names.contains(&detail.author) || detail.author == "system";
        crate::model::conversation::ConversationItem::apply_bot_flags(
            &mut detail.conversations,
            &bot_names,
        );
        Ok(detail)
    }

    pub async fn get_issue_suggestions(
        &self,
        query: &str,
    ) -> Result<Vec<IssueSuggestions>, MegaError> {
        let (issues, cls) = self
            .ctx
            .storage()
            .issue_service
            .get_suggestions(query)
            .await?;
        let mut res: Vec<IssueSuggestions> = issues.into_iter().map(|m| m.into()).collect();
        let mut mr_list: Vec<IssueSuggestions> = cls.into_iter().map(|m| m.into()).collect();
        res.append(&mut mr_list);
        res.sort();
        Ok(res)
    }

    pub async fn get_issue_list(
        &self,
        filter: ListPayload,
        pagination: Pagination,
    ) -> Result<(Vec<ItemRes>, u64), MegaError> {
        let (items, total) = self
            .ctx
            .storage()
            .issue_service
            .issue_store()
            .get_issue_list(filter.into(), pagination)
            .await?;
        let mut items: Vec<ItemRes> = items.into_iter().map(|m| m.into()).collect();
        let names: Vec<String> = items.iter().map(|i| i.author.clone()).collect();
        let bot_names = self
            .ctx
            .storage()
            .bots_storage()
            .bot_names_among(&names)
            .await?;
        ItemRes::apply_bot_flags(&mut items, &bot_names);
        Ok((items, total))
    }

    pub async fn save_issue(
        &self,
        username: &str,
        title: &str,
    ) -> Result<callisto::mega_issue::Model, MegaError> {
        self.ctx
            .storage()
            .issue_service
            .issue_store()
            .save_issue(username, title)
            .await
    }

    pub async fn close_issue(&self, link: &str) -> Result<(), MegaError> {
        self.ctx
            .storage()
            .issue_service
            .issue_store()
            .close_issue(link)
            .await
    }

    pub async fn reopen_issue(&self, link: &str) -> Result<(), MegaError> {
        self.ctx
            .storage()
            .issue_service
            .issue_store()
            .reopen_issue(link)
            .await
    }

    pub async fn edit_issue_title(&self, link: &str, title: &str) -> Result<(), MegaError> {
        self.ctx
            .storage()
            .issue_service
            .issue_store()
            .edit_title(link, title)
            .await
    }

    pub async fn list_labels_by_page(
        &self,
        pagination: Pagination,
        query: &str,
    ) -> Result<(Vec<LabelItem>, u64), MegaError> {
        let (items, total) = self
            .ctx
            .storage()
            .issue_service
            .issue_store()
            .list_labels_by_page(pagination, query)
            .await?;
        Ok((items.into_iter().map(|m| m.into()).collect(), total))
    }

    pub async fn new_label(
        &self,
        name: &str,
        color: &str,
        description: &str,
    ) -> Result<LabelItem, MegaError> {
        let model = self
            .ctx
            .storage()
            .issue_service
            .issue_store()
            .new_label(name, color, description)
            .await?;
        Ok(model.into())
    }

    pub async fn get_label_by_id(&self, id: i64) -> Result<Option<LabelItem>, MegaError> {
        let label = self
            .ctx
            .storage()
            .issue_service
            .issue_store()
            .get_label_by_id(id)
            .await?;
        Ok(label.map(|m| m.into()))
    }
}
