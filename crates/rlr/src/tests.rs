//!

// test RlrNetwork + RlrStorage + mem kv cache

#[cfg(test)]
mod mem {
    use std::{collections::HashMap, io::Cursor, sync::Arc};

    use futures::TryStreamExt;
    use openraft::{
        Membership, Raft, RaftSnapshotBuilder, RaftTypeConfig, Snapshot, alias::SnapshotDataOf,
        storage::RaftStateMachine,
    };
    use tokio::sync::RwLock;

    use crate::{
        tests::mem,
        types::{AppResponse, AppTypeConfig},
    };
    pub(crate) struct MemHashMap {
        // "index" -> "str"
        map: Arc<RwLock<HashMap<String, String>>>,
        last_applied_log_id: Option<openraft::alias::LogIdOf<AppTypeConfig>>,
        last_membership: openraft::StoredMembership<AppTypeConfig>,
        current_snapshot: Option<Snapshot<AppTypeConfig>>,
    }

    impl MemHashMap {
        pub(crate) fn new() -> Self {
            Self {
                map: Arc::new(RwLock::new(HashMap::new())),
                last_applied_log_id: None,
                last_membership: openraft::StoredMembership::new(None, Membership::default()),
                current_snapshot: None,
            }
        }
    }

    pub(crate) struct MemSnapshotBuilder<C: RaftTypeConfig> {
        _phantom: std::marker::PhantomData<C>,
        last_applied_log_id: Option<openraft::alias::LogIdOf<C>>,
        last_membership: openraft::Membership<C>,
        data: Vec<u8>,
    }

    impl RaftSnapshotBuilder<AppTypeConfig> for MemSnapshotBuilder<AppTypeConfig> {
        async fn build_snapshot(&mut self) -> Result<Snapshot<AppTypeConfig>, std::io::Error> {
            let rand_snapshot_id = format!("ss{}", uuid::Uuid::new_v4());
            let snapshot = Snapshot {
                meta: openraft::SnapshotMeta {
                    snapshot_id: rand_snapshot_id,
                    last_log_id: self.last_applied_log_id,
                    last_membership: openraft::StoredMembership::new(
                        self.last_applied_log_id,
                        self.last_membership.clone(),
                    ),
                },
                snapshot: Cursor::new(self.data.clone()),
            };
            Ok(snapshot)
        }
    }

    impl RaftStateMachine<AppTypeConfig> for MemHashMap {
        type SnapshotBuilder = MemSnapshotBuilder<AppTypeConfig>;

        async fn applied_state(
            &mut self,
        ) -> Result<
            (
                Option<openraft::alias::LogIdOf<AppTypeConfig>>,
                openraft::StoredMembership<AppTypeConfig>,
            ),
            std::io::Error,
        > {
            Ok((self.last_applied_log_id, self.last_membership.clone()))
        }

        async fn apply<EntryStream>(
            &mut self,
            mut entries: EntryStream,
        ) -> Result<(), std::io::Error>
        where
            EntryStream: futures::Stream<
                    Item = Result<openraft::storage::EntryResponder<AppTypeConfig>, std::io::Error>,
                > + Unpin
                + openraft::OptionalSend,
        {
            while let Some((entry, responder)) = entries.try_next().await? {
                // AppEntry -> StateMachine -> AppResponse, Openraft不关心
                let rsp = match &entry.payload {
                    openraft::EntryPayload::Normal(data) => {
                        let s = String::from_utf8(data.0.clone()).unwrap();
                        let key = format!("{}", entry.log_id.index);
                        self.map.write().await.insert(key, s);
                        AppResponse(b"kv updated".to_vec())
                    }
                    openraft::EntryPayload::Membership(m) => {
                        self.last_membership =
                            openraft::StoredMembership::new(Some(entry.log_id), m.clone());
                        AppResponse(b"ok".to_vec())
                    }
                    // ignore blank
                    openraft::EntryPayload::Blank => AppResponse(Vec::new()),
                };
                self.last_applied_log_id = Some(entry.log_id);
                if let Some(r) = responder {
                    let _ = r.send(rsp);
                }
            }
            Ok(())
        }

