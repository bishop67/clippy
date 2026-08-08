use sea_orm_migration::{
    prelude::*,
    schema::{string, string_null},
};

#[derive(Iden)]
enum Settings {
    Table,
    PasteOnSelect,
    PasteRestoreToken,
}

// "off" keeps the pre-existing copy-only behaviour for anyone upgrading.
const PASTE_ON_SELECT: &str = "off";

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // SQLite supports only one column per ALTER TABLE, so add each separately.
        manager
            .alter_table(
                Table::alter()
                    .table(Settings::Table)
                    .add_column(string(Settings::PasteOnSelect).default(PASTE_ON_SELECT))
                    .to_owned(),
            )
            .await?;

        // Handed back by the RemoteDesktop portal so Wayland users approve input
        // simulation once instead of on every paste.
        manager
            .alter_table(
                Table::alter()
                    .table(Settings::Table)
                    .add_column(string_null(Settings::PasteRestoreToken))
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Settings::Table)
                    .drop_column(Settings::PasteOnSelect)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Settings::Table)
                    .drop_column(Settings::PasteRestoreToken)
                    .to_owned(),
            )
            .await
    }
}
