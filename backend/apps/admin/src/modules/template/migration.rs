//! The module's own migration chain 鈥?independent of the admin
//! service's chain, so the two evolve without touching each other's
//! history. Same facility as the admin schema bootstrap
//! (`rushwind-storage-seaorm-migration`): the table DDL derives from
//! the entity definitions, applied once and tracked by name.

use rushwind_storage_seaorm_migration::{EntityTables, MigrationTrait, MigratorTrait};
use sea_orm::DatabaseBackend;

use super::data;

pub struct Migrator;

impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        // The chain starter: the module's tables. Later columns land as
        // follow-up vintages (the admin chain's hand-written column
        // migrations 鈥?`impl MigrationName + MigrationTrait` with
        // `execute_unprepared` DDL 鈥?are the shape to copy), never by
        // editing this one.
        let init = EntityTables::new(
            "m20251110_000001_module_template_init",
            DatabaseBackend::Postgres,
        )
        .table::<data::notices::Entity>()
        .build();
        vec![Box::new(init)]
    }

    // 独立追踪表：默认的 `seaql_migrations` 是 admin 主链的账本——两条链
    // 共用一张表时，各自的"已应用集合"互为对方的缺失项，模板链在 admin
    // 已迁移过的库上启动即报"applied but its file is missing"。按本文件
    // 头部声明的链独立性，每个 fork 模块的迁移链记自己的账本。
    fn migration_table_name() -> sea_orm::sea_query::DynIden {
        use sea_orm::sea_query::IntoIden;
        sea_orm::sea_query::Alias::new("seaql_migrations_module_template").into_iden()
    }
}

/// Runs the module migration chain (the assembly binary calls this
/// after the admin chain, under the same `database.migrate` gate).
pub async fn run(db: &sea_orm::DatabaseConnection) -> Result<(), String> {
    Migrator::up(db, None).await.map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 模块链必须记自己的账本：默认的 `seaql_migrations` 是 admin 主链的
    /// 追踪表，两条链共表时各自的已应用集合互为对方的缺失项——模板链在
    /// admin 已迁移过的库上启动即报 "applied but its file is missing"
    ///（真启动事故的回归钉）。
    #[test]
    fn module_chain_keeps_its_own_ledger_table() {
        let ledger = <Migrator as MigratorTrait>::migration_table_name().to_string();
        assert_eq!(ledger, "seaql_migrations_module_template");
        // 链内迁移名一律带模块命名空间，与 admin 链的版本号零交集。
        for m in Migrator::migrations() {
            assert!(
                m.name().starts_with("m20251110_000001_module_template"),
                "migration name {} must be module-namespaced",
                m.name()
            );
        }
    }
}
