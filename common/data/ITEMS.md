# Planned item manifest

`items-v01.tsv` is the versioned source for the 270 planned inventory definitions. It was imported from [the TK planning document](https://linear.app/tarkov-killer/document/catalogo-canonico-de-itens-270-definicoes-autorais-v01-6eecc24314f1), revision 0.1, as of 2026-11-15. The import concatenated the 12 TSV blocks in document order, retaining one header and every data row.

Original import SHA-256: `7b60be06f2b476f8bce426fb7ed56d06fb58bab56033d712c57c8e9ad901be15`.

TSV preserves the original authoring text and avoids maintaining a second copy of the data in RON. The shared Rust catalog exposes these rows as planning metadata, separately from active game definitions. The six S1 seed IDs select rows for the first slice; they do not enable item spawning, use, equipment, or attachment. Asset and instantiation checks belong at the point where definitions are promoted into gameplay.
