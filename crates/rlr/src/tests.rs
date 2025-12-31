//!
// test RlrNetwork + RlrStorage + mem kv cache
#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use crate::node::AppStateMachineHandler;
    use crate::storage;
    use crate::storage::RlrLogStore;
    use crate::types::AppTypeConfig;
    use openraft::testing::log::{StoreBuilder, Suite};
    use openraft::{RaftTypeConfig, StorageError};
    use rocksdb::{ColumnFamilyDescriptor, DB, Options};
    use tempfile::TempDir;
    use tokio::io;
    use tracing_subscriber;

    struct TestSuiteBuilder {}

    async fn new_testee<C, P: AsRef<Path>>(
        db_path: P,
    ) -> Result<(RlrLogStore<C>, AppStateMachineHandler), io::Error>
    where
        C: RaftTypeConfig,
    {
        let mut db_opts = Options::default();
        db_opts.create_missing_column_families(true);
        db_opts.create_if_missing(true);

        let cfs = vec![
            ColumnFamilyDescriptor::new(storage::CF_META, Options::default()),
            ColumnFamilyDescriptor::new(storage::CF_LOGS, Options::default()),
            ColumnFamilyDescriptor::new(storage::CF_PURGE_ID, Options::default()),
            ColumnFamilyDescriptor::new(storage::CF_VOTE, Options::default()),
        ];

        let db_path = db_path.as_ref();
        let db = DB::open_cf_descriptors(&db_opts, db_path, cfs).map_err(io::Error::other)?;
        let db = Arc::new(db);

        let rlrStorage = RlrLogStore::new(db.clone());
        let mut appSm = AppStateMachineHandler::new(db_path.to_path_buf().join("snapshots"));
        // recover if needed
        // todo: AppStateMachine.recover()

        Ok((rlrStorage, appSm))
    }

    impl StoreBuilder<AppTypeConfig, RlrLogStore<AppTypeConfig>, AppStateMachineHandler, TempDir>
        for TestSuiteBuilder
    {
        async fn build(
            &self,
        ) -> Result<
            (TempDir, RlrLogStore<AppTypeConfig>, AppStateMachineHandler),
            StorageError<AppTypeConfig>,
        > {
            // create a temp dir in WORKING_DIR/tmp
            let td = TempDir::new_in("./tmp").unwrap();
            let (log_store, sm) = new_testee(td.path()).await.unwrap();
            Ok((td, log_store, sm))
        }
    }

    #[tokio::test]
    async fn test_store_correctness() {
        tracing_subscriber::fmt()
            .with_file(true)
            .with_line_number(true)
            .with_env_filter("info")
            .with_thread_ids(true)
            .init();

        let builder = TestSuiteBuilder {};
        match Suite::test_all(builder).await {
            Ok(_) => {}
            Err(e) => {
                panic!("test_store_correctness failed: {}", e);
            }
        }
    }
}
