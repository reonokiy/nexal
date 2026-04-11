use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "bot_messages")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub session_id: String,
    pub sender: String,
    pub role: String,
    pub text: String,
    pub timestamp: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
