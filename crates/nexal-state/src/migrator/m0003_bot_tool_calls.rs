use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m0003_bot_tool_calls"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(BotToolCall::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(BotToolCall::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(BotToolCall::SessionId).string().not_null())
                    .col(ColumnDef::new(BotToolCall::ToolCallId).string().not_null())
                    .col(ColumnDef::new(BotToolCall::ToolName).string().not_null())
                    .col(
                        ColumnDef::new(BotToolCall::Arguments)
                            .text()
                            .not_null()
                            .default("{}"),
                    )
                    .col(
                        ColumnDef::new(BotToolCall::Output)
                            .text()
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(BotToolCall::Status)
                            .string()
                            .not_null()
                            .default("ok"),
                    )
                    .col(ColumnDef::new(BotToolCall::DurationMs).big_integer().null())
                    .col(ColumnDef::new(BotToolCall::Timestamp).big_integer().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .from(BotToolCall::Table, BotToolCall::SessionId)
                            .to(BotSession::Table, BotSession::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_bot_tool_calls_session")
                    .table(BotToolCall::Table)
                    .col(BotToolCall::SessionId)
                    .col(BotToolCall::Timestamp)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(BotToolCall::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum BotToolCall {
    Table,
    Id,
    SessionId,
    ToolCallId,
    ToolName,
    Arguments,
    Output,
    Status,
    DurationMs,
    Timestamp,
}

#[derive(Iden)]
enum BotSession {
    Table,
    Id,
}
