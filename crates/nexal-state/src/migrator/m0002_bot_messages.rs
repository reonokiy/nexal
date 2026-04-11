use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m0002_bot_messages"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(BotMessage::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(BotMessage::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(BotMessage::SessionId).string().not_null())
                    .col(ColumnDef::new(BotMessage::Sender).string().not_null())
                    .col(ColumnDef::new(BotMessage::Role).string().not_null())
                    .col(ColumnDef::new(BotMessage::Text).text().not_null())
                    .col(ColumnDef::new(BotMessage::Timestamp).big_integer().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .from(BotMessage::Table, BotMessage::SessionId)
                            .to(BotSession::Table, BotSession::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_bot_messages_session")
                    .table(BotMessage::Table)
                    .col(BotMessage::SessionId)
                    .col(BotMessage::Timestamp)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(BotMessage::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum BotMessage {
    Table,
    Id,
    SessionId,
    Sender,
    Role,
    Text,
    Timestamp,
}

#[derive(Iden)]
enum BotSession {
    Table,
    Id,
}
