//!
//! 错误代码定义

pub static ERR_OB_INTERNAL: i32 = 1000000; // OB内部错误, 一般用于不可能路径
pub static ERR_OB_ORDER_TYPE_TIF: i32 = 1000001; // 订单类型或时间有效性错误
pub static ERR_OB_ORDER_PRICE_OVERFLOW: i32 = 1000002; // 订单价格超出波动范围
pub static ERR_OB_INVALID_SEQ_ID: i32 = 1000003; // seqID无效

pub static ERR_OB_ORDER_NOT_FOUND: i32 = 100102; // 订单未找到
pub static ERR_OB_ORDER_FILLED: i32 = 1000003; // 订单已经完全成交
pub static ERR_OB_ORDER_CANCELED: i32 = 1000004; // 订单已经被取消
