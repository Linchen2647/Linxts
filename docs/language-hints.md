# Language hints

A hint marker occupies a source line and starts with `^` after optional indentation. It influences following source until the next marker.

Known aliases are case-insensitive and normalized across spaces, hyphens, underscores, and common descriptive forms.

| Language | Examples |
| --- | --- |
| Python | `^python`, `^py`, `^這是python` |
| JavaScript | `^javascript`, `^js`, `^node`, `^接下來用JavaScript` |
| Rust | `^rust`, `^rs`, `^這是rust` |
| Natural language | `^natural`, `^nl`, `^自然語言`, `^自然语言` |

Unknown compact markers are preserved in the scan result for future AI classification. They do not silently select a language.

A caret inside a string is not a marker. The scanner tracks single- and double-quoted strings across lines. In v0.1, markers are line-oriented; a caret embedded after other source tokens is ordinary text.

Without a marker, the detector combines syntax features, token patterns, parser compatibility as the implementation evolves, and eventually AI classification only when deterministic evidence is insufficient.
