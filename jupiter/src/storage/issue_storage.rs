use std::{collections::HashMap, ops::Deref};

use api_model::common::Pagination;
use callisto::{
    issue_cl_references, item_assignees, item_labels, label, mega_conversation, mega_issue,
    sea_orm_active_enums::ReferenceTypeEnum,
};
use common::errors::MegaError;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, EntityTrait, IntoActiveModel, JoinType,
    PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, QueryTrait, RelationTrait, Set,
    TransactionTrait, prelude::Expr,
};

use crate::{
    model::common::{ItemDetails, LabelAssigneeParams, ListParams},
    storage::{
        base_storage::{BaseStorage, StorageConnector},
        stg_common::{
            combine_item_list,
            query_build::{apply_sort, filter_by_assignees, filter_by_labels},
        },
    },
};

#[derive(Clone)]
pub struct IssueStorage {
    pub base: BaseStorage,
}

impl Deref for IssueStorage {
    type Target = BaseStorage;
    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl IssueStorage {
    pub async fn get_issue_list(
        &self,
        params: ListParams,
        page: Pagination,
    ) -> Result<(Vec<ItemDetails>, u64), MegaError> {
        let cond = Condition::all();
        let cond = filter_by_labels(cond, params.labels);
        let cond = filter_by_assignees(cond, params.assignees);

        let query = mega_issue::Entity::find()
            .join(
                JoinType::LeftJoin,
                callisto::entity_ext::mega_issue::Relation::ItemLabels.def(),
            )
            .join(
                JoinType::LeftJoin,
                callisto::entity_ext::mega_issue::Relation::ItemAssignees.def(),
            )
            .filter(mega_issue::Column::Status.eq(params.status))
            .apply_if(params.author, |q, author| {
                q.filter(mega_issue::Column::CampsiteUserId.eq(author))
            })
            .filter(cond)
            .distinct()
            .order_by_asc(mega_issue::Column::Id);

        let mut sort_map = HashMap::new();
        sort_map.insert("created_at", mega_issue::Column::CreatedAt);
        sort_map.insert("updated_at", mega_issue::Column::UpdatedAt);

        let query = apply_sort(query, params.sort_by.as_deref(), params.asc, &sort_map);

        let paginator = query.paginate(self.get_connection(), page.per_page);
        let total = paginator.num_items().await?;
        let issues = paginator.fetch_page(page.page - 1).await?;

        if issues.is_empty() {
            return Ok((vec![], 0));
        }

        let issue_ids = issues.iter().map(|m| m.id).collect::<Vec<_>>();

        let label_query =
            mega_issue::Entity::find().filter(mega_issue::Column::Id.is_in(issue_ids.clone()));
        let label_query = apply_sort(
            label_query,
            params.sort_by.as_deref(),
            params.asc,
            &sort_map,
        );
        let labels: Vec<(mega_issue::Model, Vec<label::Model>)> = label_query
            .find_with_related(label::Entity)
            .all(self.get_connection())
            .await?;

        let assignees: Vec<(mega_issue::Model, Vec<item_assignees::Model>)> =
            mega_issue::Entity::find()
                .filter(mega_issue::Column::Id.is_in(issue_ids.clone()))
                .find_with_related(item_assignees::Entity)
                .all(self.get_connection())
                .await?;

        let conversations: Vec<(mega_issue::Model, Vec<mega_conversation::Model>)> =
            mega_issue::Entity::find()
                .filter(mega_issue::Column::Id.is_in(issue_ids))
                .find_with_related(mega_conversation::Entity)
                .all(self.get_connection())
                .await?;

        let res = combine_item_list::<mega_issue::Entity>(labels, assignees, conversations);

        Ok((res, total))
    }

    pub async fn get_issue_suggestions_by_query(
        &self,
        query: &str,
    ) -> Result<Vec<mega_issue::Model>, MegaError> {
        let keyword = format!("%{query}%");
        let res = mega_issue::Entity::find()
            .filter(
                Condition::any()
                    .add(mega_issue::Column::Link.like(&keyword))
                    .add(mega_issue::Column::Title.like(&keyword)),
            )
            .limit(5)
            .all(self.get_connection())
            .await?;
        Ok(res)
    }

    pub async fn get_issue(&self, link: &str) -> Result<Option<mega_issue::Model>, MegaError> {
        let model = mega_issue::Entity::find()
            .filter(mega_issue::Column::Link.eq(link))
            .one(self.get_connection())
            .await
            .unwrap();
        Ok(model)
    }

    pub async fn get_issue_labels(
        &self,
        link: &str,
    ) -> Result<Option<(mega_issue::Model, Vec<label::Model>)>, MegaError> {
        let labels: Vec<(mega_issue::Model, Vec<label::Model>)> = mega_issue::Entity::find()
            .filter(mega_issue::Column::Link.eq(link))
            .find_with_related(label::Entity)
            .all(self.get_connection())
            .await?;
        Ok(labels.first().cloned())
    }

    pub async fn get_issue_assignees(
        &self,
        link: &str,
    ) -> Result<Option<(mega_issue::Model, Vec<item_assignees::Model>)>, MegaError> {
        let assignees: Vec<(mega_issue::Model, Vec<item_assignees::Model>)> =
            mega_issue::Entity::find()
                .filter(mega_issue::Column::Link.eq(link))
                .find_with_related(item_assignees::Entity)
                .all(self.get_connection())
                .await?;
        Ok(assignees.first().cloned())
    }

    pub async fn get_issue_by_id(&self, id: i64) -> Result<Option<mega_issue::Model>, MegaError> {
        let model = mega_issue::Entity::find_by_id(id)
            .one(self.get_connection())
            .await
            .unwrap();
        Ok(model)
    }

    pub async fn save_issue(
        &self,
        username: &str,
        title: &str,
    ) -> Result<mega_issue::Model, MegaError> {
        let model = mega_issue::Model::new(title.to_owned(), username.to_owned());
        let res = model
            .into_active_model()
            .insert(self.get_connection())
            .await?;
        Ok(res)
    }

    pub async fn edit_title(&self, link: &str, title: &str) -> Result<(), MegaError> {
        mega_issue::Entity::update_many()
            .col_expr(mega_issue::Column::Title, Expr::value(title))
            .col_expr(
                mega_issue::Column::UpdatedAt,
                Expr::value(chrono::Utc::now().naive_utc()),
            )
            .filter(mega_issue::Column::Link.eq(link))
            .exec(self.get_connection())
            .await?;
        Ok(())
    }

    pub async fn close_issue(&self, link: &str) -> Result<(), MegaError> {
        if let Some(model) = self.get_issue(link).await.unwrap() {
            let mut issue = model.into_active_model();
            issue.status = Set("closed".to_owned());
            issue.update(self.get_connection()).await.unwrap();
        };
        Ok(())
    }

    pub async fn reopen_issue(&self, link: &str) -> Result<(), MegaError> {
        if let Some(model) = self.get_issue(link).await.unwrap() {
            let mut issue = model.into_active_model();
            issue.status = Set("open".to_owned());
            issue.update(self.get_connection()).await.unwrap();
        };
        Ok(())
    }

    pub async fn new_label(
        &self,
        name: &str,
        color: &str,
        description: &str,
    ) -> Result<label::Model, MegaError> {
        let model = label::Model::new(name, color, description);
        let res = model
            .into_active_model()
            .insert(self.get_connection())
            .await?;
        Ok(res)
    }

    pub async fn get_label_by_id(&self, id: i64) -> Result<Option<label::Model>, MegaError> {
        let model = label::Entity::find_by_id(id)
            .one(self.get_connection())
            .await?;
        Ok(model)
    }

    pub async fn list_labels_by_page(
        &self,
        page: Pagination,
        name: &str,
    ) -> Result<(Vec<label::Model>, u64), MegaError> {
        let mut condition = Condition::all();
        if !name.is_empty() {
            let name = format!("%{name}%");
            condition = condition.add(label::Column::Name.like(name));
        }
        let paginator = label::Entity::find()
            .filter(condition)
            .paginate(self.get_connection(), page.per_page);
        let num_pages = paginator.num_items().await?;
        Ok(paginator
            .fetch_page(page.page - 1)
            .await
            .map(|m| (m, num_pages))?)
    }

    pub async fn find_item_exist_labels(
        &self,
        item_id: i64,
    ) -> Result<Vec<item_labels::Model>, MegaError> {
        let item_labels = item_labels::Entity::find()
            .filter(item_labels::Column::ItemId.eq(item_id))
            .all(self.get_connection())
            .await?;
        Ok(item_labels)
    }

    pub async fn find_item_exist_assignees(
        &self,
        item_id: i64,
    ) -> Result<Vec<item_assignees::Model>, MegaError> {
        let item_assignees = item_assignees::Entity::find()
            .filter(item_assignees::Column::ItemId.eq(item_id))
            .all(self.get_connection())
            .await?;
        Ok(item_assignees)
    }

    pub async fn modify_labels(
        &self,
        to_add: Vec<i64>,
        to_remove: Vec<i64>,
        params: LabelAssigneeParams,
    ) -> Result<(), MegaError> {
        let txn = self.get_connection().begin().await?;

        let LabelAssigneeParams { item_id, item_type } = params;

        if !to_remove.is_empty() {
            item_labels::Entity::delete_many()
                .filter(item_labels::Column::ItemId.eq(item_id))
                .filter(item_labels::Column::LabelId.is_in(to_remove.clone()))
                .exec(&txn)
                .await?;
        }

        if !to_add.is_empty() {
            let mut new_item_labels = Vec::new();
            for label_id in to_add.clone() {
                new_item_labels.push(
                    item_labels::Model {
                        created_at: chrono::Utc::now().naive_utc(),
                        updated_at: chrono::Utc::now().naive_utc(),
                        item_id,
                        label_id,
                        item_type: item_type.clone(),
                    }
                    .into_active_model(),
                );
            }

            item_labels::Entity::insert_many(new_item_labels)
                .exec(&txn)
                .await?;
        }

        txn.commit().await?;

        Ok(())
    }

    pub async fn modify_assignees(
        &self,
        to_add: Vec<String>,
        to_remove: Vec<String>,
        params: LabelAssigneeParams,
    ) -> Result<(), MegaError> {
        let txn = self.get_connection().begin().await?;

        let LabelAssigneeParams { item_id, item_type } = params;

        if !to_remove.is_empty() {
            item_assignees::Entity::delete_many()
                .filter(item_assignees::Column::ItemId.eq(item_id))
                .filter(item_assignees::Column::CampsiteUserId.is_in(to_remove.clone()))
                .exec(&txn)
                .await?;
        }

        if !to_add.is_empty() {
            let mut new_item = Vec::new();
            for campsite_user_id in to_add.clone() {
                new_item.push(
                    item_assignees::Model {
                        created_at: chrono::Utc::now().naive_utc(),
                        updated_at: chrono::Utc::now().naive_utc(),
                        item_id,
                        campsite_user_id,
                        item_type: item_type.clone(),
                    }
                    .into_active_model(),
                );
            }

            item_assignees::Entity::insert_many(new_item)
                .exec(&txn)
                .await?;
        }

        txn.commit().await?;

        Ok(())
    }

    pub async fn add_reference(
        &self,
        source_id: &str,
        target_id: &str,
        reference_type: ReferenceTypeEnum,
    ) -> Result<issue_cl_references::Model, MegaError> {
        let issue_ref = issue_cl_references::Model {
            source_id: source_id.to_owned(),
            target_id: target_id.to_owned(),
            reference_type,
            created_at: chrono::Utc::now().naive_utc(),
            updated_at: chrono::Utc::now().naive_utc(),
        };

        let res = issue_ref
            .into_active_model()
            .insert(self.get_connection())
            .await?;

        Ok(res)
    }
}
