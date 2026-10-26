# Deterministic repair engine

Repair runs only after an initial parse fails. It is bounded to four passes and reparses the complete candidate after changes.

Each repair records:

- stable per-compilation repair ID
- original source span
- original and repaired text
- repair type
- reason
- semantic risk (`low`, `medium`, or `high`)

Implemented v0.1 rules close unmatched quotes, parentheses, brackets, and braces and correct a small allowlist of output-call typos. Delimiter scanning ignores delimiters inside strings. When an unterminated quote is followed by an existing structural closer, the quote is inserted before that closer and the next bounded pass rescans the whole candidate; this avoids inventing duplicate parentheses or braces from stale delimiter state.

Aggressive mode accepts deterministic low- and medium-risk rules. Strict mode permits only low-risk repair, so it can close a parenthesis but will not infer a missing block brace or spelling correction.

Repair does not introduce file access, process execution, networking, secret handling, deletion, or any capability not already explicit in the source. Parser and validator limits prevent unbounded retry or nesting behavior.
