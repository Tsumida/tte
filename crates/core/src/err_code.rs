//!
//! 错误代码定义
//! 每个domain分配100个号码

pub static ERR_INTERNAL: i32 = 1000000; // OMS内部错误, 一般用于各个组件非预期错误
pub static ERR_INPOSSIBLE_STATE: i32 = 1000001; // 不可能的状态
pub static ERR_INVALID_REQUEST: i32 = 1000002; // 请求参数无效

// 撮合器返回错误
pub static ERR_OB_ORDER_TYPE_TIF: i32 = 1000101; // 订单类型或时间有效性错误
pub static ERR_OB_ORDER_PRICE_OVERFLOW: i32 = 1000102; // 订单价格超出波动范围
pub static ERR_OB_INVALID_SEQ_ID: i32 = 1000103; // seqID无效
pub static ERR_OB_ORDER_NOT_FOUND: i32 = 1000104; // 订单不在订单簿中
pub static ERR_OB_ORDER_FILLED: i32 = 1000105; // 订单已经完全成交
pub static ERR_OB_ORDER_CANCELED: i32 = 1000106; // 订单已经被取消

// OMS
pub static ERR_OMS_PAIR_NOT_TRADING: i32 = 1000201; // 交易对不在交易状态
pub static ERR_OMS_PRICE_OUT_OF_RANGE: i32 = 1000202; // 价格超出范围
pub static ERR_OMS_QTY_OUT_OF_RANGE: i32 = 1000203; // 数量过小
pub static ERR_OMS_DUPLICATE_CLI_ORD_ID: i32 = 1000204; // client_order_id重复, 等于下单重复
pub static ERR_OMS_PAIR_NOT_FOUND: i32 = 1000205; // 交易对未找到
pub static ERR_OMS_ORDER_NOT_FOUND: i32 = 1000206; // 订单未找到
pub static ERR_OMS_ACCOUNT_NOT_FOUND: i32 = 1000207; // 账户未找到
pub static ERR_OMS_INVALID_MATCH_RESULT: i32 = 1000208; // 撮合返回数据无法解析
pub static ERR_OMS_MATCH_RESULT_FAILED: i32 = 1000209; // 撮合结果处理失败, 需要介入处理

pub static ERR_LEDGER_INSUFFICIENT_BALANCE: i32 = 1000301; // 余额不足
pub static ERR_LEDGER_INVALID_ACCOUNT: i32 = 1000302; // 无效账户
pub static ERR_LEDGER_INVALID_FROZEN_ID: i32 = 1000303; // 无效冻结ID
pub static ERR_LEDGER_INSUFFICIENT_FROZEN: i32 = 1000304; // 冻结释放数量不足

pub trait TradeEngineErr {
    fn module(&self) -> &'static str;
    fn code(&self) -> i32;
}
