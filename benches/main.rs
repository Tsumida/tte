// use criterion::{Criterion, criterion_group, criterion_main};
// use rust_decimal_macros::dec;

// use trade_engine::{
//     common::types::Direction,
//     ledger::{
//         spot::{SpotLedger, SpotLedgerMatchResultConsumer, SpotLedgerRPCHandler},
//         testkit::add_balance,
//     },
//     testkit::testkit::{fill_buy_limit_order, new_limit_order},
// };

// fn criterion_benchmark(c: &mut Criterion) {}

// fn bench_place_order(c: &mut Criterion) {
//     let mut ledger = SpotLedger::new();
//     _ = add_balance(&mut ledger, 1001, "USDT", 1000000000.0);

//     c.bench_function("place_order", |b| {
//         b.iter(|| {
//             let order = new_limit_order("ORD10000001", 1001, Direction::Buy, dec!(100), dec!(0.5));
//             _ = ledger.place_order(order, dec!(50.0));
//         })
//     });
// }

// fn bench_fill_order(c: &mut Criterion) {
//     let mut ledger = SpotLedger::new();
//     _ = add_balance(&mut ledger, 1001, "USDT", 1000000000.0);
//     _ = add_balance(&mut ledger, 1002, "BTC", 10000.0);
//     let buy_order = new_limit_order("ORD10000001", 1001, Direction::Buy, dec!(0.1), dec!(1));
//     let sell_order = new_limit_order("ORD10000002", 1002, Direction::Sell, dec!(0.1), dec!(1));
//     _ = ledger.place_order(buy_order.clone(), dec!(0.1));
//     _ = ledger.place_order(sell_order.clone(), dec!(0.1));

//     c.bench_function("fill_order", |b| {
//         let fill = fill_buy_limit_order(&buy_order, &sell_order, dec!(0.1))
//             .fill_result
//             .unwrap();
//         b.iter(|| _ = ledger.fill_order(&fill.results[0]))
//     });
// }

// criterion_group!(benches, bench_place_order, bench_fill_order);
// criterion_main!(benches);
