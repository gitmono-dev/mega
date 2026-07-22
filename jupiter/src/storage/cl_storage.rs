use std::{collections::HashMap, ops::Deref};

use api_model::common::Pagination;
use callisto::{
    check_result, item_assignees, label, mega_cl, mega_conversation, path_check_configs,
    sea_orm_active_enums::MergeStatusEnum,
};
use common::errors::MegaError;
use git_internal::internal::object::commit::Commit;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, EntityTrait, IntoActiveModel, JoinType,
    PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, QueryTrait, RelationTrait, Set,
    prelude::Expr, sea_query::OnConflict,
};

use crate::{
    model::common::{ItemDetails, ListParams},
    storage::{
        base_storage::{BaseStorage, StorageConnector},
        stg_common::{
            combine_item_list,
            query_build::{apply_sort, filter_by_assignees, filter_by_labels},
        },
    },
};

#[derive(Clone)]
pub struct ClStorage {
    pub base: BaseStorage,
}

impl Deref for ClStorage {
    type Target = BaseStorage;
    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl ClStorage {
    pub async fn get_open_cl_by_path(
        &self,
        path: &str,
        username: &str,
    ) -> Result<Option<mega_cl::Model>, MegaError> {
        let model = mega_cl::Entity::find()
            .filter(mega_cl::Column::Path.eq(path))
            .filter(mega_cl::Column::Username.eq(username))
            .filter(mega_cl::Column::Status.eq(MergeStatusEnum::Open))
            .one(self.get_connection())
            .await
            .unwrap();
        Ok(model)
    }

    pub async fn get_open_cls_by_path_prefix(
        &self,
        path_prefix: &str,
    ) -> Result<Vec<mega_cl::Model>, MegaError> {
        let models = mega_cl::Entity::find()
            .filter(mega_cl::Column::Path.starts_with(path_prefix))
            .filter(mega_cl::Column::Status.eq(MergeStatusEnum::Open))
            .all(self.get_connection())
            .await?;
        Ok(models)
    }

    pub async fn get_open_cls(&self) -> Result<Vec<mega_cl::Model>, MegaError> {
        let models = mega_cl::Entity::find()
            .filter(mega_cl::Column::Status.eq(MergeStatusEnum::Open))
            .all(self.get_connection())
            .await?;
        Ok(models)
    }

    pub async fn get_cl_list(
        &self,
        params: ListParams,
        page: Pagination,
    ) -> Result<(Vec<ItemDetails>, u64), MegaError> {
        let cond = Condition::all();
        let cond = filter_by_labels(cond, params.labels);
        let cond = filter_by_assignees(cond, params.assignees);

        let status = if params.status == "open" {
            vec![MergeStatusEnum::Open, MergeStatusEnum::Draft]
        } else if params.status == "closed" {
            vec![MergeStatusEnum::Closed, MergeStatusEnum::Merged]
        } else {
            vec![
                MergeStatusEnum::Open,
                MergeStatusEnum::Closed,
                MergeStatusEnum::Merged,
                MergeStatusEnum::Draft,
            ]
        };

        let base_query = mega_cl::Entity::find()
            .join(
                JoinType::LeftJoin,
                callisto::entity_ext::mega_cl::Relation::ItemLabels.def(),
            )
            .join(
                JoinType::LeftJoin,
                callisto::entity_ext::mega_cl::Relation::ItemAssignees.def(),
            )
            .filter(mega_cl::Column::Status.is_in(status))
            .apply_if(params.author, |q, author| {
                q.filter(mega_cl::Column::Username.eq(author))
            })
            .filter(cond)
            .distinct();

        let mut sort_map = HashMap::new();
        sort_map.insert("created_at", mega_cl::Column::CreatedAt);
        sort_map.insert("updated_at", mega_cl::Column::UpdatedAt);

        let sort_field = params.sort_by.as_deref();
        let has_valid_sort = sort_field.and_then(|field| sort_map.get(field)).is_some();

        let mut sorted_query = apply_sort(base_query, sort_field, params.asc, &sort_map);

        if !has_valid_sort {
            sorted_query = sorted_query.order_by_desc(mega_cl::Column::Id);
        }

        let paginator = sorted_query.paginate(self.get_connection(), page.per_page);
        let total = paginator.num_items().await?;

        let cl_list = paginator.fetch_page(page.page - 1).await?;

        if cl_list.is_empty() {
            return Ok((vec![], 0));
        }

        let ids = cl_list.iter().map(|m| m.id).collect::<Vec<_>>();

        let label_query = mega_cl::Entity::find().filter(mega_cl::Column::Id.is_in(ids.clone()));
        let label_query = apply_sort(
            label_query,
            params.sort_by.as_deref(),
            params.asc,
            &sort_map,
        );
        let labels: Vec<(mega_cl::Model, Vec<label::Model>)> = label_query
            .find_with_related(label::Entity)
            .all(self.get_connection())
            .await?;

        let assignees: Vec<(mega_cl::Model, Vec<item_assignees::Model>)> = mega_cl::Entity::find()
            .filter(mega_cl::Column::Id.is_in(ids.clone()))
            .find_with_related(item_assignees::Entity)
            .all(self.get_connection())
            .await?;

        let conversations: Vec<(mega_cl::Model, Vec<mega_conversation::Model>)> =
            mega_cl::Entity::find()
                .filter(mega_cl::Column::Id.is_in(ids))
                .find_with_related(mega_conversation::Entity)
                .all(self.get_connection())
                .await?;

        let res = combine_item_list::<mega_cl::Entity>(labels, assignees, conversations);

        Ok((res, total))
    }

    pub async fn get_cl_suggestions_by_query(
        &self,
        query: &str,
    ) -> Result<Vec<mega_cl::Model>, MegaError> {
        let keyword = format!("%{query}%");
        let res = mega_cl::Entity::find()
            .filter(
                Condition::any()
                    .add(mega_cl::Column::Link.like(&keyword))
                    .add(mega_cl::Column::Title.like(&keyword)),
            )
            .limit(5)
            .all(self.get_connection())
            .await?;
        Ok(res)
    }

    pub async fn get_cl(&self, link: &str) -> Result<Option<mega_cl::Model>, MegaError> {
        let model = mega_cl::Entity::find()
            .filter(mega_cl::Column::Link.eq(link))
            .one(self.get_connection())
            .await?;
        Ok(model)
    }

    pub async fn get_cl_labels(
        &self,
        link: &str,
    ) -> Result<Option<(mega_cl::Model, Vec<label::Model>)>, MegaError> {
        let labels: Vec<(mega_cl::Model, Vec<label::Model>)> = mega_cl::Entity::find()
            .filter(mega_cl::Column::Link.eq(link))
            .find_with_related(label::Entity)
            .all(self.get_connection())
            .await?;
        Ok(labels.first().cloned())
    }

    pub async fn get_cl_assignees(
        &self,
        link: &str,
    ) -> Result<Option<(mega_cl::Model, Vec<item_assignees::Model>)>, MegaError> {
        let assignees: Vec<(mega_cl::Model, Vec<item_assignees::Model>)> = mega_cl::Entity::find()
            .filter(mega_cl::Column::Link.eq(link))
            .find_with_related(item_assignees::Entity)
            .all(self.get_connection())
            .await?;
        Ok(assignees.first().cloned())
    }

    pub async fn is_assignee(&self, link: &str, username: &str) -> Result<(), MegaError> {
        let assignee = mega_cl::Entity::find()
            .filter(mega_cl::Column::Link.eq(link))
            .find_with_related(item_assignees::Entity)
            .filter(item_assignees::Column::AssignneeId.eq(username))
            .all(self.get_connection())
            .await?;
        if assignee.is_empty() {
            return Err(MegaError::Other("Not an assignee".to_string()));
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn new_cl(
        &self,
        path: &str,
        link: &str,
        title: &str,
        base_branch: &str,
        from_hash: &str,
        to_hash: &str,
        username: &str,
    ) -> Result<String, MegaError> {
        self.new_cl_model(path, link, title, base_branch, from_hash, to_hash, username)
            .await
            .map(|res| res.link)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn new_cl_model(
        &self,
        path: &str,
        link: &str,
        title: &str,
        base_branch: &str,
        from_hash: &str,
        to_hash: &str,
        username: &str,
    ) -> Result<mega_cl::Model, MegaError> {
        let model = mega_cl::Model::new(
            path.to_owned(),
            title.to_owned(),
            link.to_owned(),
            base_branch.to_owned(),
            from_hash.to_owned(),
            to_hash.to_owned(),
            username.to_owned(),
        );
        let res = model
            .into_active_model()
            .insert(self.get_connection())
            .await?;
        Ok(res)
    }

    /// Create a new CL with Draft status (for Buck upload)
    pub async fn new_cl_draft(
        &self,
        path: &str,
        link: &str,
        title: &str,
        base_branch: &str,
        from_hash: &str,
        username: &str,
    ) -> Result<String, MegaError> {
        let model = mega_cl::Model::new_draft(
            path.to_owned(),
            title.to_owned(),
            link.to_owned(),
            base_branch.to_owned(),
            from_hash.to_owned(),
            username.to_owned(),
        );
        let res = model
            .into_active_model()
            .insert(self.get_connection())
            .await?;
        Ok(res.link)
    }

    pub async fn edit_title(&self, link: &str, title: &str) -> Result<(), MegaError> {
        mega_cl::Entity::update_many()
            .col_expr(mega_cl::Column::Title, Expr::value(title))
            .col_expr(
                mega_cl::Column::UpdatedAt,
                Expr::value(chrono::Utc::now().naive_utc()),
            )
            .filter(mega_cl::Column::Link.eq(link))
            .exec(self.get_connection())
            .await?;
        Ok(())
    }

    pub async fn close_cl(&self, model: mega_cl::Model) -> Result<(), MegaError> {
        let mut a_model = model.into_active_model();
        a_model.status = Set(MergeStatusEnum::Closed);
        a_model.updated_at = Set(chrono::Utc::now().naive_utc());
        a_model.update(self.get_connection()).await.unwrap();
        Ok(())
    }

    pub async fn reopen_cl(&self, model: mega_cl::Model) -> Result<(), MegaError> {
        let mut a_model = model.into_active_model();
        a_model.status = Set(MergeStatusEnum::Open);
        a_model.updated_at = Set(chrono::Utc::now().naive_utc());
        a_model.update(self.get_connection()).await.unwrap();
        Ok(())
    }

    pub async fn update_cl_status(
        &self,
        model: mega_cl::Model,
        status: MergeStatusEnum,
    ) -> Result<(), MegaError> {
        let mut a_model = model.into_active_model();
        a_model.status = Set(status);
        a_model.updated_at = Set(chrono::Utc::now().naive_utc());
        a_model.update(self.get_connection()).await?;
        Ok(())
    }

    pub async fn merge_cl(&self, model: mega_cl::Model) -> Result<(), MegaError> {
        let mut a_model = model.into_active_model();
        a_model.status = Set(MergeStatusEnum::Merged);
        a_model.updated_at = Set(chrono::Utc::now().naive_utc());
        a_model.update(self.get_connection()).await.unwrap();
        Ok(())
    }

    pub async fn update_cl_to_hash(
        &self,
        model: mega_cl::Model,
        to_hash: &str,
    ) -> Result<(), MegaError> {
        let mut a_model = model.into_active_model();
        a_model.to_hash = Set(to_hash.to_owned());
        a_model.updated_at = Set(chrono::Utc::now().naive_utc());
        a_model.update(self.get_connection()).await.unwrap();
        Ok(())
    }

    pub async fn update_cl_hash(
        &self,
        model: mega_cl::Model,
        from_hash: &str,
        to_hash: &str,
    ) -> Result<(), MegaError> {
        let mut a_model = model.into_active_model();
        a_model.from_hash = Set(from_hash.to_owned());
        a_model.to_hash = Set(to_hash.to_owned());
        a_model.updated_at = Set(chrono::Utc::now().naive_utc());
        a_model.update(self.get_connection()).await.unwrap();
        Ok(())
    }

    pub async fn update_cl_title(
        &self,
        model: mega_cl::Model,
        title: &str,
    ) -> Result<(), MegaError> {
        let mut a_model = model.into_active_model();
        a_model.title = Set(title.to_owned());
        a_model.updated_at = Set(chrono::Utc::now().naive_utc());
        a_model.update(self.get_connection()).await?;
        Ok(())
    }

    pub async fn get_checks_config_by_path(
        &self,
        _: &str,
    ) -> Result<Vec<path_check_configs::Model>, MegaError> {
        let models = path_check_configs::Entity::find()
            // .filter(path_check_configs::Column::Path.eq(path))
            .filter(path_check_configs::Column::Enabled.eq(true))
            .all(self.get_connection())
            .await?;
        Ok(models)
    }

    pub async fn save_check_results(
        &self,
        models: Vec<check_result::Model>,
    ) -> Result<(), MegaError> {
        let models: Vec<check_result::ActiveModel> =
            models.into_iter().map(|m| m.into_active_model()).collect();
        check_result::Entity::insert_many(models)
            .on_conflict(
                OnConflict::columns(vec![
                    check_result::Column::ClLink,
                    check_result::Column::CheckTypeCode,
                ])
                .update_columns([
                    check_result::Column::CommitId,
                    check_result::Column::Status,
                    check_result::Column::Message,
                ])
                .to_owned(),
            )
            .try_insert()
            .exec(self.get_connection())
            .await?;
        Ok(())
    }

    pub async fn get_check_result(
        &self,
        cl_link: &str,
    ) -> Result<Vec<check_result::Model>, MegaError> {
        let models = check_result::Entity::find()
            .filter(check_result::Column::ClLink.eq(cl_link))
            .all(self.get_connection())
            .await?;
        Ok(models)
    }

    pub async fn save_cl_commits(&self, link: &str, commits: Vec<Commit>) -> Result<(), MegaError> {
        let mut save_models = vec![];
        for commit in commits {
            let model = callisto::mega_cl_commits::ActiveModel {
                cl_link: Set(link.to_string()),
                commit_sha: Set(commit.id.to_string()),
                author_name: Set(commit.author.name.clone()),
                author_email: Set(commit.author.email.clone()),
                message: Set(commit.format_message()),
                created_at: Set(chrono::Utc::now().naive_utc()),
                updated_at: Set(chrono::Utc::now().naive_utc()),
            };
            save_models.push(model);
        }
        self.batch_save_model(save_models).await?;
        Ok(())
    }
}
