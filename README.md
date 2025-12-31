# Introduction

Tiny trade engine(tte) is a crypto exchange prototype, consists of Order manage system (OMS), match engine (ME), ledger and risk management. 

It's a deterministic state machine based on [Openraft](https://github.com/databendlabs/openraft) and RocksDB.


# Functionality
-  Place order (Limit order + GTC, Market order + IOC\FOK)
-  Cancel order by orderID or ClientOrderID

# Design
[Document (Chinese)](https://ai.feishu.cn/wiki/HQCIwBlqZik5bhkPKcIcHJbC)

