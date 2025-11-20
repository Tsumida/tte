#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct TradePair {
    /// 基础币种
    #[prost(string, tag = "1")]
    pub base: ::prost::alloc::string::String,
    /// 计价币种
    #[prost(string, tag = "2")]
    pub quote: ::prost::alloc::string::String,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Order {
    #[prost(string, tag = "1")]
    pub order_id: ::prost::alloc::string::String,
    #[prost(message, optional, tag = "2")]
    pub trade_pair: ::core::option::Option<TradePair>,
    /// 对应Direction枚举
    #[prost(int32, tag = "3")]
    pub direction: i32,
    /// 对应TimeInForce枚举
    #[prost(int32, tag = "4")]
    pub time_in_force: i32,
    /// 对应OrderType枚举
    #[prost(int32, tag = "5")]
    pub order_type: i32,
    #[prost(string, tag = "6")]
    pub price: ::prost::alloc::string::String,
    #[prost(string, tag = "7")]
    pub quantity: ::prost::alloc::string::String,
    /// us
    #[prost(string, tag = "8")]
    pub create_time: ::prost::alloc::string::String,
    /// 可选
    #[prost(string, tag = "9")]
    pub client_order_id: ::prost::alloc::string::String,
    /// 对应STPStrategy枚举
    #[prost(int32, tag = "10")]
    pub stp_strategy: i32,
    #[prost(uint64, tag = "11")]
    pub account_id: u64,
    /// true表示只挂单不吃单
    #[prost(bool, tag = "12")]
    pub post_only: bool,
    /// 创建订单时初始化，此后不变
    #[prost(uint64, tag = "13")]
    pub seq_id: u64,
    #[prost(uint64, tag = "14")]
    pub prev_seq_id: u64,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct OrderDetail {
    #[prost(message, optional, tag = "1")]
    pub original: ::core::option::Option<Order>,
    /// 对应OrderState枚举
    #[prost(string, tag = "2")]
    pub current_state: ::prost::alloc::string::String,
    #[prost(string, tag = "3")]
    pub filled_quantity: ::prost::alloc::string::String,
    #[prost(uint64, tag = "4")]
    pub last_seq_id: u64,
    /// us
    #[prost(uint64, tag = "5")]
    pub update_time: u64,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct AdminCmd {}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct TradeCmd {
    #[prost(uint64, tag = "1")]
    pub seq_id: u64,
    #[prost(uint64, tag = "2")]
    pub prev_seq_id: u64,
    /// 对应BizAction枚举
    #[prost(int32, tag = "3")]
    pub biz_action: i32,
    /// biz_action为PlaceOrder时有效
    #[prost(message, optional, tag = "4")]
    pub place_order_req: ::core::option::Option<PlaceOrderReq>,
    /// biz_action为CancelOrder时有效
    #[prost(message, optional, tag = "5")]
    pub cancel_order_req: ::core::option::Option<CancelOrderReq>,
    /// biz_action >=100及以上时有效
    #[prost(message, optional, tag = "10")]
    pub admin_cmd: ::core::option::Option<AdminCmd>,
}
/// 撮合结果
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct MatchRecord {
    #[prost(uint64, tag = "1")]
    pub seq_id: u64,
    #[prost(uint64, tag = "2")]
    pub prev_seq_id: u64,
    #[prost(string, tag = "3")]
    pub price: ::prost::alloc::string::String,
    #[prost(string, tag = "4")]
    pub quantity: ::prost::alloc::string::String,
    /// 对应Direction枚举
    #[prost(int32, tag = "5")]
    pub direction: i32,
    #[prost(string, tag = "6")]
    pub taker_order_id: ::prost::alloc::string::String,
    #[prost(string, tag = "7")]
    pub maker_order_id: ::prost::alloc::string::String,
    #[prost(bool, tag = "8")]
    pub is_taker_fulfilled: bool,
    #[prost(bool, tag = "9")]
    pub is_maker_fulfilled: bool,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct MatchResult {
    #[prost(message, optional, tag = "1")]
    pub order: ::core::option::Option<Order>,
    #[prost(message, repeated, tag = "2")]
    pub records: ::prost::alloc::vec::Vec<MatchRecord>,
}
/// ======================================== 账户类
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct BalanceItem {
    /// BTC\/USDT等
    #[prost(string, tag = "1")]
    pub currency: ::prost::alloc::string::String,
    /// 总余额, 等于available + frozen
    #[prost(string, tag = "2")]
    pub balance: ::prost::alloc::string::String,
    /// 可用于交易
    #[prost(string, tag = "3")]
    pub available: ::prost::alloc::string::String,
    /// 冻结余额
    #[prost(string, tag = "4")]
    pub frozen: ::prost::alloc::string::String,
    /// 最后更新时间，us
    #[prost(uint64, tag = "5")]
    pub update_time: u64,
}
/// ======================================== 配置类
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct TradePairConfig {
    #[prost(string, tag = "1")]
    pub trade_pair: ::prost::alloc::string::String,
    #[prost(string, tag = "2")]
    pub min_price_increment: ::prost::alloc::string::String,
    #[prost(string, tag = "3")]
    pub min_quantity_increment: ::prost::alloc::string::String,
    /// 对应TradePairState枚举
    #[prost(int32, tag = "4")]
    pub state: i32,
    /// 价格波动限制百分比，例如0.1表示10%
    #[prost(string, tag = "5")]
    pub volatility_limit: ::prost::alloc::string::String,
}
/// ======================================= 行情类
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct LevelSnapshot {
    #[prost(string, tag = "1")]
    pub trade_pair: ::prost::alloc::string::String,
    #[prost(message, repeated, tag = "2")]
    pub bids: ::prost::alloc::vec::Vec<Order>,
    #[prost(message, repeated, tag = "3")]
    pub asks: ::prost::alloc::vec::Vec<Order>,
}
/// ======================================== 指令类
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ReplaceOrderCmd {
    #[prost(string, tag = "1")]
    pub order_id: ::prost::alloc::string::String,
    #[prost(string, tag = "2")]
    pub new_price: ::prost::alloc::string::String,
    #[prost(string, tag = "3")]
    pub new_quantity: ::prost::alloc::string::String,
    #[prost(uint64, tag = "4")]
    pub account_id: u64,
}
/// ======================================== RPC类
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct PlaceOrderReq {
    #[prost(message, optional, tag = "1")]
    pub order: ::core::option::Option<Order>,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct PlaceOrderRsp {}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ReplaceOrderReq {
    #[prost(message, optional, tag = "1")]
    pub cmd: ::core::option::Option<ReplaceOrderCmd>,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ReplaceOrderRsp {}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct CancelOrderReq {
    #[prost(string, tag = "1")]
    pub order_id: ::prost::alloc::string::String,
    #[prost(uint64, tag = "2")]
    pub account_id: u64,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct CancelOrderRsp {}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct TakeSnapshotReq {}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct TakeSnapshotRsp {}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetOrderDetailReq {
    #[prost(string, tag = "1")]
    pub order_id: ::prost::alloc::string::String,
    #[prost(string, tag = "2")]
    pub client_order_id: ::prost::alloc::string::String,
    #[prost(uint64, tag = "3")]
    pub account_id: u64,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetOrderDetailRsp {
    #[prost(message, optional, tag = "1")]
    pub detail: ::core::option::Option<OrderDetail>,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct TransferFreezeReq {
    #[prost(string, tag = "1")]
    pub transfer_id: ::prost::alloc::string::String,
    #[prost(uint64, tag = "2")]
    pub account_id: u64,
    #[prost(string, tag = "3")]
    pub currency: ::prost::alloc::string::String,
    #[prost(string, tag = "4")]
    pub amount: ::prost::alloc::string::String,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct TransferFreezeRsp {}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct TransferReq {
    #[prost(string, tag = "1")]
    pub transfer_id: ::prost::alloc::string::String,
    #[prost(uint64, tag = "2")]
    pub from_account_id: u64,
    #[prost(uint64, tag = "3")]
    pub to_account_id: u64,
    #[prost(string, tag = "4")]
    pub currency: ::prost::alloc::string::String,
    #[prost(string, tag = "5")]
    pub amount: ::prost::alloc::string::String,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct TransferRsp {}
/// ======================================== 账户类
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetBalanceReq {
    #[prost(uint64, tag = "1")]
    pub account_id: u64,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GetBalanceRsp {
    #[prost(uint64, tag = "1")]
    pub account_id: u64,
    #[prost(message, repeated, tag = "2")]
    pub balances: ::prost::alloc::vec::Vec<BalanceItem>,
}
/// ======================================== 管理类
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct UpdateTradePairConfigReq {
    #[prost(message, optional, tag = "1")]
    pub config: ::core::option::Option<TradePairConfig>,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct UpdateTradePairConfigRsp {}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum OrderState {
    UndefinedOrderState = 0,
    New = 1,
    Rejected = 2,
    PendingNew = 3,
    PartiallyFilled = 4,
    Filled = 5,
    Cancelled = 6,
}
impl OrderState {
    /// String value of the enum field names used in the ProtoBuf definition.
    ///
    /// The values are not transformed in any way and thus are considered stable
    /// (if the ProtoBuf definition does not change) and safe for programmatic use.
    pub fn as_str_name(&self) -> &'static str {
        match self {
            OrderState::UndefinedOrderState => "UndefinedOrderState",
            OrderState::New => "New",
            OrderState::Rejected => "Rejected",
            OrderState::PendingNew => "PendingNew",
            OrderState::PartiallyFilled => "PartiallyFilled",
            OrderState::Filled => "Filled",
            OrderState::Cancelled => "Cancelled",
        }
    }
    /// Creates an enum from field names used in the ProtoBuf definition.
    pub fn from_str_name(value: &str) -> ::core::option::Option<Self> {
        match value {
            "UndefinedOrderState" => Some(Self::UndefinedOrderState),
            "New" => Some(Self::New),
            "Rejected" => Some(Self::Rejected),
            "PendingNew" => Some(Self::PendingNew),
            "PartiallyFilled" => Some(Self::PartiallyFilled),
            "Filled" => Some(Self::Filled),
            "Cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum Direction {
    UndefinedDirection = 0,
    Buy = 1,
    Sell = 2,
}
impl Direction {
    /// String value of the enum field names used in the ProtoBuf definition.
    ///
    /// The values are not transformed in any way and thus are considered stable
    /// (if the ProtoBuf definition does not change) and safe for programmatic use.
    pub fn as_str_name(&self) -> &'static str {
        match self {
            Direction::UndefinedDirection => "UndefinedDirection",
            Direction::Buy => "Buy",
            Direction::Sell => "Sell",
        }
    }
    /// Creates an enum from field names used in the ProtoBuf definition.
    pub fn from_str_name(value: &str) -> ::core::option::Option<Self> {
        match value {
            "UndefinedDirection" => Some(Self::UndefinedDirection),
            "Buy" => Some(Self::Buy),
            "Sell" => Some(Self::Sell),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum TimeInForce {
    UndefinedTif = 0,
    Gtk = 1,
    Fok = 2,
    Ioc = 3,
}
impl TimeInForce {
    /// String value of the enum field names used in the ProtoBuf definition.
    ///
    /// The values are not transformed in any way and thus are considered stable
    /// (if the ProtoBuf definition does not change) and safe for programmatic use.
    pub fn as_str_name(&self) -> &'static str {
        match self {
            TimeInForce::UndefinedTif => "UndefinedTIF",
            TimeInForce::Gtk => "GTK",
            TimeInForce::Fok => "FOK",
            TimeInForce::Ioc => "IOC",
        }
    }
    /// Creates an enum from field names used in the ProtoBuf definition.
    pub fn from_str_name(value: &str) -> ::core::option::Option<Self> {
        match value {
            "UndefinedTIF" => Some(Self::UndefinedTif),
            "GTK" => Some(Self::Gtk),
            "FOK" => Some(Self::Fok),
            "IOC" => Some(Self::Ioc),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum OrderType {
    UndefinedOrderType = 0,
    Limit = 1,
    Market = 2,
}
impl OrderType {
    /// String value of the enum field names used in the ProtoBuf definition.
    ///
    /// The values are not transformed in any way and thus are considered stable
    /// (if the ProtoBuf definition does not change) and safe for programmatic use.
    pub fn as_str_name(&self) -> &'static str {
        match self {
            OrderType::UndefinedOrderType => "UndefinedOrderType",
            OrderType::Limit => "Limit",
            OrderType::Market => "Market",
        }
    }
    /// Creates an enum from field names used in the ProtoBuf definition.
    pub fn from_str_name(value: &str) -> ::core::option::Option<Self> {
        match value {
            "UndefinedOrderType" => Some(Self::UndefinedOrderType),
            "Limit" => Some(Self::Limit),
            "Market" => Some(Self::Market),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum StpStrategy {
    UndefinedStpStrategy = 0,
    /// 撤销最老订单
    CancelTaker = 1,
    /// 撤销最新订单
    CancelMaker = 2,
    /// 撤销两个订单
    CancelBoth = 3,
}
impl StpStrategy {
    /// String value of the enum field names used in the ProtoBuf definition.
    ///
    /// The values are not transformed in any way and thus are considered stable
    /// (if the ProtoBuf definition does not change) and safe for programmatic use.
    pub fn as_str_name(&self) -> &'static str {
        match self {
            StpStrategy::UndefinedStpStrategy => "UndefinedSTPStrategy",
            StpStrategy::CancelTaker => "CancelTaker",
            StpStrategy::CancelMaker => "CancelMaker",
            StpStrategy::CancelBoth => "CancelBoth",
        }
    }
    /// Creates an enum from field names used in the ProtoBuf definition.
    pub fn from_str_name(value: &str) -> ::core::option::Option<Self> {
        match value {
            "UndefinedSTPStrategy" => Some(Self::UndefinedStpStrategy),
            "CancelTaker" => Some(Self::CancelTaker),
            "CancelMaker" => Some(Self::CancelMaker),
            "CancelBoth" => Some(Self::CancelBoth),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum BizAction {
    UndefinedBizAction = 0,
    Deposit = 1,
    Withdraw = 2,
    /// 账户间划转
    Transfer = 3,
    PlaceOrder = 4,
    ReplaceOrder = 5,
    CancelOrder = 6,
    FillOrder = 7,
}
impl BizAction {
    /// String value of the enum field names used in the ProtoBuf definition.
    ///
    /// The values are not transformed in any way and thus are considered stable
    /// (if the ProtoBuf definition does not change) and safe for programmatic use.
    pub fn as_str_name(&self) -> &'static str {
        match self {
            BizAction::UndefinedBizAction => "UndefinedBizAction",
            BizAction::Deposit => "Deposit",
            BizAction::Withdraw => "Withdraw",
            BizAction::Transfer => "Transfer",
            BizAction::PlaceOrder => "PlaceOrder",
            BizAction::ReplaceOrder => "ReplaceOrder",
            BizAction::CancelOrder => "CancelOrder",
            BizAction::FillOrder => "FillOrder",
        }
    }
    /// Creates an enum from field names used in the ProtoBuf definition.
    pub fn from_str_name(value: &str) -> ::core::option::Option<Self> {
        match value {
            "UndefinedBizAction" => Some(Self::UndefinedBizAction),
            "Deposit" => Some(Self::Deposit),
            "Withdraw" => Some(Self::Withdraw),
            "Transfer" => Some(Self::Transfer),
            "PlaceOrder" => Some(Self::PlaceOrder),
            "ReplaceOrder" => Some(Self::ReplaceOrder),
            "CancelOrder" => Some(Self::CancelOrder),
            "FillOrder" => Some(Self::FillOrder),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum TradePairState {
    UndefinedPairState = 0,
    /// 正常交易
    TradingPair = 1,
    /// 不可交易，但可以撤单
    SuspendedPair = 2,
    /// 新上线，暂不可交易
    NewPair = 3,
}
impl TradePairState {
    /// String value of the enum field names used in the ProtoBuf definition.
    ///
    /// The values are not transformed in any way and thus are considered stable
    /// (if the ProtoBuf definition does not change) and safe for programmatic use.
    pub fn as_str_name(&self) -> &'static str {
        match self {
            TradePairState::UndefinedPairState => "UndefinedPairState",
            TradePairState::TradingPair => "TradingPair",
            TradePairState::SuspendedPair => "SuspendedPair",
            TradePairState::NewPair => "NewPair",
        }
    }
    /// Creates an enum from field names used in the ProtoBuf definition.
    pub fn from_str_name(value: &str) -> ::core::option::Option<Self> {
        match value {
            "UndefinedPairState" => Some(Self::UndefinedPairState),
            "TradingPair" => Some(Self::TradingPair),
            "SuspendedPair" => Some(Self::SuspendedPair),
            "NewPair" => Some(Self::NewPair),
            _ => None,
        }
    }
}
/// Generated client implementations.
pub mod oms_service_client {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    /// ================================ 业务类
    #[derive(Debug, Clone)]
    pub struct OmsServiceClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl OmsServiceClient<tonic::transport::Channel> {
        /// Attempt to create a new client by connecting to a given endpoint.
        pub async fn connect<D>(dst: D) -> Result<Self, tonic::transport::Error>
        where
            D: TryInto<tonic::transport::Endpoint>,
            D::Error: Into<StdError>,
        {
            let conn = tonic::transport::Endpoint::new(dst)?.connect().await?;
            Ok(Self::new(conn))
        }
    }
    impl<T> OmsServiceClient<T>
    where
        T: tonic::client::GrpcService<tonic::body::BoxBody>,
        T::Error: Into<StdError>,
        T::ResponseBody: Body<Data = Bytes> + Send + 'static,
        <T::ResponseBody as Body>::Error: Into<StdError> + Send,
    {
        pub fn new(inner: T) -> Self {
            let inner = tonic::client::Grpc::new(inner);
            Self { inner }
        }
        pub fn with_origin(inner: T, origin: Uri) -> Self {
            let inner = tonic::client::Grpc::with_origin(inner, origin);
            Self { inner }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> OmsServiceClient<InterceptedService<T, F>>
        where
            F: tonic::service::Interceptor,
            T::ResponseBody: Default,
            T: tonic::codegen::Service<
                http::Request<tonic::body::BoxBody>,
                Response = http::Response<
                    <T as tonic::client::GrpcService<tonic::body::BoxBody>>::ResponseBody,
                >,
            >,
            <T as tonic::codegen::Service<
                http::Request<tonic::body::BoxBody>,
            >>::Error: Into<StdError> + Send + Sync,
        {
            OmsServiceClient::new(InterceptedService::new(inner, interceptor))
        }
        /// Compress requests with the given encoding.
        ///
        /// This requires the server to support it otherwise it might respond with an
        /// error.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.send_compressed(encoding);
            self
        }
        /// Enable decompressing responses.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.accept_compressed(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_decoding_message_size(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_encoding_message_size(limit);
            self
        }
        pub async fn place_order(
            &mut self,
            request: impl tonic::IntoRequest<super::PlaceOrderReq>,
        ) -> std::result::Result<tonic::Response<super::PlaceOrderRsp>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/oms.OMSService/PlaceOrder",
            );
            let mut req = request.into_request();
            req.extensions_mut().insert(GrpcMethod::new("oms.OMSService", "PlaceOrder"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn cancel_order(
            &mut self,
            request: impl tonic::IntoRequest<super::CancelOrderReq>,
        ) -> std::result::Result<tonic::Response<super::CancelOrderRsp>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/oms.OMSService/CancelOrder",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("oms.OMSService", "CancelOrder"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn transfer_freeze(
            &mut self,
            request: impl tonic::IntoRequest<super::TransferFreezeReq>,
        ) -> std::result::Result<
            tonic::Response<super::TransferFreezeRsp>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/oms.OMSService/TransferFreeze",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("oms.OMSService", "TransferFreeze"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn transfer(
            &mut self,
            request: impl tonic::IntoRequest<super::TransferReq>,
        ) -> std::result::Result<tonic::Response<super::TransferRsp>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static("/oms.OMSService/Transfer");
            let mut req = request.into_request();
            req.extensions_mut().insert(GrpcMethod::new("oms.OMSService", "Transfer"));
            self.inner.unary(req, path, codec).await
        }
        /// 订单查询类
        /// 优先按order_id查询，其次按client_order_id查询
        pub async fn get_order_detail(
            &mut self,
            request: impl tonic::IntoRequest<super::GetOrderDetailReq>,
        ) -> std::result::Result<
            tonic::Response<super::GetOrderDetailRsp>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/oms.OMSService/GetOrderDetail",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("oms.OMSService", "GetOrderDetail"));
            self.inner.unary(req, path, codec).await
        }
        /// Balance类
        pub async fn get_balance(
            &mut self,
            request: impl tonic::IntoRequest<super::GetBalanceReq>,
        ) -> std::result::Result<tonic::Response<super::GetBalanceRsp>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/oms.OMSService/GetBalance",
            );
            let mut req = request.into_request();
            req.extensions_mut().insert(GrpcMethod::new("oms.OMSService", "GetBalance"));
            self.inner.unary(req, path, codec).await
        }
        /// 内部使用
        pub async fn take_snapshot(
            &mut self,
            request: impl tonic::IntoRequest<super::TakeSnapshotReq>,
        ) -> std::result::Result<
            tonic::Response<super::TakeSnapshotRsp>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/oms.OMSService/TakeSnapshot",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("oms.OMSService", "TakeSnapshot"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn update_trade_pair_config(
            &mut self,
            request: impl tonic::IntoRequest<super::UpdateTradePairConfigReq>,
        ) -> std::result::Result<
            tonic::Response<super::UpdateTradePairConfigRsp>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/oms.OMSService/UpdateTradePairConfig",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("oms.OMSService", "UpdateTradePairConfig"));
            self.inner.unary(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod oms_service_server {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with OmsServiceServer.
    #[async_trait]
    pub trait OmsService: Send + Sync + 'static {
        async fn place_order(
            &self,
            request: tonic::Request<super::PlaceOrderReq>,
        ) -> std::result::Result<tonic::Response<super::PlaceOrderRsp>, tonic::Status>;
        async fn cancel_order(
            &self,
            request: tonic::Request<super::CancelOrderReq>,
        ) -> std::result::Result<tonic::Response<super::CancelOrderRsp>, tonic::Status>;
        async fn transfer_freeze(
            &self,
            request: tonic::Request<super::TransferFreezeReq>,
        ) -> std::result::Result<
            tonic::Response<super::TransferFreezeRsp>,
            tonic::Status,
        >;
        async fn transfer(
            &self,
            request: tonic::Request<super::TransferReq>,
        ) -> std::result::Result<tonic::Response<super::TransferRsp>, tonic::Status>;
        /// 订单查询类
        /// 优先按order_id查询，其次按client_order_id查询
        async fn get_order_detail(
            &self,
            request: tonic::Request<super::GetOrderDetailReq>,
        ) -> std::result::Result<
            tonic::Response<super::GetOrderDetailRsp>,
            tonic::Status,
        >;
        /// Balance类
        async fn get_balance(
            &self,
            request: tonic::Request<super::GetBalanceReq>,
        ) -> std::result::Result<tonic::Response<super::GetBalanceRsp>, tonic::Status>;
        /// 内部使用
        async fn take_snapshot(
            &self,
            request: tonic::Request<super::TakeSnapshotReq>,
        ) -> std::result::Result<tonic::Response<super::TakeSnapshotRsp>, tonic::Status>;
        async fn update_trade_pair_config(
            &self,
            request: tonic::Request<super::UpdateTradePairConfigReq>,
        ) -> std::result::Result<
            tonic::Response<super::UpdateTradePairConfigRsp>,
            tonic::Status,
        >;
    }
    /// ================================ 业务类
    #[derive(Debug)]
    pub struct OmsServiceServer<T: OmsService> {
        inner: _Inner<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    struct _Inner<T>(Arc<T>);
    impl<T: OmsService> OmsServiceServer<T> {
        pub fn new(inner: T) -> Self {
            Self::from_arc(Arc::new(inner))
        }
        pub fn from_arc(inner: Arc<T>) -> Self {
            let inner = _Inner(inner);
            Self {
                inner,
                accept_compression_encodings: Default::default(),
                send_compression_encodings: Default::default(),
                max_decoding_message_size: None,
                max_encoding_message_size: None,
            }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> InterceptedService<Self, F>
        where
            F: tonic::service::Interceptor,
        {
            InterceptedService::new(Self::new(inner), interceptor)
        }
        /// Enable decompressing requests with the given encoding.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.accept_compression_encodings.enable(encoding);
            self
        }
        /// Compress responses with the given encoding, if the client supports it.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.send_compression_encodings.enable(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.max_decoding_message_size = Some(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.max_encoding_message_size = Some(limit);
            self
        }
    }
    impl<T, B> tonic::codegen::Service<http::Request<B>> for OmsServiceServer<T>
    where
        T: OmsService,
        B: Body + Send + 'static,
        B::Error: Into<StdError> + Send + 'static,
    {
        type Response = http::Response<tonic::body::BoxBody>;
        type Error = std::convert::Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;
        fn poll_ready(
            &mut self,
            _cx: &mut Context<'_>,
        ) -> Poll<std::result::Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
        fn call(&mut self, req: http::Request<B>) -> Self::Future {
            let inner = self.inner.clone();
            match req.uri().path() {
                "/oms.OMSService/PlaceOrder" => {
                    #[allow(non_camel_case_types)]
                    struct PlaceOrderSvc<T: OmsService>(pub Arc<T>);
                    impl<T: OmsService> tonic::server::UnaryService<super::PlaceOrderReq>
                    for PlaceOrderSvc<T> {
                        type Response = super::PlaceOrderRsp;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::PlaceOrderReq>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move { (*inner).place_order(request).await };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = PlaceOrderSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/oms.OMSService/CancelOrder" => {
                    #[allow(non_camel_case_types)]
                    struct CancelOrderSvc<T: OmsService>(pub Arc<T>);
                    impl<
                        T: OmsService,
                    > tonic::server::UnaryService<super::CancelOrderReq>
                    for CancelOrderSvc<T> {
                        type Response = super::CancelOrderRsp;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::CancelOrderReq>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                (*inner).cancel_order(request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = CancelOrderSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/oms.OMSService/TransferFreeze" => {
                    #[allow(non_camel_case_types)]
                    struct TransferFreezeSvc<T: OmsService>(pub Arc<T>);
                    impl<
                        T: OmsService,
                    > tonic::server::UnaryService<super::TransferFreezeReq>
                    for TransferFreezeSvc<T> {
                        type Response = super::TransferFreezeRsp;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::TransferFreezeReq>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                (*inner).transfer_freeze(request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = TransferFreezeSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/oms.OMSService/Transfer" => {
                    #[allow(non_camel_case_types)]
                    struct TransferSvc<T: OmsService>(pub Arc<T>);
                    impl<T: OmsService> tonic::server::UnaryService<super::TransferReq>
                    for TransferSvc<T> {
                        type Response = super::TransferRsp;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::TransferReq>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move { (*inner).transfer(request).await };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = TransferSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/oms.OMSService/GetOrderDetail" => {
                    #[allow(non_camel_case_types)]
                    struct GetOrderDetailSvc<T: OmsService>(pub Arc<T>);
                    impl<
                        T: OmsService,
                    > tonic::server::UnaryService<super::GetOrderDetailReq>
                    for GetOrderDetailSvc<T> {
                        type Response = super::GetOrderDetailRsp;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetOrderDetailReq>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                (*inner).get_order_detail(request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = GetOrderDetailSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/oms.OMSService/GetBalance" => {
                    #[allow(non_camel_case_types)]
                    struct GetBalanceSvc<T: OmsService>(pub Arc<T>);
                    impl<T: OmsService> tonic::server::UnaryService<super::GetBalanceReq>
                    for GetBalanceSvc<T> {
                        type Response = super::GetBalanceRsp;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetBalanceReq>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move { (*inner).get_balance(request).await };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = GetBalanceSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/oms.OMSService/TakeSnapshot" => {
                    #[allow(non_camel_case_types)]
                    struct TakeSnapshotSvc<T: OmsService>(pub Arc<T>);
                    impl<
                        T: OmsService,
                    > tonic::server::UnaryService<super::TakeSnapshotReq>
                    for TakeSnapshotSvc<T> {
                        type Response = super::TakeSnapshotRsp;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::TakeSnapshotReq>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                (*inner).take_snapshot(request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = TakeSnapshotSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/oms.OMSService/UpdateTradePairConfig" => {
                    #[allow(non_camel_case_types)]
                    struct UpdateTradePairConfigSvc<T: OmsService>(pub Arc<T>);
                    impl<
                        T: OmsService,
                    > tonic::server::UnaryService<super::UpdateTradePairConfigReq>
                    for UpdateTradePairConfigSvc<T> {
                        type Response = super::UpdateTradePairConfigRsp;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::UpdateTradePairConfigReq>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                (*inner).update_trade_pair_config(request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = UpdateTradePairConfigSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                _ => {
                    Box::pin(async move {
                        Ok(
                            http::Response::builder()
                                .status(200)
                                .header("grpc-status", "12")
                                .header("content-type", "application/grpc")
                                .body(empty_body())
                                .unwrap(),
                        )
                    })
                }
            }
        }
    }
    impl<T: OmsService> Clone for OmsServiceServer<T> {
        fn clone(&self) -> Self {
            let inner = self.inner.clone();
            Self {
                inner,
                accept_compression_encodings: self.accept_compression_encodings,
                send_compression_encodings: self.send_compression_encodings,
                max_decoding_message_size: self.max_decoding_message_size,
                max_encoding_message_size: self.max_encoding_message_size,
            }
        }
    }
    impl<T: OmsService> Clone for _Inner<T> {
        fn clone(&self) -> Self {
            Self(Arc::clone(&self.0))
        }
    }
    impl<T: std::fmt::Debug> std::fmt::Debug for _Inner<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{:?}", self.0)
        }
    }
    impl<T: OmsService> tonic::server::NamedService for OmsServiceServer<T> {
        const NAME: &'static str = "oms.OMSService";
    }
}
