# Capabilities

The `capabilities` module provides public constants and validation functions for
host requirement capability names used in OpenJD templates (§3.3).

## Standard Capabilities

### Amount Capabilities

```rust
pub const STANDARD_AMOUNT_CAPABILITIES: &[&str] = &[
    "amount.worker.vcpu",
    "amount.worker.memory",
    "amount.worker.gpu",
    "amount.worker.gpu.memory",
    "amount.worker.disk.scratch",
];
```

### Attribute Capabilities

```rust
pub const STANDARD_ATTRIBUTE_CAPABILITIES: &[(&str, &[&str])] = &[
    ("attr.worker.os.family", &["linux", "windows", "macos"]),
    ("attr.worker.cpu.arch", &["x86_64", "arm64"]),
];

pub const STANDARD_ATTRIBUTE_CAPABILITY_NAMES: &[&str] =
    &["attr.worker.os.family", "attr.worker.cpu.arch"];
```

## Validation Functions

### `validate_amount_capability_name(name) -> Result<(), String>`

Validates that `name` matches the amount capability regex:
`(?i)^([A-Za-z_][A-Za-z0-9_]*:)?amount\.[A-Za-z_][A-Za-z0-9_]*(\.[A-Za-z_][A-Za-z0-9_]*)*$`

The optional prefix before `:` is a vendor namespace. The `amount.` prefix is required,
followed by dot-separated identifier segments.

### `validate_attribute_capability_name(name) -> Result<(), String>`

Validates that `name` matches the attribute capability regex:
`(?i)^([A-Za-z_][A-Za-z0-9_]*:)?attr\.[A-Za-z_][A-Za-z0-9_]*(\.[A-Za-z_][A-Za-z0-9_]*)*$`

Same structure as amount capabilities but with the `attr.` prefix.

## Implementation Note

Both validation functions delegate to pre-compiled regexes in
`validate_v2023_09::helpers` (`AMOUNT_CAP_RE`, `ATTR_CAP_RE`). The `capabilities`
module re-exports the standard capability lists that also appear in `helpers` —
the `helpers` versions are used internally during validation, while the `capabilities`
versions form the public API.
