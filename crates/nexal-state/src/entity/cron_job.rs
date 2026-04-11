use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "cron_jobs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub label: String,
    pub schedule: String,
    pub message: String,
    pub target_channel: String,
    pub target_chat_id: String,
    pub context: String,
    /// Stored as 0/1 integer in both SQLite and Postgres for compatibility.
    pub enabled: i32,
    pub last_run_at: Option<i64>,
    pub created_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
