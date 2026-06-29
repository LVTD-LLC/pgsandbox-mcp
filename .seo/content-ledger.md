# SEO Content Ledger

## Shipped

| Date | Type | Title | Slug | Target Keyword | Primary Internal Links | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| 2026-06-28 | Blog post | Disposable Postgres Databases for AI Agent Workflows | disposable-postgres-for-ai-agents | disposable Postgres databases for AI agents | /, /docs/mcp-tools/, /docs/architecture/ | Existing Rowset post; do not duplicate. |
| 2026-06-29 | Guide / checklist | Postgres MCP Server Safety Checklist for Coding Agents | postgres-mcp-server-safety-checklist | postgres mcp server | /, /docs/mcp-tools/, /docs/architecture/, /blog/disposable-postgres-for-ai-agents/ | Rowset row 1578, status `published`. |

## Candidate Backlog

Last researched: 2026-06-29

| Rank | Score | Proposed Type | Title | Target Keyword | Volume | KD | Intent | SERP Read | Why It Fits |
| --- | ---: | --- | --- | --- | ---: | ---: | --- | --- | --- |
| 2 | 21 | Comparison | Database Branching vs Disposable Postgres Sandboxes for Agent Workflows | database branching | 210 | 9 | Informational | SERP is dominated by database branching explainers and vendor pages from Xata, Supabase, Neon, SingleStore, PlanetScale, DoltHub, and Redgate. | Captures a growing category and gives PGSandbox a clear mental-model page against hosted branching tools. Strong information gain: when agents need disposable local proof rather than long-lived branch environments. |
| 3 | 20 | How-to / tutorial | How to Clone a Postgres Database Into a Safe Disposable Sandbox | postgres clone database | 110 | 3 | Informational | SERP includes Stack Overflow, DBA StackExchange, Atlassian, pgcopydb, and practical pg_dump/pg_restore guides. | Very winnable and aligned with the next major product capability. Can use the repo's clone flow as first-hand implementation detail. |
| 4 | 18 | Definition / guide | What Is a Database Sandbox? | database sandbox | 90 | 15 | Informational | Results skew broad and mixed across vendor docs, online sandboxes, and educational material. | Good category-creation page, but slightly less direct than Postgres/MCP queries and broader intent may dilute conversion. |
| 5 | 16 | Tutorial | How to Create a Test Postgres Database for Agent-Generated SQL | postgres test database | 20 | 2 | Informational | Low-volume long-tail with practical developer intent. | Very winnable, but lower ceiling. Good future support piece for migration validation. |

## Notes

- Keyword data source: DataForSEO Google keyword overview and suggestions, United States / English, pulled 2026-06-29.
- SERP read source: web search fallback on 2026-06-29 because DataForSEO live SERP endpoint disconnected repeatedly from this environment.
- Treat `postgres mcp server` and `mcp postgres` as related but not identical: the former is broader informational discovery; the latter appears navigational and may skew toward specific registries/repos.
