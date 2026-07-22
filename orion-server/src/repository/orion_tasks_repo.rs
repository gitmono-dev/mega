use api_model::buck2::{status::Status, types::ProjectRelativePath};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DbErr, EntityTrait, IntoActiveModel,
    QueryFilter as _, QueryOrder as _, QuerySelect as _,
};
use serde_json::to_value;
use uuid::Uuid;

pub struct OrionTasksRepo;

impl OrionTasksRepo {
    pub async fn ping(conn: &impl ConnectionTrait) -> Result<(), DbErr> {
        let _ = callisto::orion_tasks::Entity::find()
            .limit(1)
            .all(conn)
            .await?;
        Ok(())
    }

    pub async fn exists_by_id(conn: &impl ConnectionTrait, id: Uuid) -> Result<bool, DbErr> {
        Ok(callisto::orion_tasks::Entity::find_by_id(id)
            .one(conn)
            .await?
            .is_some())
    }

    pub async fn find_by_id(
        conn: &impl ConnectionTrait,
        id: Uuid,
    ) -> Result<Option<callisto::orion_tasks::Model>, DbErr> {
        callisto::orion_tasks::Entity::find_by_id(id)
            .one(conn)
            .await
    }

    pub async fn find_by_cl(
        conn: &impl ConnectionTrait,
        cl: &str,
    ) -> Result<Vec<callisto::orion_tasks::Model>, DbErr> {
        callisto::orion_tasks::Entity::find()
            .filter(callisto::orion_tasks::Column::Cl.eq(cl))
            .order_by_asc(callisto::orion_tasks::Column::CreatedAt)
            .all(conn)
            .await
    }

    fn create_task_model(
        task_id: Uuid,
        cl_link: &str,
        repo: &str,
        changes: &Vec<Status<ProjectRelativePath>>,
    ) -> Result<callisto::orion_tasks::Model, serde_json::Error> {
        Ok(callisto::orion_tasks::Model {
            id: task_id,
            cl: cl_link.to_string(),
            repo_name: repo.to_string(),
            changes: to_value(changes)?,
            created_at: chrono::Utc::now().into(),
        })
    }

    pub async fn insert_task(
        task_id: Uuid,
        cl_link: &str,
        repo: &str,
        changes: &Vec<Status<ProjectRelativePath>>,
        db: &impl ConnectionTrait,
    ) -> Result<callisto::orion_tasks::Model, DbErr> {
        let task_model = Self::create_task_model(task_id, cl_link, repo, changes)
            .map_err(|e| DbErr::Custom(e.to_string()))?;
        task_model.into_active_model().insert(db).await
    }
}
