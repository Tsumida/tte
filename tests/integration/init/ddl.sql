-- 创建数据库
SET NAMES utf8mb4;
CREATE DATABASE IF NOT EXISTS tex DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_general_ci;
USE tex;

CREATE TABLE `spot` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT COMMENT '自增ID',
  `account_id` bigint unsigned NOT NULL DEFAULT '0' COMMENT '用户ID',
  `currency` varchar(8) NOT NULL DEFAULT '' COMMENT '币种',
  `deposit` decimal(65,18) NOT NULL DEFAULT '0.000000000000000000' COMMENT '可用资产',
  `frozen` decimal(65,18) NOT NULL DEFAULT '0.000000000000000000' COMMENT '冻结资产',
  `balance` decimal(65,18) NOT NULL DEFAULT '0.000000000000000000' COMMENT '总资产',
  `version` bigint unsigned NOT NULL DEFAULT '0' COMMENT '版本号, 用于并发控制',
  `create_time` bigint unsigned NOT NULL DEFAULT '0' COMMENT '创建时间',
  `update_time` bigint unsigned NOT NULL DEFAULT '0' COMMENT '更新时间',
  PRIMARY KEY (`id`),
  UNIQUE KEY `udx_account_currency` (`account_id`,`currency`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='账户资产表';

CREATE TABLE `balance_event` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT COMMENT '自增ID',
  `account_id` bigint unsigned NOT NULL DEFAULT '0' COMMENT '用户ID',
  `currency` varchar(8) NOT NULL DEFAULT '' COMMENT '币种',
  `deposit` decimal(65,18) NOT NULL DEFAULT '0.000000000000000000' COMMENT '可用资产',
  `frozen` decimal(65,18) NOT NULL DEFAULT '0.000000000000000000' COMMENT '冻结资产',
  `balance` decimal(65,18) NOT NULL DEFAULT '0.000000000000000000' COMMENT '总资产',
  `update_time` bigint unsigned NOT NULL DEFAULT '0' COMMENT '事件更新时间, 来自oms.BalanceEvent',
  `create_time` bigint unsigned NOT NULL DEFAULT '0' COMMENT '写入时间',
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='账户余额变更事件表';


CREATE TABLE `fill` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT COMMENT '自增ID',
  `match_id` bigint unsigned NOT NULL DEFAULT '0' COMMENT '成交ID',
  `prev_match_id` bigint unsigned NOT NULL DEFAULT '0' COMMENT '上次成交ID, 用于幂等',
  `seq_id` bigint unsigned NOT NULL DEFAULT '0' COMMENT '撮合器sequencer自增ID, 用于和match_result关联',
  `taker_order_id` varchar(64) NOT NULL DEFAULT '' COMMENT '吃单ID',
  `maker_order_id` varchar(64) NOT NULL DEFAULT '' COMMENT '挂单ID',
  `taker_account_id` bigint unsigned NOT NULL DEFAULT '0' COMMENT '吃单用户ID',
  `maker_account_id` bigint unsigned NOT NULL DEFAULT '0' COMMENT '挂单用户ID',
  `price` decimal(65,18) NOT NULL DEFAULT '0.000000000000000000' COMMENT '成交价格',
  `qty` decimal(65,18) NOT NULL DEFAULT '0.000000000000000000' COMMENT '成交量',
  `direction` tinyint unsigned NOT NULL DEFAULT '0' COMMENT '方向,Bid,Ask',
  `tx_time` bigint unsigned NOT NULL DEFAULT '0' COMMENT '成交时间',
  `create_time` bigint unsigned NOT NULL DEFAULT '0' COMMENT '创建时间',
  `update_time` bigint unsigned NOT NULL DEFAULT '0' COMMENT '更新时间',
  PRIMARY KEY (`id`),
  UNIQUE KEY `udx_match_id` (`match_id`),
  KEY `idx_seq_id` (`seq_id`),
  KEY `idx_taker_order_id` (`taker_order_id`),
  KEY `idx_maker_order_id` (`maker_order_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='成交明细日志表';

CREATE TABLE `match_result_event` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT COMMENT '自增ID',
  `match_id` bigint unsigned NOT NULL DEFAULT '0' COMMENT '撮合器sequencer自增ID',
  `prev_match_id` bigint unsigned NOT NULL DEFAULT '0' COMMENT '上次撮合ID, 用于幂等',
  `seq_id` bigint unsigned NOT NULL DEFAULT '0' COMMENT '撮合器sequencer自增ID, 全局唯一',
  `base` varchar(8) NOT NULL DEFAULT '' COMMENT '基础币种',
  `quote` varchar(8) NOT NULL DEFAULT '' COMMENT '计价币种',
  `biz_action` tinyint unsigned NOT NULL DEFAULT '0' COMMENT '业务动作, 对应BizAction枚举',
  `payload` text COMMENT '撮合结果的详细信息',
  `tx_time` bigint unsigned NOT NULL DEFAULT '0' COMMENT '撮合时间',
  `create_time` bigint unsigned NOT NULL DEFAULT '0' COMMENT '创建时间',
  `update_time` bigint unsigned NOT NULL DEFAULT '0' COMMENT '更新时间',
  PRIMARY KEY (`id`),
  UNIQUE KEY `udx_match_id` (`match_id`),
  KEY `idx_seq_id` (`seq_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='撮合结果事件表';

CREATE TABLE `order` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT COMMENT '自增ID',
  `order_id` varchar(64) NOT NULL DEFAULT '' COMMENT '订单ID',
  `account_id` bigint unsigned NOT NULL DEFAULT '0' COMMENT '用户ID',
  `trading_pair` varchar(16) NOT NULL DEFAULT '' COMMENT '交易对',
  `order_type` tinyint unsigned NOT NULL DEFAULT '0' COMMENT '订单类型, 对应OrderType枚举',
  `direction` tinyint unsigned NOT NULL DEFAULT '0' COMMENT '方向,Bid,Ask',
  `target_qty` decimal(65,18) NOT NULL DEFAULT '0.000000000000000000' COMMENT '目标成交量',
  `filled_qty` decimal(65,18) NOT NULL DEFAULT '0.000000000000000000' COMMENT '已成交量',
  `price` decimal(65,18) NOT NULL DEFAULT '0.000000000000000000' COMMENT '委托价格',
  `order_state` tinyint unsigned NOT NULL DEFAULT '0' COMMENT '订单状态, 对应OrderState枚举',
  `version` bigint unsigned NOT NULL DEFAULT '0' COMMENT '版本号, 用于并发控制',
  `create_time` bigint unsigned NOT NULL DEFAULT '0' COMMENT '创建时间',
  `update_time` bigint unsigned NOT NULL DEFAULT '0' COMMENT '更新时间',
  `final_time` bigint unsigned NOT NULL DEFAULT '0' COMMENT '订单完结时间',
  PRIMARY KEY (`id`),
  UNIQUE KEY `udx_order_id` (`order_id`),
  KEY `idx_aid_pair_time` (`account_id`,`trading_pair`,`create_time`),
  KEY `idx_final_time_state` (`final_time`, `order_state`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='订单状态表';

CREATE TABLE `order_event` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT COMMENT '自增ID',
  `order_id` varchar(64) NOT NULL DEFAULT '' COMMENT '订单ID',
  `version` bigint unsigned NOT NULL DEFAULT '0' COMMENT '版本号, 用于并发控制',
  `account_id` bigint unsigned NOT NULL DEFAULT '0' COMMENT '用户ID',
  `base` varchar(8) NOT NULL DEFAULT '' COMMENT '基础币种',
  `quote` varchar(8) NOT NULL DEFAULT '' COMMENT '计价币种',
  `direction` tinyint unsigned NOT NULL DEFAULT '0' COMMENT '方向,Bid,Ask',
  `payload` text COMMENT '订单事件原始数据',
  `create_time` bigint unsigned NOT NULL DEFAULT '0' COMMENT '创建时间',
  `update_time` bigint unsigned NOT NULL DEFAULT '0' COMMENT '更新时间',
  PRIMARY KEY (`id`),
  UNIQUE KEY `idx_order_id` (`order_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='订单事件表';
