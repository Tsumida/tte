use tte_core::pbcode::oms::{self, TradePair};

pub(crate) struct FileParser {}

impl FileParser {
    pub(crate) fn new() -> Self {
        FileParser {}
    }

    //  OMS:Bid,1001,CLI_1001_00001,LIMIT,BTC_USDT,10050.00,0.5,GTK,1,false
    //  OMS:Ask,1002,CLI_1002_00001,LIMIT,BTC_USDT,9990.00,0.5,GTK,1,false
    pub(crate) fn parse_fill(
        &self,
        parts: Vec<&str>,
    ) -> Result<oms::PlaceOrderReq, Box<dyn std::error::Error>> {
        // 解析指令字段
        let direction = parts[0];
        let account_id: u64 = parts[1]
            .parse()
            .map_err(|e| format!("AccountID parse error: {}", e))?;
        let cli_order_id = parts[2];
        let order_type = parts[3];
        let trade_pair_str = parts.get(4).ok_or("Missing TradePair")?;
        let trade_pair = TradePair::from_str(trade_pair_str)?;
        let price: f64 = parts[5]
            .parse()
            .map_err(|e| format!("Price parse error: {}", e))?;
        let quantity: f64 = parts[6]
            .parse()
            .map_err(|e| format!("Quantity parse error: {}", e))?;
        let time_in_force = parts[7];
        let stp_strategy: i32 = parts[8]
            .parse()
            .map_err(|e| format!("STPStrategy parse error: {}", e))?;
        let post_only: bool = parts[9]
            .parse()
            .map_err(|e| format!("PostOnly parse error: {}", e))?;

        // 映射枚举值
        let dir = match direction {
            "Bid" => oms::Direction::Buy,
            "Ask" => oms::Direction::Sell,
            _ => {
                tracing::error!("Invalid direction: {}", direction);
                return Err(format!("Invalid direction: {}", direction).into());
            }
        } as i32;

        let ord_type = match order_type {
            "LIMIT" => oms::OrderType::Limit,
            "MARKET" => oms::OrderType::Market,
            _ => {
                tracing::error!("Invalid order type: {}", order_type);
                return Err(format!("Invalid order type: {}", order_type).into());
            }
        } as i32;

        let tif = match time_in_force {
            "GTK" => oms::TimeInForce::Gtk,
            "IOC" => oms::TimeInForce::Ioc,
            "FOK" => oms::TimeInForce::Fok,
            _ => {
                tracing::error!("Invalid time in force: {}", time_in_force);
                return Err(format!("Invalid time in force: {}", time_in_force).into());
            }
        } as i32;

        let create_ts_us = chrono::Utc::now().timestamp_micros() as u64;

        Ok(oms::PlaceOrderReq {
            order: Some(oms::Order {
                order_id: "".to_string(), // assigned by OMS
                client_order_id: cli_order_id.to_string(),
                direction: dir,
                account_id,
                order_type: ord_type,
                trade_pair: Some(trade_pair.into()),
                price: format!("{:.2}", price),       // 统一格式化价格
                quantity: format!("{:.4}", quantity), // 统一格式化数量
                time_in_force: tif,
                stp_strategy,
                post_only,
                trade_id: 0,
                prev_trade_id: 0,
                create_time: create_ts_us,
            }),
        })
    }

    //  OMS:Cancel,Bid,1001,CLI_1001_00001,BTC_USDT
    //  OMS:Cancel,Ask,1002,CLI_1002_00001,BTC_USDT
    pub(crate) fn parse_cancel(
        &self,
        parts: Vec<&str>,
    ) -> Result<oms::CancelOrderReq, Box<dyn std::error::Error>> {
        let parts: Vec<&str> = parts.into_iter().filter(|p| !p.is_empty()).collect();
        let direction = parts[0];
        let account_id: u64 = parts[1]
            .parse()
            .map_err(|e| format!("AccountID parse error: {}", e))?;
        let cli_order_id = parts[2];
        let trade_pair = TradePair::from_str(parts[3])?;

        Ok(oms::CancelOrderReq {
            account_id,
            order_id: "".to_string(), // assigned by OMS
            client_order_id: cli_order_id.to_string(),
            direction: match direction {
                "Bid" => oms::Direction::Buy as i32,
                "Ask" => oms::Direction::Sell as i32,
                _ => {
                    tracing::error!("Invalid direction: {}", direction);
                    return Err(format!("Invalid direction: {}", direction).into());
                }
            },
            base: trade_pair.base,
            quote: trade_pair.quote,
        })
    }
}
