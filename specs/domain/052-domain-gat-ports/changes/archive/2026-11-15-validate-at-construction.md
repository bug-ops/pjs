---
tags:
  - sdd
  - delta
created: 2026-11-15
status: merged
related:
  - "[[052-domain-gat-ports/spec|spec]]"
---

# Delta: validate-at-construction

Merged into [[052-domain-gat-ports/spec|spec]] on 2026-11-15.

## MODIFIED
- FR-009: `Pagination::new` validates; fields are private and `validate()` is gone (#604).
- FR-010: `SessionQueryCriteria` and `StreamFilter` are built with builders whose `build()` validates (#604). `SessionTimeout` bounds session timeouts (#596, see 053).
- FR-013: `DictionaryStore` is not feature-gated and uses `TrainedDictionary` and `DomainResult` (#605).

## REMOVED
- `StreamFilter.min_priority` and `max_priority`, the `validate()` methods, and open issues #604 and #605.
