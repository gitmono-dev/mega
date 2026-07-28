use api_model::common::{CommonPage, Pagination};
use common::errors::MegaError;

use super::context::ClApplicationService;
use crate::model::{change_list::ListPayload, issue::ItemRes};

impl ClApplicationService {
    pub async fn get_cl_list(
        &self,
        filter: ListPayload,
        pagination: Pagination,
    ) -> Result<CommonPage<ItemRes>, MegaError> {
        let (items, total) = self
            .storage()
            .cl_service
            .cl_store()
            .get_cl_list(filter.into(), pagination)
            .await?;
        let mut items: Vec<ItemRes> = items.into_iter().map(|m| m.into()).collect();
        let names: Vec<String> = items.iter().map(|i| i.author.clone()).collect();
        let bot_names = self
            .storage()
            .bots_storage()
            .bot_names_among(&names)
            .await?;
        ItemRes::apply_bot_flags(&mut items, &bot_names);
        Ok(CommonPage { items, total })
    }

    pub async fn get_cl_model(&self, link: &str) -> Result<callisto::mega_cl::Model, MegaError> {
        self.storage()
            .cl_service
            .cl_store()
            .get_cl(link)
            .await?
            .ok_or_else(|| MegaError::NotFound(format!("CL {link} not found")))
    }
}
