# Forager integration

The cost order is `direct retrieval → ordinary search → research`.

- Delegate multi-page evidence gathering to a subagent that returns conclusions and citations.
- For bulk work, persist bulk evidence outside the main context with `--output FILE --receipt` and read it on demand. `--output` alone also prints the full result.
- Run a direct fetch in the main context only for a single-page spot check.
