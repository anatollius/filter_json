use filter_json::{FilterCriteria, filter_json, filter_json_exclude};
use std::fs;

fn fixture(name: &str) -> String {
    fs::read_to_string(format!("tests/fixtures/{name}"))
        .unwrap_or_else(|e| panic!("failed to read fixture {name}: {e}"))
}

// ── Inclusion: exact string comparison (leaf values only) ───────────────────

#[test]
fn include_three_levels_deep() {
    let json = fixture("ecommerce_order.json");
    let c = FilterCriteria::new(&["shipping.address.city"]).unwrap();
    assert_eq!(
        filter_json(&json, &c).unwrap(),
        r#"{"shipping":{"address":{"city":"Springfield"}}}"#
    );
}

#[test]
fn include_multiple_sibling_fields() {
    let json = fixture("ecommerce_order.json");
    let c = FilterCriteria::new(&["customer.name", "customer.email"]).unwrap();
    assert_eq!(
        filter_json(&json, &c).unwrap(),
        r#"{"customer":{"name":"Alice Smith","email":"alice@example.com"}}"#
    );
}

#[test]
fn include_fields_from_different_branches() {
    let json = fixture("ecommerce_order.json");
    let c = FilterCriteria::new(&["order_id", "totals.grand_total"]).unwrap();
    assert_eq!(
        filter_json(&json, &c).unwrap(),
        r#"{"order_id":"ORD-12345","totals":{"grand_total":54.56}}"#
    );
}

#[test]
fn include_four_levels_deep() {
    let json = fixture("user_profile.json");
    let c = FilterCriteria::new(&["profile.location.city"]).unwrap();
    assert_eq!(
        filter_json(&json, &c).unwrap(),
        r#"{"profile":{"location":{"city":"San Francisco"}}}"#
    );
}

#[test]
fn include_nested_boolean() {
    let json = fixture("user_profile.json");
    let c = FilterCriteria::new(&["settings.notifications.push"]).unwrap();
    assert_eq!(
        filter_json(&json, &c).unwrap(),
        r#"{"settings":{"notifications":{"push":true}}}"#
    );
}

#[test]
fn include_two_nested_booleans() {
    let json = fixture("user_profile.json");
    let c = FilterCriteria::new(&[
        "settings.notifications.email",
        "settings.notifications.push",
    ]).unwrap();
    assert_eq!(
        filter_json(&json, &c).unwrap(),
        r#"{"settings":{"notifications":{"email":true,"push":true}}}"#
    );
}

#[test]
fn include_fields_from_different_top_levels() {
    let json = fixture("user_profile.json");
    let c = FilterCriteria::new(&["username", "stats.followers"]).unwrap();
    assert_eq!(
        filter_json(&json, &c).unwrap(),
        r#"{"username":"johndoe","stats":{"followers":389}}"#
    );
}

// ── Inclusion: subtree (contains/not-contains) ──────────────────────────────

#[test]
fn include_entire_subtree() {
    let json = fixture("ecommerce_order.json");
    let c = FilterCriteria::new(&["payment"]).unwrap();
    let result = filter_json(&json, &c).unwrap();
    assert!(result.contains("credit_card"), "should contain credit_card");
    assert!(result.contains("4242"), "should contain 4242");
    assert!(!result.contains("order_id"), "should not contain order_id");
    assert!(!result.contains("customer"), "should not contain customer");
}

// ── Escape sequences: inline JSON ───────────────────────────────────────────

