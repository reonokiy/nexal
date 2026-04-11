use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m0004_cron_jobs"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(CronJob::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(CronJob::Id)
                            .string()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(CronJob::Label).string().not_null())
                    .col(ColumnDef::new(CronJob::Schedule).string().not_null())
                    .col(ColumnDef::new(CronJob::Message).text().not_null())
                    .col(ColumnDef::new(CronJob::TargetChannel).string().not_null())
                    .col(ColumnDef::new(CronJob::TargetChatId).string().not_null())
                    .col(
                        ColumnDef::new(CronJob::Context)
                            .text()
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(CronJob::Enabled)
                            .integer()
                            .not_null()
                            .default(1),
                    )
                    .col(ColumnDef::new(CronJob::LastRunAt).big_integer().null())
                    .col(ColumnDef::new(CronJob::CreatedAt).big_integer().not_null())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_cron_jobs_enabled")
                    .table(CronJob::Table)
                    .col(CronJob::Enabled)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(CronJob::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum CronJob {
    Table,
    Id,
    Label,
    Schedule,
    Message,
    TargetChannel,
    TargetChatId,
    Context,
    Enabled,
    LastRunAt,
    CreatedAt,
}
