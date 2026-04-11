use sea_orm_migration::prelude::*;

mod m0001_bot_sessions;
mod m0002_bot_messages;
mod m0003_bot_tool_calls;
mod m0004_cron_jobs;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m0001_bot_sessions::Migration),
            Box::new(m0002_bot_messages::Migration),
            Box::new(m0003_bot_tool_calls::Migration),
            Box::new(m0004_cron_jobs::Migration),
        ]
    }
}
