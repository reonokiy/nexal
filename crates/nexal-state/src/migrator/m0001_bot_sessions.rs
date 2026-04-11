use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m0001_bot_sessions"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(BotSession::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(BotSession::Id)
                            .string()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(BotSession::Channel).string().not_null())
                    .col(ColumnDef::new(BotSession::ChatId).string().not_null())
                    .col(ColumnDef::new(BotSession::ThreadId).string().null())
                    .col(ColumnDef::new(BotSession::CreatedAt).big_integer().not_null())
                    .col(ColumnDef::new(BotSession::UpdatedAt).big_integer().not_null())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_bot_sessions_channel_chat")
                    .table(BotSession::Table)
                    .col(BotSession::Channel)
                    .col(BotSession::ChatId)
                    .unique()
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(BotSession::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum BotSession {
    Table,
    Id,
    Channel,
    ChatId,
    ThreadId,
    CreatedAt,
    UpdatedAt,
}
