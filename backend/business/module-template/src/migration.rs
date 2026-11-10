//! The module's own migration chain — independent of the admin
//! service's chain, so the two evolve without touching each other's
//! history. Same facility as the admin schema bootstrap
//! (`rushwind-storage-seaorm-migration`): the table DDL derives from
//! the entity definitions, applied once and tracked by name.

use rushwind_storage_seaorm_migration::{EntityTables, MigrationTrait, MigratorTrait};
use sea_orm::DatabaseBackend;

use crate::data;

pub struct Migrator;

impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        // The chain starter: the module's tables. Later columns land as
        // follow-up vintages (the admin chain's hand-written column
        // migrations — `impl MigrationName + MigrationTrait` with
        // `execute_unprepared` DDL — are the shape to copy), never by
        // editing this one.
        let init = EntityTables::new(
            "m20251110_000001_module_template_init",
            DatabaseBackend::Postgres,
        )
        .table::<data::notices::Entity>()
        .build();
        vec![Box::new(init)]
    }
}

/// Runs the module migration chain (the assembly binary calls this
/// after the admin chain, under the same `database.migrate` gate).
pub async fn run(db: &sea_orm::DatabaseConnection) -> Result<(), String> {
    Migrator::up(db, None).await.map_err(|e| e.to_string())?;
    Ok(())
}
