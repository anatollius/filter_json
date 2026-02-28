use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use filter_json::{filter_json, filter_json_exclude, FilterCriteria};

// ── Shared inputs ────────────────────────────────────────────────────────────

/// A flat object with several scalar fields.
const FLAT: &str = r#"{"id":1,"name":"Alice","age":30,"city":"London","active":true}"#;

/// A two-level nested object.
const NESTED: &str = r#"{
    "order": {
        "id": 42,
        "customer": {"name": "Bob", "email": "bob@example.com"},
        "total": 99.95,
        "status": "shipped"
    },
    "meta": {"created": "2024-01-01", "updated": "2024-06-15"}
}"#;

/// An array of 100 objects, each with several fields.
fn large_array() -> String {
    let mut s = String::from("[");
    for i in 0..100 {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!(
            r#"{{"id":{i},"name":"user{i}","secret":"tok{i}","score":{score}}}"#,
            score = i * 3
        ));
    }
    s.push(']');
    s
}

/// A wide object with 50 top-level keys; we include only one.
fn wide_object() -> String {
    let mut s = String::from("{");
    for i in 0..50 {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!(r#""key{i}":"value{i}""#));
    }
    s.push('}');
    s
}

// ── Benchmarks ───────────────────────────────────────────────────────────────

fn bench_include(c: &mut Criterion) {
    let mut g = c.benchmark_group("include");

    // Top-level key from a flat object.
    let c_name = FilterCriteria::new(&["name"]);
    g.bench_function("flat/top_level_key", |b| {
        b.iter(|| filter_json(FLAT, &c_name).unwrap())
    });

    // Deeply nested key — must recurse through two levels.
    let c_email = FilterCriteria::new(&["order.customer.email"]);
    g.bench_function("nested/deep_key", |b| {
        b.iter(|| filter_json(NESTED, &c_email).unwrap())
    });

    // Include one key out of 50 — lots of skipping.
    let wide = wide_object();
    let c_key0 = FilterCriteria::new(&["key0"]);
    g.bench_function("wide/one_of_fifty", |b| {
        b.iter_batched(
            || wide.as_str(),
            |input| filter_json(input, &c_key0).unwrap(),
            BatchSize::SmallInput,
        )
    });

    g.finish();
}

fn bench_exclude(c: &mut Criterion) {
    let mut g = c.benchmark_group("exclude");

    // Remove one top-level key from a flat object.
    let c_age = FilterCriteria::new(&["age"]);
    g.bench_function("flat/top_level_key", |b| {
        b.iter(|| filter_json_exclude(FLAT, &c_age).unwrap())
    });

    // Remove a nested key two levels down.
    let c_status = FilterCriteria::new(&["order.status"]);
    g.bench_function("nested/deep_key", |b| {
        b.iter(|| filter_json_exclude(NESTED, &c_status).unwrap())
    });

    // Remove one field from each element of a 100-item array.
    let array = large_array();
    let c_secret = FilterCriteria::new(&["secret"]);
    g.bench_function("array/strip_field_100_items", |b| {
        b.iter_batched(
            || array.as_str(),
            |input| filter_json_exclude(input, &c_secret).unwrap(),
            BatchSize::SmallInput,
        )
    });

    g.finish();
}

criterion_group!(benches, bench_include, bench_exclude);
criterion_main!(benches);
