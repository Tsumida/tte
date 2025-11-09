use criterion::{Criterion, criterion_group, criterion_main};
use rust_decimal_macros::dec;

use trade_engine::{
    common::types::Direction,
    ledger::spot::{SpotLedger, SpotLedgerRPCHandler},
    ledger::testkit::add_balance,
    testkit::testkit::new_limit_order,
};

fn criterion_benchmark(c: &mut Criterion) {}

fn bench_place_order(c: &mut Criterion) {
    let mut ledger = SpotLedger::new();
    _ = add_balance(&mut ledger, 1001, "USDT", 1000000000.0);

    c.bench_function("place_order", |b| {
        b.iter(|| {
            let order = new_limit_order("ORD10000001", 1001, Direction::Buy, dec!(100), dec!(0.5));
            _ = ledger.place_order(order, dec!(50.0));
        })
    });
}

criterion_group!(benches, bench_place_order);
criterion_main!(benches);
