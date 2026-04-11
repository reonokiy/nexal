use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "bot_tool_calls")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub session_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub arguments: String,
    pub output: String,
    pub status: String,
    pub duration_ms: Option<i64>,
    pub timestamp: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