        async fn get_snapshot_builder(&mut self) -> Self::SnapshotBuilder {
            let data = {
                let map = self.map.read().await;
                serde_json::to_vec(&*map).unwrap()
            };
            MemSnapshotBuilder {
                _phantom: std::marker::PhantomData,
                data,
                last_applied_log_id: self.last_applied_log_id,
                last_membership: self.last_membership.membership().clone(),
            }
        }

        async fn begin_receiving_snapshot(
            &mut self,
        ) -> Result<SnapshotDataOf<AppTypeConfig>, std::io::Error> {
            Ok(Cursor::new(Vec::new()))
        }

        async fn install_snapshot(
            &mut self,
            meta: &openraft::SnapshotMeta<AppTypeConfig>,
            snapshot: <AppTypeConfig as RaftTypeConfig>::SnapshotData,
        ) -> Result<(), std::io::Error> {
            self.map.write().await.clear();
            match serde_json::from_slice(snapshot.get_ref()) {
                Ok(m) => {
                    self.map = Arc::new(RwLock::new(m));
                }
                Err(e) => {
                    return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e));
                }
            }

            self.current_snapshot = Some(Snapshot {
                meta: meta.clone(),
                snapshot,
            });

            Ok(())
        }

        async fn get_current_snapshot(
            &mut self,
        ) -> Result<Option<openraft::storage::Snapshot<AppTypeConfig>>, std::io::Error> {
            Ok(self.current_snapshot.clone())
        }
    }

    type MemRaftNode = Raft<AppTypeConfig>;
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::Path;
    use std::sync::Arc;

    use crate::tests::mem;
    use crate::types::AppTypeConfig;
    use crate::{storage::RlrLogStore, tests::mem::MemHashMap};
    use openraft::testing::log::{StoreBuilder, Suite};
    use openraft::{Membership, RaftTypeConfig, StorageError};
    use rocksdb::{ColumnFamilyDescriptor, DB, Options};
    use tempfile::TempDir;
    use tokio::io;

    struct TestSuiteBuilder {}

    async fn new_testee<C, P: AsRef<Path>>(
        db_path: P,
    ) -> Result<(RlrLogStore<C>, MemHashMap), io::Error>
    where
        C: RaftTypeConfig,
    {
        let mut db_opts = Options::default();
        db_opts.create_missing_column_families(true);
        db_opts.create_if_missing(true);

        let meta = ColumnFamilyDescriptor::new("meta", Options::default());
        let sm_meta = ColumnFamilyDescriptor::new("sm_meta", Options::default());
        let sm_data = ColumnFamilyDescriptor::new("sm_data", Options::default());
        let logs = ColumnFamilyDescriptor::new("logs", Options::default());

        let db_path = db_path.as_ref();
        // let snapshot_dir = db_path.join("snapshots");

        let db = DB::open_cf_descriptors(&db_opts, db_path, vec![meta, sm_meta, sm_data, logs])
            .map_err(io::Error::other)?;

        let db = Arc::new(db);
        Ok((RlrLogStore::new(db.clone()), MemHashMap::new()))
    }

    impl StoreBuilder<AppTypeConfig, RlrLogStore<AppTypeConfig>, MemHashMap> for TestSuiteBuilder {
        async fn build(
            &self,
        ) -> Result<((), RlrLogStore<AppTypeConfig>, MemHashMap), StorageError<AppTypeConfig>>
        {
            // create a temp dir in WORKING_DIR/tmp
            let td = TempDir::new_in("./tmp").unwrap();
            let (log_store, sm) = new_testee(td.path()).await.unwrap();
            Ok(((), log_store, sm))
        }
    }

    #[tokio::test]
    async fn test_store_correctness() {
        let builder = TestSuiteBuilder {};
        match Suite::test_all(builder).await {
            Ok(_) => {}
            Err(e) => {
                panic!("test_store_correctness failed: {}", e);
            }
        }
    }
}
