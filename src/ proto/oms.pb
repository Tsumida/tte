
syntax = "proto3";

package oms;

message Order struct{
    string order_id = 1; 
    string symbol = 2; 
    string direction = 3; 
    string time_in_force = 4;
    int32 order_type = 5;
    string price = 6;
    string quantity = 7;
}

message LevelSnapshot struct{
    string symbol = 1; 
    repeated Order bids = 2; 
    repeated Order asks = 3; 
}

message ReplaceOrderCmd struct{
    string order_id = 1; 
    string new_price = 2;
    string new_quantity = 3;
    uint64 account_id = 4;
}

// Generate PlaceOrder, ReplaceOrder, CancelOrder, TakeSnapshot

message PlaceOrderReq{
    uint64 seq_id = 1; 
    uint64 prev_seq_id = 2;
    Order order = 3;
    uint64 account_id = 4;
}

message PlaceOrderRsp{
}

message ReplaceOrderReq{    
    uint64 seq_id = 1; 
    uint64 prev_seq_id = 2;
    ReplaceOrderCmd cmd = 3;
}

message ReplaceOrderRsp{

}

message CancelOrderReq{
    uint64 seq_id = 1; 
    uint64 prev_seq_id = 2;
    string order_id = 3;
    uint64 account_id = 4;
}

message CancelOrderRsp{

}

message TakeSnapshotReq{
}

message TakeSnapshotRsp{

}

service MatchService {
    rpc PlaceOrder(PlaceOrderReq) returns (PlaceOrderRsp);
    rpc ReplaceOrder(ReplaceOrderReq) returns (ReplaceOrderRsp);
    rpc CancelOrder(CancelOrderReq) returns (CancelOrderRsp);
    rpc TakeSnapshot(TakeSnapshotReq) returns (TakeSnapshotRsp);
}