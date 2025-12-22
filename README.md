# TTE

Tiny trade engine(TTE) 是一个Rust编写的交易引擎原型，包含订单管理系统(OMS)和撮合引擎(MatchEngine). 

OMS包含了订单校验、订单状态管理、风控管理、账本管理，接受下单、撤单请求后通过kafka转发给对应交易对的MatchEngine。

MatchEngine是一个确定性状态机，按照价格优先、时间优先顺序串行撮合买卖双方，并输出成交结果、撤单结果。

OMS订阅相关topic消费撮合输出，并根据结果来更新订单和账本。

[整体设计文档](https://ai.feishu.cn/wiki/HQCIwBlqZik5bhkPKcIcHJbCnVc)

# 功能
- 支持下单、撤单
- 订单类型支持：限价单(Good to cancel), 市价单(Fill or kill, Immediate or cancel)

