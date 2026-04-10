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
let criteria = FilterCriteria::new(&["customer.name"]).unwrap();
let result = filter_json(json, &criteria).unwrap();

assert_eq!(result, r#"{"customer":{"name":"Alice"}}"#);
```

Multiple paths are supported — include fields from anywhere in the document in one pass:

```rust
let criteria = FilterCriteria::new(&["customer.name", "order_id"]).unwrap();
let result = filter_json(json, &criteria).unwrap();

assert_eq!(result, r#"{"customer":{"name":"Alice"},"order_id":"ORD-1"}"#);
```

### Exclusion — strip matching keys

```rust
use filter_json::{filter_json_exclude, FilterCriteria};

let json = r#"{"name": "Alice", "secret_token": "abc123", "score": 99}"#;
let criteria = FilterCriteria::new(&["secret_token"]).unwrap();
let result = filter_json_exclude(json, &criteria).unwrap();

assert_eq!(result, r#"{"name":"Alice","score":99}"#);
```

## API

### `FilterCriteria`

```rust
// From a slice of path strings
let c = FilterCriteria::new(&["shipping.address.city", "order_id"]).unwrap();

// From a Vec<&str>
let c = FilterCriteria::try_from(vec!["customer.name"]).unwrap();
```

`FilterCriteria::new` returns `Result<FilterCriteria, FilterError>` and validates every path at construction time. Invalid paths (see [Path syntax](#path-syntax) below) return `Err(FilterError::InvalidCriteria(...))` with a description of the problem.

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
    InvalidJson(String),      // malformed JSON with a description
    UnexpectedEof,            // input ended mid-value
    InvalidCriteria(String),  // bad path syntax with a description
}
```

## Path syntax

Paths are dot-separated key names that describe a route through the JSON hierarchy. Array elements are selected with bracket notation after the key name.

| Syntax | Meaning |
|---|---|
| `name` | Top-level key `name` |
| `customer.name` | Key `name` nested inside `customer` |
| `items[*].price` | `price` field of every element in `items` |
| `items[0].price` | `price` field of the first element only |
| `items[:3].price` | `price` field of the first three elements |
| `items[1:4].price` | `price` field of elements at index 1, 2, and 3 |
| `items[2:].price` | `price` field of all elements from index 2 onwards |
| `[*].name` | `name` field of every element in a top-level array |
| `[:]` | Every element of a top-level array (equivalent to `[*]`) |

### Validation

Paths are validated when `FilterCriteria::new` is called. The following are all errors:

- Empty string
- Leading, trailing, or consecutive dots (`.name`, `name.`, `a..b`)
- Empty bracket selector (`items[]`)
- Unclosed bracket (`items[`)
- Non-integer, non-wildcard bracket content (`items[abc]`)
- Invalid slice bounds (`items[x:3]`, `items[1:y]`)
- Trailing characters after a closing bracket (`items[0]extra`)

## Behaviour notes

**Output is always compact.** Whitespace in the input is not preserved; the output has no extra spaces or newlines.

**Entire subtrees are matched.** If a criterion points at an object, the whole subtree is included or excluded — you don't need to enumerate every leaf:

```rust
// Includes the full "payment" object with all its nested fields
let c = FilterCriteria::new(&["payment"]).unwrap();
```

**Array selectors apply to both inclusion and exclusion.** `[*]` recurses into every element; `[n]` targets a specific index; slices target a range:

```rust
// Removes "price" from every object in the "items" array
let c = FilterCriteria::new(&["items[*].price"]).unwrap();
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
