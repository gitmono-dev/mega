use std::ops::Deref;

use callisto::{artifact_objects, artifact_set_files, artifact_sets};
use common::errors::MegaError;
use sea_orm::{
    ColumnTrait, Condition, EntityTrait, ExprTrait, JoinType, QueryFilter, QueryOrder, QuerySelect,
    QueryTrait, RelationTrait, Value as SeaValue, sea_query::Expr,
};

use crate::storage::base_storage::{BaseStorage, StorageConnector};

pub struct ArtifactSetsPageQuery<'a> {
    pub repo: &'a str,
    pub namespace: &'a str,
    pub object_type: &'a str,
    pub run_id: Option<&'a str>,
    pub commit_sha: Option<&'a str>,
    pub cursor_before: Option<(chrono::NaiveDateTime, i64)>,
    pub limit_plus_one: u64,
}

#[derive(Clone)]
pub struct ArtifactStorage {
    pub base: BaseStorage,
}

impl Deref for ArtifactStorage {
    type Target = BaseStorage;
    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl ArtifactStorage {
    /// PostgreSQL only: `artifact_sets.metadata ->> 'key'` with `$n` placeholders
    /// (`cust_with_values` + `?` is not valid on Postgres).
    pub fn filter_list_by_metadata_json<'a>(
        query: sea_orm::Select<artifact_sets::Entity>,
        run_id: Option<&'a str>,
        commit_sha: Option<&'a str>,
    ) -> sea_orm::Select<artifact_sets::Entity> {
        let mut q = query;
        match (run_id, commit_sha) {
            (Some(run_id), Some(commit_sha)) => {
                q = q.filter(Expr::cust_with_values(
                    r#"(metadata ->> 'run_id') = $1 AND (metadata ->> 'commit_sha') = $2"#,
                    [
                        SeaValue::String(Some(run_id.to_owned())),
                        SeaValue::String(Some(commit_sha.to_owned())),
                    ],
                ));
            }
            (Some(run_id), None) => {
                q = q.filter(Expr::cust_with_values(
                    r#"(metadata ->> 'run_id') = $1"#,
                    [SeaValue::String(Some(run_id.to_owned()))],
                ));
            }
            (None, Some(commit_sha)) => {
                q = q.filter(Expr::cust_with_values(
                    r#"(metadata ->> 'commit_sha') = $1"#,
                    [SeaValue::String(Some(commit_sha.to_owned()))],
                ));
            }
            (None, None) => {}
        }
        q
    }

