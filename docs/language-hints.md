# Language hints

A hint marker starts with `^` after optional indentation and influences following source until the next marker. A known short alias may share its line with the first statement, such as `^python print("Hello")`; the scanner preserves that trailing source instead of swallowing it.

Known aliases are case-insensitive and normalized across spaces, hyphens, underscores, and common descriptive forms.

| Language | Examples |
| --- | --- |
| Python | `^python`, `^py`, `^這是python` |
| JavaScript | `^javascript`, `^js`, `^node`, `^接下來用JavaScript` |
| Rust | `^rust`, `^rs`, `^這是rust` |
| C | `^c`, `^C語言`, `^這是C` |
| Natural language | `^natural`, `^nl`, `^自然語言`, `^自然语言` |

Unknown compact markers are preserved in the scan result for future AI classification. They do not silently select a language.

A caret inside a string is not a marker. The scanner tracks single- and double-quoted strings across lines. A caret embedded after ordinary source tokens is ordinary text. Unknown hints containing whitespace are not treated as markers, preventing an unsupported phrase from silently consuming source code.

Without a marker, the detector combines syntax features, token patterns, parser compatibility as the implementation evolves, and eventually AI classification only when deterministic evidence is insufficient.
