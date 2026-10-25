# Linxts Unified IR v0.1

The IR schema version is `0.1`. A module contains a source snapshot hash, a top-level block, and functions.

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
- parser error nodes (invalid at the validator boundary)

Every statement and expression carries a source span. Expressions carry a provisional type and effect; statements carry effect metadata. v0.1 inference is intentionally limited and later semantic passes will refine these fields.

## Validation

The validator rejects:

- schema versions other than `0.1`
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
