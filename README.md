# Welcome to `filter_json`

`filter_json` is a tool that enables you to filter JSON text without full deserialization. By providing a set of filter criteria alongside your JSON to the tool, it will transform the data to include only what you need.

```rust
let a = r#"{"customer": {"name": "Tom", "age": 24}}"#

let criteria = InclusionCriteria::from(vec!["customer.name"])
let filtered = filter_json(a.bytes().iter(), criteria).collect()

assert_eq!(filtered, r#"{"customer": {"name": "Tom"}}"#)
```
