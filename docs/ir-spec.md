# Linxts Unified IR v0.2

The IR schema version is `0.2`. A module contains a source snapshot hash, a top-level block, and functions.

## Nodes

Implemented statements:

- variable declaration
- assignment
- print
- expression statement
- conditional
- condition-controlled loop
- return

Implemented expressions:

- integer, string, boolean, and unit literals
- variable reference
- unary negate/not
- arithmetic, comparison, equality, and boolean binary operations
- function calls
- format expressions composed of literal text and recursively validated value segments
- parser error nodes (invalid at the validator boundary)

Every statement and expression carries a source span. Expressions carry a provisional type and effect; statements carry effect metadata. v0.2 inference is intentionally limited and later semantic passes will refine these fields.

## Format expressions

`Format` gives source-language adapters one shared interpolation representation. A format expression contains an ordered list of `Text` and `Value` segments. Backends evaluate value segments in order, convert integer, boolean, string, and unit values with Linxts semantics, concatenate the results, and return one string.

The current adapters lower these forms:

- Python `f"value={x}"`
- JavaScript `` `value=${x}` ``
- Rust `"value={}", x` and captured `"value={x}"`
- C `"value=%d", x` (`%d`, `%i`, `%s`, and `%f` placeholders)

This is a bounded placeholder subset, not a claim of full source-language formatting compatibility. Format specifications outside the subset are not interpreted as supported placeholders.

## JSON representation

Enum nodes use adjacent tags: `{"kind":"print","data":{...expression...}}`. This avoids key collisions between a statement/expression wrapper and nested node kinds and guarantees JSON round-tripping. The AI protocol publishes recursive definitions for modules, blocks, functions, statements, expressions, literals, types, effects, and spans; typed deserialization and the validator remain authoritative.

## Validation

The validator rejects:

- schema versions other than `0.2`
- duplicate functions and parameters
- duplicate declarations in one scope
- assignment or reference to undeclared variables
- calls to unknown functions or calls with incorrect arity
- return outside a function
- parser error nodes
- nesting deeper than 128 nodes

Validation proves structural admissibility, not that a repair or AI model understood the user's intent. High-risk ambiguity requires rejection or future user confirmation.

## Cross-language semantics

IR operations are defined by Linxts semantics, not by assuming source languages behave identically. The current integer VM uses saturating addition, subtraction, multiplication, and integer division. Future adapters must record language-specific conversion decisions in lowering diagnostics.
