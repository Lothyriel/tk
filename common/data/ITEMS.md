# Planned item manifest

`items-v01.tsv` is the versioned source for the 270 planned inventory definitions. It was imported from [the TK planning document](https://linear.app/tarkov-killer/document/catalogo-canonico-de-itens-270-definicoes-autorais-v01-6eecc24314f1), revision 0.1, as of 2026-11-15. The import concatenated the 12 TSV blocks in document order, retaining one header and every data row.

Original import SHA-256: `7b60be06f2b476f8bce426fb7ed56d06fb58bab56033d712c57c8e9ad901be15`.

TSV preserves the original authoring text and avoids maintaining a second copy of the data in RON. The six S1 seed IDs select rows for the shared inventory fixtures; they do not enable item spawning, use, equipment, or attachment.

`ActiveItems::validate` accepts an explicit list of item-to-visual bindings and resolves each logical asset key against the caller's validated visual catalog. Any invalid row rejects the entire list. Unlisted rows remain planned, even when they share a profile with a listed row. The visual pipeline owns asset validation and supplies the lookup; this shared layer does not load client files or wire gameplay. No production item bindings are shipped here.