    /// PostgreSQL only: same as [`Self::filter_list_by_metadata_json`] for resolve joins.
    pub fn filter_resolve_by_metadata_json<'a>(
        query: sea_orm::SelectTwo<artifact_set_files::Entity, artifact_sets::Entity>,
        run_id: Option<&'a str>,
        commit_sha: Option<&'a str>,
    ) -> sea_orm::SelectTwo<artifact_set_files::Entity, artifact_sets::Entity> {
        let mut q = query;
        match (run_id, commit_sha) {
            (Some(run_id), Some(commit_sha)) => {
                q = q.filter(Expr::cust_with_values(
                    r#"(metadata ->> 'run_id') = $1 AND (metadata ->> 'commit_sha') = $2"#,
                    [
                        SeaValue::String(Some(run_id.to_owned())),
                        SeaValue::String(Some(commit_sha.to_owned())),
                    ],
                ));
            }
            (Some(run_id), None) => {
                q = q.filter(Expr::cust_with_values(
                    r#"(metadata ->> 'run_id') = $1"#,
                    [SeaValue::String(Some(run_id.to_owned()))],
                ));
            }
            (None, Some(commit_sha)) => {
                q = q.filter(Expr::cust_with_values(
                    r#"(metadata ->> 'commit_sha') = $1"#,
                    [SeaValue::String(Some(commit_sha.to_owned()))],
                ));
            }
            (None, None) => {}
        }
        q
    }

    pub async fn find_artifact_sets_page(
        &self,
        q: ArtifactSetsPageQuery<'_>,
    ) -> Result<Vec<artifact_sets::Model>, MegaError> {
        let mut query = artifact_sets::Entity::find()
            .filter(artifact_sets::Column::Repo.eq(q.repo))
            .filter(artifact_sets::Column::Namespace.eq(q.namespace))
            .filter(artifact_sets::Column::ObjectType.eq(q.object_type));

        query = Self::filter_list_by_metadata_json(query, q.run_id, q.commit_sha);

        if let Some((ca, id)) = q.cursor_before {
            query = query.filter(
                Condition::any()
                    .add(artifact_sets::Column::CreatedAt.lt(ca))
                    .add(
                        Condition::all()
                            .add(artifact_sets::Column::CreatedAt.eq(ca))
                            .add(artifact_sets::Column::Id.lt(id)),
                    ),
            );
        }

        let models = query
            .order_by_desc(artifact_sets::Column::CreatedAt)
            .order_by_desc(artifact_sets::Column::Id)
            .limit(Some(q.limit_plus_one))
            .all(self.get_connection())
            .await?;
        Ok(models)
    }

    pub async fn list_artifact_set_files_by_set_ids(
        &self,
        set_ids: &[i64],
    ) -> Result<Vec<artifact_set_files::Model>, MegaError> {
        if set_ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows = artifact_set_files::Entity::find()
            .filter(artifact_set_files::Column::SetId.is_in(set_ids.iter().copied()))
            .all(self.get_connection())
            .await?;
        Ok(rows)
    }

    pub async fn find_artifact_set_by_natural_key(
        &self,
        repo: &str,
        namespace: &str,
        object_type: &str,
        artifact_set_id: &str,
    ) -> Result<Option<artifact_sets::Model>, MegaError> {
        let m = artifact_sets::Entity::find()
            .filter(artifact_sets::Column::Repo.eq(repo))
            .filter(artifact_sets::Column::Namespace.eq(namespace))
            .filter(artifact_sets::Column::ObjectType.eq(object_type))
            .filter(artifact_sets::Column::ArtifactSetId.eq(artifact_set_id))
            .one(self.get_connection())
            .await?;
        Ok(m)
    }

    pub async fn list_artifact_set_files(
        &self,
        set_id: i64,
    ) -> Result<Vec<artifact_set_files::Model>, MegaError> {
        let rows = artifact_set_files::Entity::find()
            .filter(artifact_set_files::Column::SetId.eq(set_id))
            .all(self.get_connection())
            .await?;
        Ok(rows)
    }

    pub async fn find_latest_artifact_file_for_path(
        &self,
        repo: &str,
        namespace: &str,
        object_type: &str,
        path: &str,
        run_id: Option<&str>,
        commit_sha: Option<&str>,
    ) -> Result<Option<(artifact_set_files::Model, artifact_sets::Model)>, MegaError> {
        let mut query = artifact_set_files::Entity::find()
            .find_also_related(artifact_sets::Entity)
            .filter(artifact_set_files::Column::Path.eq(path))
            .filter(artifact_sets::Column::Repo.eq(repo))
            .filter(artifact_sets::Column::Namespace.eq(namespace))
            .filter(artifact_sets::Column::ObjectType.eq(object_type));

        query = Self::filter_resolve_by_metadata_json(query, run_id, commit_sha);

        let row = query
            .order_by_desc(artifact_sets::Column::CreatedAt)
            .order_by_desc(artifact_sets::Column::Id)
            .one(self.get_connection())
            .await?;
        Ok(row.and_then(|(f, s)| s.map(|set| (f, set))))
    }

    /// Load [`artifact_objects`] rows for the given `oid`s (any order; empty `oids` → empty vec).
    pub async fn find_artifact_objects_by_oids(
        &self,
        oids: &[String],
    ) -> Result<Vec<artifact_objects::Model>, MegaError> {
        if oids.is_empty() {
            return Ok(Vec::new());
        }
        let rows = artifact_objects::Entity::find()
            .filter(artifact_objects::Column::Oid.is_in(oids.iter().cloned()))
            .all(self.get_connection())
            .await?;
        Ok(rows)
    }

    /// `true` if `oid` appears in any committed manifest for `repo` (`artifact_set_files` ∩ `artifact_sets`).
    pub async fn artifact_oid_committed_in_repo(
        &self,
        repo: &str,
        oid: &str,
    ) -> Result<bool, MegaError> {
        let row = artifact_set_files::Entity::find()
            .join(
                JoinType::InnerJoin,
                artifact_set_files::Relation::ArtifactSets.def(),
            )
            .filter(artifact_sets::Column::Repo.eq(repo))
            .filter(artifact_set_files::Column::Oid.eq(oid))
            .one(self.get_connection())
            .await?;
        Ok(row.is_some())
    }

    pub async fn find_artifact_object_by_oid(
        &self,
        oid: &str,
    ) -> Result<Option<artifact_objects::Model>, MegaError> {
        Ok(artifact_objects::Entity::find_by_id(oid)
            .one(self.get_connection())
            .await?)
    }

    /// Set `last_seen_at` to now for the given `oid`s (e.g. `/batch` confirmed present, `/commit` success).
    pub async fn touch_artifact_objects_last_seen_at(
        &self,
        oids: &[String],
    ) -> Result<(), MegaError> {
        if oids.is_empty() {
            return Ok(());
        }
        let now = chrono::Utc::now().naive_utc();
        artifact_objects::Entity::update_many()
            .col_expr(artifact_objects::Column::LastSeenAt, Expr::value(now))
            .filter(artifact_objects::Column::Oid.is_in(oids.iter().cloned()))
            .exec(self.get_connection())
            .await?;
        Ok(())
    }

    /// `true` if any `artifact_set_files` row references `oid`.
    pub async fn artifact_set_files_references_oid(&self, oid: &str) -> Result<bool, MegaError> {
        let row = artifact_set_files::Entity::find()
            .filter(artifact_set_files::Column::Oid.eq(oid))
            .one(self.get_connection())
            .await?;
        Ok(row.is_some())
    }

    /// Rows eligible for blob GC: no manifest references and `last_seen_at` strictly before the cutoff.
    ///
    /// **PostgreSQL only.** See `docs/artifacts-protocol.md` §10.6 (`NOT EXISTS` on `artifact_set_files`).
    pub async fn list_gc_unreferenced_artifact_objects(
        &self,
        last_seen_before: chrono::NaiveDateTime,
        limit: u64,
    ) -> Result<Vec<artifact_objects::Model>, MegaError> {
        let conn = self.get_connection();

        let no_manifest_row = Expr::exists(
            artifact_set_files::Entity::find()
                .select_only()
                .column(artifact_set_files::Column::Oid)
                .filter(
                    Expr::col((artifact_set_files::Entity, artifact_set_files::Column::Oid)).eq(
                        Expr::col((artifact_objects::Entity, artifact_objects::Column::Oid)),
                    ),
                )
                .into_query(),
        )
        .not();

        artifact_objects::Entity::find()
            .filter(artifact_objects::Column::LastSeenAt.lt(last_seen_before))
            .filter(no_manifest_row)
            .order_by_asc(artifact_objects::Column::LastSeenAt)
            .limit(std::cmp::max(limit, 1))
            .all(conn)
            .await
            .map_err(MegaError::Db)
    }

    /// Remove the `artifact_objects` row for `oid` (call after object-store delete succeeds).
    pub async fn delete_artifact_object_row(&self, oid: &str) -> Result<(), MegaError> {
        artifact_objects::Entity::delete_many()
            .filter(artifact_objects::Column::Oid.eq(oid))
            .exec(self.get_connection())
            .await?;
        Ok(())
    }
}
