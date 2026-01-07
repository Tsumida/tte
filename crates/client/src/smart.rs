// use std::{
//     collections::{HashMap, HashSet},
//     sync::Arc,
// };

// use tokio::{sync::RwLock, time};
// use tonic::transport::Channel;
// use tte_core::pbcode::oms::oms_service_client;

// use crate::parser;

// pub struct SmartMeClient {
//     nodes: HashMap<u64, oms_service_client::OmsServiceClient<Channel>>,
//     current_leader: Arc<RwLock<Option<u64>>>,
// }

// impl SmartMeClient {
//     // Build a SmartMeClient connecting to the given OMS address
//     pub async fn new(addr_map: HashMap<u64, String>) -> Result<Self, Box<dyn std::error::Error>> {
//         let mut clients = HashMap::new();
//         for (id, address) in addr_map.iter() {
//             // with retry
//             for _ in 0..5 {
//                 match oms_service_client::OmsServiceClient::connect(format!("http://{}", address))
//                     .await
//                  {
//                     Ok(c) => {
//                         clients.insert(*id, c);
//                         break;
//                     }
//                     Err(e) => {
//                         tracing::error!("Failed to connect to OMS at {}: {}", address, e);
//                     }
//                 }
//                 time::sleep(std::time::Duration::from_millis(500)).await;
//             }
//         }

//         if clients.len() != addr_map.len() {
//             return Err("Failed to connect to all OMS nodes".into());
//         }

//         Ok(SmartMeClient {
//             nodes: clients,
//             current_leader: Arc::new(RwLock::new(None)),
//         })
//     }

//     // todo:
//     pub async fn place_order(&self, line: String) -> Result<(), anyhow::Error> {
//         // 找出当前的leader节点
//         let leader = self.current_leader.write().await;
//         // get leader_id and call place_order_to()
//         // if no leader_id, try call metrics() at one node to get current leader
//         if let Some(leader_id) = *leader {
//             if let Some(client) = self.nodes.get(&leader_id) {
//                 drop(leader);
//                 return self.place_order_to(&mut client.clone(), line).await;
//             }
//         }
//         // try to find leader
//         for (id, client) in self.nodes.iter() {
//             let mut client = client.clone();
//             let response = client..await;
//             if let Ok(resp) = response {
//                 let metrics = resp.into_inner();
//                 if metrics.is_leader {
//                     let mut leader = self.current_leader.write().await;
//                     *leader = Some(*id);
//                     drop(leader);
//                 }
//             }
//         }

//         self.place_order_to(&mut client, line).await
//     }

//     async fn place_order_to(
//         &self,
//         client: &mut oms_service_client::OmsServiceClient<Channel>,
//         line: String,
//     ) -> Result<(), anyhow::Error> {
//         let req = parser::FileParser::new()
//             .parse_fill(line.trim().split(',').collect())
//             .unwrap();
//         tracing::info!("Req{:?}", req);
//         let response = client.place_order(req).await.expect("place order failed");
//         tracing::info!("PlaceOrderRsp{:?}", response.into_inner());

//         Ok(())
//     }
// }
