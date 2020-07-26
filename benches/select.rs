use criterion::{criterion_group, criterion_main, Criterion};
use rsfbclient::{ConnectionBuilder, Statement, Transaction};

fn select_1(tr: &Transaction) {
    tr.execute_immediate("SELECT 1 FROM RDB$DATABASE", ())
        .unwrap();
}

fn select_1_prepared(stmt: &mut Statement, tr: &Transaction) {
    stmt.execute(()).unwrap();
    tr.commit_retaining().unwrap();
}

fn criterion_benchmark(c: &mut Criterion) {
    #[cfg(not(feature = "dynamic_loading"))]
    let conn = ConnectionBuilder::default()
        .connect()
        .expect("Error on connect the test database");

    #[cfg(feature = "dynamic_loading")]
    let conn = ConnectionBuilder::with_client("./fbclient.lib")
        .expect("Error finding fbclient lib")
        .connect()
        .expect("Error on connect the test database");

    let tr = conn.transaction().unwrap();

    let mut prepared = tr.prepare("SELECT 1 FROM RDB$DATABASE").unwrap();

    c.bench_function("select 1", |b| b.iter(|| select_1(&tr)));

    c.bench_function("select 1 prepared", |b| {
        b.iter(|| select_1_prepared(&mut prepared, &tr))
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
