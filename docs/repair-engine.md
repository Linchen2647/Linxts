# Deterministic repair engine

Repair runs only after an initial parse fails. It is bounded to four passes and reparses the complete candidate after changes.

Each repair records:

- stable per-compilation repair ID
- original source span
- original and repaired text
- repair type
- reason
- semantic risk (`low`, `medium`, or `high`)

Implemented rules close unmatched quotes, parentheses, brackets, and braces; correct a small allowlist of output-call typos; and move a duplicated quote after contiguous numeric content in a recognized `print`/`println` call, such as repairing `print(""666)` to `print("666")`. The numeric quote rule is bounded to an output-call argument immediately followed by `)` and is recorded as medium risk. Delimiter scanning ignores delimiters inside strings. When an unterminated quote is followed by an existing structural closer, the quote is inserted before that closer and the next bounded pass rescans the whole candidate; this avoids inventing duplicate parentheses or braces from stale delimiter state.

The compiler currently invokes automatic repair only for Python, JavaScript, and C segments. Rust and other code adapters must parse successfully using their documented subset syntax; malformed segments receive `LXT-R02` and are not forwarded to AI fallback. This keeps correction work language-specific instead of applying cross-language guesses.

The first language-specific rules are:

- Python: `pritn`/`pirnt` output-call typos and a missing `:` when a supported block header is immediately followed by a more-indented suite;
- JavaScript: `consle.log` and `console,log` output-call typos;
- C: `pritnf` and `printif` output-call typos.

Allowlisted language-specific rewrites run before parsing. This matters for typo-shaped function calls: they may be structurally parseable but would otherwise survive until IR validation as unknown functions. Every rewrite still emits a repair record and strict mode rejects these medium-risk changes.

Aggressive mode accepts deterministic low- and medium-risk rules. Strict mode permits only low-risk repair, so it can close a parenthesis but will not infer a missing block brace or spelling correction.

Repair does not introduce file access, process execution, networking, secret handling, deletion, or any capability not already explicit in the source. Parser and validator limits prevent unbounded retry or nesting behavior.
