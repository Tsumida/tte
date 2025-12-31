//!
// test RlrNetwork + RlrStorage + mem kv cache
#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::Arc;

    use crate::node::{AppStateMachine, AppStateMachineHandler};
    use crate::storage::RlrLogStore;
    use crate::storage::{self};
    use crate::types::AppTypeConfig;
    use openraft::testing::log::{StoreBuilder, Suite};
    use openraft::{RaftTypeConfig, StorageError};
    use rocksdb::{ColumnFamilyDescriptor, DB, Options};
    use tempfile::TempDir;
    use tokio::io;
    use tracing_subscriber;

    struct TestSuiteBuilder {}

    #[derive(Default)]
    struct HashMapInner {
        map: HashMap<String, String>,
    }

    impl Into<Vec<u8>> for HashMapInner {
        fn into(self) -> Vec<u8> {
            serde_json::to_vec(&self.map).unwrap()
        }
    }

    impl From<Vec<u8>> for HashMapInner {
        fn from(data: Vec<u8>) -> Self {
            let map: HashMap<String, String> = serde_json::from_slice(&data).unwrap();
            HashMapInner { map }
        }
    }

    impl AppStateMachine for HashMapInner {
        fn apply(
            &mut self,
            req: &crate::types::AppStateMachineInput,
        ) -> anyhow::Result<crate::types::AppStateMachineOutput> {
            let key = serde_json::from_slice::<String>(&req.0)?;
            self.map.insert(key.clone(), key.clone());
            Ok(crate::types::AppStateMachineOutput(b"ok".to_vec()))
        }

        fn recover(
            &mut self,
            snapshot: &openraft::alias::SnapshotDataOf<AppTypeConfig>,
        ) -> anyhow::Result<()> {
            let data: HashMap<String, String> = serde_json::from_slice(&snapshot.get_ref())?;

            self.map = data;
            Ok(())
        }

        fn take_snapshot(&self) -> Vec<u8> {
            serde_json::to_vec(&self.map).unwrap()
        }

        fn from_snapshot(data: Vec<u8>) -> Self {
            let map: HashMap<String, String> = serde_json::from_slice(&data).unwrap();
            HashMapInner { map }
        }
    }

    async fn new_testee<C, P: AsRef<Path>>(
        db_path: P,
    ) -> Result<(RlrLogStore<C>, AppStateMachineHandler<HashMapInner>), io::Error>
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

        let rlr_storage = RlrLogStore::new(db.clone());
        let snapshot_dir = db_path.to_path_buf().join("snapshots");

        // create snapshot dir if not exists
        if !snapshot_dir.exists() {
            tokio::fs::create_dir_all(&snapshot_dir).await?;
        }

        let app_statemachine = AppStateMachineHandler::new(snapshot_dir.clone());
        // let ssm = DefaultSnapshotManager::new("test", snapshot_dir);
        // match ssm.load_latest_snapshot().await {
        //     Ok(Some(snap)) => {
        //         app_statemachine
        //             .install_snapshot(&snap.meta, snap.snapshot)
        //             .await?;
        //     }
        //     Ok(None) => {
        //         tracing::info!("no snapshot found");
        //     }
        //     Err(e) => {
        //         return Err(io::Error::new(
        //             io::ErrorKind::Other,
        //             format!("load snapshot error: {}", e),
        //         ));
        //     }
        // }

        Ok((rlr_storage, app_statemachine))
    }

    impl
        StoreBuilder<
            AppTypeConfig,
            RlrLogStore<AppTypeConfig>,
            AppStateMachineHandler<HashMapInner>,
            TempDir,
        > for TestSuiteBuilder
    {
        async fn build(
            &self,
        ) -> Result<
            (
                TempDir,
                RlrLogStore<AppTypeConfig>,
                AppStateMachineHandler<HashMapInner>,
            ),
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
