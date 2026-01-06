use std::{collections::HashSet, sync::Arc};

use tokio::sync::RwLock;
use tonic::transport::Channel;
use tte_core::pbcode::oms::oms_service_client;

use crate::parser;

pub struct SmartMeClient {
    nodes: Arc<RwLock<oms_service_client::OmsServiceClient<Channel>>>,
}

impl SmartMeClient {
    // Build a SmartMeClient connecting to the given OMS address
    pub async fn new(
        addr: &str,
        addr_map: HashSet<String>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let client = oms_service_client::OmsServiceClient::connect(format!("http://{}", addr))
            .await
            .map_err(|e| format!("Failed to connect to OMS at {}: {}", addr, e))?;
        Ok(SmartMeClient {
            nodes: Arc::new(RwLock::new(client)),
        })
    }

    // todo:
    pub async fn place_order(line: String) -> Result<(), anyhow::Error> {
        // 找出当前的leader节点
        todo!();
    }

    async fn place_order_to(
        line: String,
        client: &mut oms_service_client::OmsServiceClient<Channel>,
    ) -> Result<(), anyhow::Error> {
        let req = parser::FileParser::new()
            .parse_fill(line.trim().split(',').collect())
            .unwrap();
        tracing::info!("Req{:?}", req);
        let response = client.place_order(req).await.expect("place order failed");
        tracing::info!("PlaceOrderRsp{:?}", response.into_inner());

        Ok(())
    }
}
