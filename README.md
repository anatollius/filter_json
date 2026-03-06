# filter_json

`filter_json` transforms JSON text by filtering keys — without fully deserializing the input. It scans the byte stream directly, copying only what matches the criteria and skipping everything else. This makes it faster and cheaper than parse-everything-then-select approaches when you only need a small portion of a large document.

## How it works

Rather than building an in-memory representation of the whole document, `filter_json` walks the JSON byte-by-byte. Keys that don't match the criteria are skipped at the parser level — their values are never allocated. Only matching subtrees are copied to the output string.

## Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
filter_json = "0.1"
```

### Inclusion — keep only matching keys

```rust
use filter_json::{filter_json, FilterCriteria};

let json = r#"{"customer": {"name": "Alice", "age": 30}, "order_id": "ORD-1"}"#;
let criteria = FilterCriteria::new(&["customer.name"]);
let result = filter_json(json, &criteria).unwrap();

assert_eq!(result, r#"{"customer":{"name":"Alice"}}"#);
```

Multiple paths are supported — include fields from anywhere in the document in one pass:

```rust
let criteria = FilterCriteria::new(&["customer.name", "order_id"]);
let result = filter_json(json, &criteria).unwrap();

assert_eq!(result, r#"{"customer":{"name":"Alice"},"order_id":"ORD-1"}"#);
```

### Exclusion — strip matching keys

```rust
use filter_json::{filter_json_exclude, FilterCriteria};

let json = r#"{"name": "Alice", "secret_token": "abc123", "score": 99}"#;
let criteria = FilterCriteria::new(&["secret_token"]);
let result = filter_json_exclude(json, &criteria).unwrap();

assert_eq!(result, r#"{"name":"Alice","score":99}"#);
```

## API

### `FilterCriteria`

```rust
// From a slice of path strings
let c = FilterCriteria::new(&["shipping.address.city", "order_id"]);

// From a Vec<&str>
let c = FilterCriteria::from(vec!["customer.name"]);
```

Paths are dot-separated key names that describe a route through the JSON object hierarchy. `"a.b.c"` means the key `c` inside `b` inside `a`.

### `filter_json`

```rust
pub fn filter_json(input: &str, criteria: &FilterCriteria) -> Result<String, FilterError>
```

Returns a new JSON string containing only the keys matched by the criteria. Output is always compact (no extra whitespace). Returns `Err` on malformed input.

### `filter_json_exclude`

```rust
pub fn filter_json_exclude(input: &str, criteria: &FilterCriteria) -> Result<String, FilterError>
```

Returns a new JSON string with the matched keys removed. Output is compact. Returns `Err` on malformed input.

### `FilterError`

```rust
pub enum FilterError {
    InvalidJson(String),  // malformed JSON with a description
    UnexpectedEof,        // input ended mid-value
}
```

## Behaviour notes

**Output is always compact.** Whitespace in the input is not preserved; the output has no extra spaces or newlines.

**Entire subtrees are matched.** If a criterion points at an object, the whole subtree is included or excluded — you don't need to enumerate every leaf:

```rust
// Includes the full "payment" object with all its nested fields
let c = FilterCriteria::new(&["payment"]);
```

**Arrays in exclusion mode.** When an exclusion path leads into an array, the criteria are applied to each element. `"items.price"` removes `price` from every object in the `items` array:

```rust
let c = FilterCriteria::new(&["items.price"]);
// {"items":[{"sku":"A","price":10},{"sku":"B","price":20}]}
// → {"items":[{"sku":"A"},{"sku":"B"}]}
```

**JSON escape sequences in keys.** Key names in criteria are plain strings. `filter_json` decodes JSON escape sequences in keys (e.g. `\t`, `\uXXXX`) before comparing, so a criterion `"a\tb"` (containing a real tab character) matches the JSON key `"a\tb"`.

## Development

```bash
# Build and install the Python extension into the current environment
maturin develop

# Build a release wheel
maturin build --release

# Run all tests
cargo test

# Lint
cargo clippy

# Format
cargo fmt

# Run benchmarks
cargo bench
```