#[test]
fn include_field_with_tab_in_key() {
    // JSON key "a\tb" (escaped tab) matched by criteria containing an actual tab char.
    // push_json_key re-escapes the tab back to \t in the output.
    let json = r#"{"a\tb": 1, "other": 2}"#;
    let c = FilterCriteria::new(&["a\tb"]).unwrap(); // Rust \t = actual tab character
    let result = filter_json(json, &c).unwrap();
    assert_eq!(result, r#"{"a\tb":1}"#);
}

#[test]
fn include_field_with_unicode_escape_in_key() {
    // JSON key "\u006E\u0061\u006D\u0065" decodes to "name"; plain "name" criteria matches.
    let json = r#"{"\u006E\u0061\u006D\u0065": "value", "age": 30}"#;
    let c = FilterCriteria::new(&["name"]).unwrap();
    let result = filter_json(json, &c).unwrap();
    assert_eq!(result, r#"{"name":"value"}"#);
}

#[test]
fn include_value_with_unicode_escape() {
    // \u escape inside a value is copied verbatim (raw bytes), not decoded.
    let json = r#"{"name": "\u0048ello", "other": 1}"#;
    let c = FilterCriteria::new(&["name"]).unwrap();
    let result = filter_json(json, &c).unwrap();
    assert_eq!(result, r#"{"name":"\u0048ello"}"#);
}

// ── Exclusion ────────────────────────────────────────────────────────────────

#[test]
fn exclude_entire_top_level_subtree() {
    let json = fixture("ecommerce_order.json");
    let c = FilterCriteria::new(&["payment"]).unwrap();
    let result = filter_json_exclude(&json, &c).unwrap();
    assert!(!result.contains("payment"), "should not contain payment");
    assert!(
        !result.contains("credit_card"),
        "should not contain credit_card"
    );
    assert!(result.contains("order_id"), "should contain order_id");
    assert!(result.contains("customer"), "should contain customer");
}

#[test]
fn exclude_nested_field_preserves_siblings() {
    let json = fixture("ecommerce_order.json");
    let c = FilterCriteria::new(&["customer.phone"]).unwrap();
    let result = filter_json_exclude(&json, &c).unwrap();
    assert!(!result.contains("phone"), "should not contain phone");
    assert!(
        !result.contains("+1-555-0100"),
        "should not contain phone value"
    );
    assert!(result.contains("Alice Smith"), "should contain name value");
    assert!(
        result.contains("alice@example.com"),
        "should contain email value"
    );
}

#[test]
fn exclude_multiple_top_level_fields() {
    let json = fixture("ecommerce_order.json");
    let c = FilterCriteria::new(&["payment", "status"]).unwrap();
    let result = filter_json_exclude(&json, &c).unwrap();
    assert!(!result.contains("payment"), "should not contain payment");
    assert!(
        !result.contains("shipped"),
        "should not contain status value"
    );
    assert!(result.contains("order_id"), "should contain order_id");
    assert!(result.contains("items"), "should contain items");
}

#[test]
fn exclude_field_from_nested_array_elements() {
    let json = fixture("ecommerce_order.json");
    let c = FilterCriteria::new(&["items[*].price"]).unwrap();
    let result = filter_json_exclude(&json, &c).unwrap();
    assert!(
        !result.contains("\"price\""),
        "should not contain price key"
    );
    assert!(
        !result.contains("10.50"),
        "should not contain first price value"
    );
    assert!(result.contains("WIDGET-A"), "should contain first sku");
    assert!(result.contains("GADGET-B"), "should contain second sku");
}

#[test]
fn exclude_deeply_nested_field() {
    let json = fixture("user_profile.json");
    let c = FilterCriteria::new(&["settings.privacy"]).unwrap();
    let result = filter_json_exclude(&json, &c).unwrap();
    assert!(!result.contains("privacy"), "should not contain privacy");
    assert!(
        !result.contains("show_email"),
        "should not contain show_email"
    );
    assert!(result.contains("theme"), "should contain theme");
    assert!(
        result.contains("notifications"),
        "should contain notifications"
    );
}

#[test]
fn company_data() {
    let json = fixture("company_data.json");
    let c = FilterCriteria::new(&[
        "scores_and_limits.limit_history[*].credit_limit",
        "scores_and_limits.limit_history[*].cs_company_id",
    ]).unwrap();
    let result = filter_json(&json, &c).unwrap();
    println!("{result}");
}

#[test]
fn include_field_from_array_elements() {
    let json = fixture("ecommerce_order.json");
    let c = FilterCriteria::new(&["items[*].sku"]).unwrap();
    let result = filter_json(&json, &c).unwrap();
    assert!(result.contains("WIDGET-A"), "should contain first sku");
    assert!(result.contains("GADGET-B"), "should contain second sku");
    assert!(!result.contains("price"), "should not contain price");
    assert!(!result.contains("qty"), "should not contain qty");
}

#[test]
fn include_single_array_element() {
    let json = fixture("ecommerce_order.json");
    let c = FilterCriteria::new(&["items[1]"]).unwrap();
    let result = filter_json(&json, &c).unwrap();
    assert!(
        result.contains("GADGET-B"),
        "should contain second item sku"
    );
    assert!(
        !result.contains("WIDGET-A"),
        "should not contain first item sku"
    );
}
