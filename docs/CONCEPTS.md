# Concepts

Shared domain vocabulary for this project — entities, named processes, and status concepts with project-specific meaning. Seeded with core domain vocabulary, then accretes as ce-compound and ce-compound-refresh process learnings; direct edits are fine. Glossary only, not a spec or catch-all.

## Transfer Restoration

### Download Session
A durable snapshot of unfinished GUI transfers containing the source identity and destination needed to reconstruct them after a graceful application close.

### Download Session Record
One persisted transfer identity within a Download Session; newly created records require queue acceptance and terminal transfers remove their records, while a failed source refetch initially retains a record unless bulk cancellation or a later session drain clears it.

### Restored Download
A reconstructed unfinished transfer that begins paused and obtains resumable progress from its partial output, requiring an explicit resume before transfer work continues.

### Bulk Transfer Control
The Home-screen action that pauses or resumes every visible transfer, derived from their current pause states rather than separately cached UI state.
