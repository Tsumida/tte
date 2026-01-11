//! Simple gRPC transport for MsgBuffer gap-fill sync.
//!
//! This module provides a small server shim and a client wrapper around the
//! generated `msgsync` proto. The server delegates to a user-provided
//! `SyncBackend` implementation which should bridge to the crate's `MsgBuffer`.

use std::sync::Arc;

use tonic::{Request, Response, Status};

use async_trait::async_trait;

use crate::MsgBuffer;
use crate::pbcode::msgsync;
use crate::traits::{MsgBufSync, OutputEvent};
use serde_json;

use msgsync::{SyncRequest, SyncResponse};

#[async_trait]
pub trait SyncBackend: Send + Sync + 'static {
    async fn fetch_events(&self, start_id: u64, count: u64) -> Result<FetchResult, anyhow::Error>;
}

pub struct FetchResult {
    pub events: Vec<Vec<u8>>,
    pub start_id: u64,
    pub end_id: u64,
    pub is_complete: bool,
}

/// Adapter that exposes a `MsgBuffer<T>` as a `SyncBackend` by serializing
/// events with `serde_json`.
pub struct MsgBufferBackend<T: OutputEvent> {
    buffer: Arc<MsgBuffer<T>>,
}

impl<T> MsgBufferBackend<T>
where
    T: OutputEvent,
{
    pub fn new(buffer: Arc<MsgBuffer<T>>) -> Self {
        Self { buffer }
    }
}

#[async_trait]
impl<T> SyncBackend for MsgBufferBackend<T>
where
    T: OutputEvent,
{
    async fn fetch_events(&self, start_id: u64, count: u64) -> Result<FetchResult, anyhow::Error> {
        let vec = self.buffer.sync(start_id, count as usize).await?;
        let mut events = Vec::with_capacity(vec.len());
        for v in vec.into_iter() {
            events.push(serde_json::to_vec(&v)?); // todo: better serialization
        }
        let len = events.len() as u64;
        let end_id = if len == 0 {
            start_id.saturating_sub(1)
        } else {
            start_id + len - 1
        };
        let is_complete = len == count;
        Ok(FetchResult {
            events,
            start_id,
            end_id,
            is_complete,
        })
    }
}

#[derive(Clone)]
pub struct GrpcServer<B> {
    backend: Arc<B>,
}

impl<B> GrpcServer<B>
where
    B: SyncBackend,
{
    pub fn new(backend: Arc<B>) -> Self {
        Self { backend }
    }

    pub fn into_service(self) -> msgsync::msg_sync_server::MsgSyncServer<Self> {
        msgsync::msg_sync_server::MsgSyncServer::new(self)
    }
}

#[async_trait]
impl<B> msgsync::msg_sync_server::MsgSync for GrpcServer<B>
where
    B: SyncBackend,
{
    async fn sync_events(
        &self,
        request: Request<SyncRequest>,
    ) -> Result<Response<SyncResponse>, Status> {
        let req = request.into_inner();
        let res = self
            .backend
            .fetch_events(req.start_id, req.count)
            .await
            .map_err(|e| Status::internal(format!("backend error: {}", e)))?;

        let reply = SyncResponse {
            events: res.events,
            start_id: res.start_id,
            end_id: res.end_id,
            is_complete: res.is_complete,
        };
        Ok(Response::new(reply))
    }
}

#[derive(Clone)]
pub struct GrpcClient {
    inner: msgsync::msg_sync_client::MsgSyncClient<tonic::transport::Channel>,
}

impl GrpcClient {
    /// Connect to the given `uri` (example: "http://127.0.0.1:50051").
    pub async fn connect(uri: String) -> Result<Self, tonic::transport::Error> {
        let inner = msgsync::msg_sync_client::MsgSyncClient::connect(uri).await?;
        Ok(Self { inner })
    }

    pub async fn fetch(
        &mut self,
        flow_name: String,
        start_id: u64,
        count: u64,
    ) -> Result<FetchResult, tonic::Status> {
        let req = SyncRequest {
            flow_name,
            start_id,
            count,
        };
        let resp = self.inner.sync_events(Request::new(req)).await?;
        let body = resp.into_inner();
        Ok(FetchResult {
            events: body.events,
            start_id: body.start_id,
            end_id: body.end_id,
            is_complete: body.is_complete,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use tokio::net::TcpListener;
    use tonic::codegen::tokio_stream::wrappers::TcpListenerStream;
    use tonic::transport::Server;

    struct TestBackend {
        events: Vec<Vec<u8>>,
    }

    #[async_trait]
    impl SyncBackend for TestBackend {
        async fn fetch_events(
            &self,
            start_id: u64,
            count: u64,
        ) -> Result<FetchResult, anyhow::Error> {
            // events are 1-based index (id -> index = id - 1)
            let len = self.events.len() as u64;
            if start_id == 0 || start_id > len {
                return Ok(FetchResult {
                    events: vec![],
                    start_id,
                    end_id: start_id.saturating_sub(1),
                    is_complete: false,
                });
            }
            let start_idx = (start_id - 1) as usize;
            let max_take = ((len - start_id + 1) as u64).min(count) as usize;
            let slice = self.events[start_idx..start_idx + max_take].to_vec();
            let end_id = start_id + (slice.len() as u64).saturating_sub(1);
            let is_complete = (slice.len() as u64) == count;
            Ok(FetchResult {
                events: slice,
                start_id,
                end_id,
                is_complete,
            })
        }
    }

    #[tokio::test]
    async fn transport_roundtrip() {
        // Prepare backend with 5 events
        let backend = Arc::new(TestBackend {
            events: (1..=5)
                .map(|i| format!("event:{}", i).into_bytes())
                .collect(),
        });

        // Bind to an ephemeral port
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = GrpcServer::new(backend.clone());
        let svc = server.into_service();

        // Run server in background
        let serve_task = tokio::spawn(async move {
            Server::builder()
                .add_service(svc)
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .unwrap();
        });

        // Client connect and fetch events [2..4]
        let uri = format!("http://{}", addr);
        let mut client = GrpcClient::connect(uri).await.unwrap();
        let res = client.fetch("test".to_string(), 2, 3).await.unwrap();
        assert_eq!(res.start_id, 2);
        assert_eq!(res.end_id, 4);
        assert_eq!(res.events.len(), 3);
        assert_eq!(res.events[0], b"event:2".to_vec());

        // Stop server
        serve_task.abort();
    }
}
