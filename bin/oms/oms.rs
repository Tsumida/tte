#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing subscriber for logging
    tracing_subscriber::fmt::init().with_file(true).finish();

    // Create an instance of OMS
    let mut oms = oms::OMS::new();

    // Example: Process a trade command (this is just a placeholder)
    let trade_cmd = Arc::new(oms::TradeCmd {
        biz_action: oms::BizAction::PlaceOrder as i32,
        place_order_req: Some(oms::PlaceOrderReq {
            order: Some(oms::Order {
                client_order_id: "client123".to_string(),
                trade_pair: "BTC-USD".to_string(),
                direction: oms::Direction::Buy as i32,
                price: "50000".to_string(),
                quantity: "0.1".to_string(),
                post_only: false,
                order_type: oms::OrderType::Limit as i32,
                time_in_force: oms::TimeInForce::GTC as i32,
                seq_id: 1,
                prev_seq_id: 0,
            }),
        }),
        ..Default::default()
    });

    match oms.process_trade_cmd(trade_cmd).await {
        Ok(change_result) => {
            tracing::info!("Processed trade command successfully: {:?}", change_result);
        }
        Err(e) => {
            tracing::error!("Error processing trade command: {:?}", e);
        }
    }

    Ok(())
}
