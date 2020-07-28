use criterion::{criterion_group, criterion_main, Criterion};
use rsfbclient::{prelude::*, ConnectionBuilder, Statement, Transaction};

const SQL: &str = "SELECT 1 FROM RDB$DATABASE";

fn select_1(tr: &mut Transaction) {
    tr.execute_immediate(SQL, ()).unwrap();
}

fn select_1_prepared(stmt: &mut Statement, tr: &mut Transaction) {
    stmt.execute(tr, ()).unwrap();
    tr.commit_retaining().unwrap();
}

fn exec_select_1_api(conn: &mut impl Queryable) {
    conn.execute(SQL, ()).unwrap();
}

fn query_select_1_api(conn: &mut impl Queryable) {
    conn.query_first::<_, (i32,)>(SQL, ()).unwrap();
}

fn criterion_benchmark(c: &mut Criterion) {
    #[cfg(not(feature = "dynamic_loading"))]
    let mut conn = ConnectionBuilder::default()
        .connect()
        .expect("Error on connect the test database");

    #[cfg(feature = "dynamic_loading")]
    let mut conn = ConnectionBuilder::with_client("./fbclient.lib")
        .expect("Error finding fbclient lib")
        .connect()
        .expect("Error on connect the test database");

    let mut tr = conn.transaction().unwrap();

    let mut prepared = tr.prepare(SQL).unwrap();

    c.bench_function("select 1", |b| b.iter(|| select_1(&mut tr)));

    c.bench_function("select 1 prepared", |b| {
        b.iter(|| select_1_prepared(&mut prepared, &mut tr))
    });

    drop(prepared);
    drop(tr);

    c.bench_function("exec_select_1_api_conn", |b| {
        b.iter(|| exec_select_1_api(&mut conn))
    });

    c.bench_function("query_select_1_api_conn", |b| {
        b.iter(|| query_select_1_api(&mut conn))
    });

    let mut tr = conn.transaction().unwrap();

    c.bench_function("exec_select_1_api_tr", |b| {
        b.iter(|| {
            exec_select_1_api(&mut tr);
            tr.commit_retaining().unwrap();
        })
    });

    c.bench_function("query_select_1_api_tr", |b| {
        b.iter(|| {
            query_select_1_api(&mut tr);
            tr.commit_retaining().unwrap();
        })
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
