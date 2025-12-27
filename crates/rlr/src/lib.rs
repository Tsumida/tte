// I want to implement a embedded reliable log replicator named as rlr.
// The OMS and MatchEngine takes their own rlr, and consumes committed msg  (persisted request like PlaceOrderReq, CancelOrderReq, AdminCmd) from it and take no concerns about data loss and corruption.
// Your mission is help me figure out the blue print, and tell me what should I do step by step. You don't have to complete the perfect code at once.

// I want something like (Take OMS as example):
// 1. Setup 4 stateful OMS process and their RLR module. There are 1 leader , 2 followers and 1 learner in RLR cluster.
// 3. One of the OMS processors is told leader and then handle the Order flow, then the leader OMS propose the PlaceOrderReq in batch as RLR ingress. The rest OMS do not handle order flow.
// 4. Once RLR commited the requests, the 3 OMS peer consume committed flow and then apply them into memory statemachine. Only a OMS learner (told by RLR) dumps events into Mysql and rest do nothing.
// 5. Once a OMS process restarts, it read snapshot from peers and then re-install it and then catch up.
// The above are my consideration about using Openraft to build a standalone RLR, please review them and try to build a demo with code as simple as possible.
// It openraft ok to do such things? please review

// Here 's some features I want to have in RLR:
// 1. Embedded: RLR is embedded into OMS/MatchEngine process, no extra deployment needed.
// 2. All OMS restarts and then install the exact same snapshot from RLR leader, so that all OMS have the same memory state machine.
// 3. RLR report metrics about replication lag, snapshot install time, and election etc.
// 4. RLR support dynamic membership change, so that I can add/remove OMS process dynamically.
