---
title: "How to Clone a Postgres Database Into a Safe Sandbox"
excerpt: "A practical cloning workflow for agent database tasks: dump from an approved source, restore into a tracked Postgres sandbox, use scoped credentials, and clean up failed restores."
author: "PGSandbox Team"
status: "published"
publishedAt: "2026-06-29"
updatedAt: "2026-06-29T18:08:00Z"
tags: ["postgres", "database-cloning", "pg-dump", "ai-agents", "mcp"]
category: "Engineering"
metaTitle: "Clone a Postgres Database Into a Sandbox"
metaDescription: "Clone a Postgres database into a safe disposable sandbox with pg_dump, pg_restore, scoped roles, cleanup, and agent-ready database boundaries."
canonicalUrl: "https://pgsandbox-mcp.lvtd.dev/blog/clone-postgres-database-sandbox/"
heroImageUrl: ""
featured: false
sortOrder: 40
---
To clone a Postgres database safely for agent work, do not give the agent the source database as its workspace. Read from an approved source, create a fresh disposable destination, restore into that destination with scoped credentials, run the task there, and delete the sandbox when the task is done.

That pattern keeps the useful part of cloning: realistic schema and data shape. It removes the dangerous part: letting a coding agent mutate the source database while it tries to prove a migration, generated SQL query, bug reproduction, or seed-state change.

The short version:

1. Choose the smallest approved source database that proves the task.
2. Create a new destination database for this task.
3. Create a role scoped to that destination.
4. Run `pg_dump` against the source.
5. Stream or restore the dump into the destination with `pg_restore`.
6. Run the agent's SQL only against the destination.
7. Clean up the sandbox, especially when restore fails halfway through.

That is the core clone workflow behind PGSandbox MCP's `clone_database` tool. It uses standard PostgreSQL client tools, but wraps them in a safer lifecycle: one task, one database, one scoped role, TTL metadata, and tracked cleanup.

## When cloning is the right move

Clone a Postgres database when schema-only setup is not enough and a coding agent needs realistic database behavior to validate work.

Good use cases include:

- Testing a migration against real tables and constraints.
- Reproducing a bug that depends on current schema shape.
- Checking generated SQL against realistic indexes and row shapes.
- Loading a known seed state before a backend change.
- Giving an agent production-shaped structure without handing it production as its workbench.

Cloning is usually too heavy for tiny unit-test fixtures. It is also the wrong default for sensitive production data. If the task can be proven with schema only, synthetic fixtures, or a reduced non-production dataset, start there.

PostgreSQL's own documentation says `pg_dump` exports a single database and makes a consistent export while the database is being used concurrently (https://www.postgresql.org/docs/current/app-pgdump.html). That makes `pg_dump` a practical tool for copying source state into a temporary target. It does not make every source safe to clone into an agent workflow.

The data decision comes first.

## The safe clone model

A safe Postgres clone for agent work has two boundaries:

- The source database is read as a source.
- The destination database is where the agent works.

Do not blur those. The agent should not "clone" by connecting to the source, running setup SQL there, and hoping it remembers to undo its work. The destination should be a separate database that exists for one task.

The safer lifecycle looks like this:

| Step | Authority | What happens |
| --- | --- | --- |
| Create destination | Admin lifecycle credential | Create one database and one login role for the task |
| Dump source | Approved source credential | Export schema and data, or schema only |
| Restore target | Destination sandbox role | Create objects and load data into the sandbox |
| Run task SQL | Destination sandbox role | Let the agent migrate, seed, inspect, or test |
| Cleanup | Tracked lifecycle credential | Delete only the database and role created for the task |

This is not about making SQL harmless. It is about making the place where SQL runs disposable.

PostgreSQL requires superuser or `CREATEDB` privilege to create a database (https://www.postgresql.org/docs/current/sql-createdatabase.html). That privilege belongs in the lifecycle layer, not in the normal task SQL the agent uses. PostgreSQL privileges are managed with `GRANT`, which can grant object privileges or role membership (https://www.postgresql.org/docs/current/sql-grant.html). Use that separation: admin authority creates the sandbox, scoped role authority runs inside it.

## A plain pg_dump and pg_restore workflow

If you are doing this manually, the simplest portable workflow is:

```bash
createdb cloned_task_db
pg_dump --format=custom --no-owner --no-privileges "$SOURCE_DATABASE_URL" > source.dump
pg_restore --no-owner --no-privileges --exit-on-error --single-transaction \
  --dbname "$DESTINATION_DATABASE_URL" source.dump
```

The important flags are not decoration.

`--format=custom` creates an archive format that works with `pg_restore`. PostgreSQL's `pg_dump` docs describe custom and directory archive formats as flexible formats for selecting and reordering restore items (https://www.postgresql.org/docs/current/app-pgdump.html).

`--no-owner` avoids emitting ownership changes that try to recreate the source ownership model in the destination. The Postgres docs say `--no-owner` prevents commands that set object ownership to match the original database, which would otherwise require a superuser or matching owner in many restores (https://www.postgresql.org/docs/current/app-pgdump.html).

`--no-privileges` avoids carrying source grants into the destination. For an agent sandbox, the source database's privilege graph is usually not what you want. You want the destination role to own or access the restored objects inside the sandbox.

`--exit-on-error` and `--single-transaction` make restore failure easier to reason about. PostgreSQL's `pg_restore` docs say `--transaction-size` implies `--exit-on-error`, and compare it with `--single-transaction`, where one transaction covers all restored objects (https://www.postgresql.org/docs/current/app-pgrestore.html). For task sandboxes, failing fast is better than leaving an agent to debug a half-restored target.

For schema-only work, add:

```bash
pg_dump --format=custom --no-owner --no-privileges --schema-only "$SOURCE_DATABASE_URL" > source-schema.dump
```

Schema-only clones are useful when production data is sensitive but real DDL, constraints, extensions, indexes, and table relationships matter.

## What PGSandbox MCP adds

PGSandbox MCP does not invent a new backup format. Its first clone backend uses `pg_dump` and `pg_restore`.

The useful part is the control plane around those tools.

PGSandbox's `clone_database` tool creates an empty tracked sandbox first, then pipes `pg_dump` output from the source into `pg_restore` connected as the generated sandbox role. The [MCP tool contract](https://pgsandbox-mcp.lvtd.dev/docs/mcp-tools/) documents that `clone_database` accepts a `sourceDatabaseUrl`, optional `ttlMinutes`, `owner`, labels, and `schemaOnly`, then returns the sandbox database id, role name, expiry, and connection string.

The [architecture notes](https://pgsandbox-mcp.lvtd.dev/docs/architecture/) describe the clone backend in four steps:

1. Create an empty sandbox database and scoped role.
2. Run `pg_dump` against the source with ownership and privileges omitted.
3. Stream the dump into `pg_restore` connected as the sandbox role.
4. Delete the destination sandbox if the clone fails.

That last step is easy to skip in a manual script. It matters for agents because interrupted or failed restores are normal when tools are being called from long-running coding sessions. A failed restore should not leave a broken destination that looks like a valid task database.

The implementation also keeps database URLs out of command arguments. The pg tools receive connection details through environment variables and the database name argument, so shell process lists are less likely to expose full URLs with passwords. The repo has a regression test that checks the generated dump and restore arguments do not contain `postgres://` or a sample secret.

That is the information-gain layer for this workflow: cloning is not one command. For agent work, cloning is a lifecycle with a cleanup contract.

## How to clone with PGSandbox MCP

Use `clone_database` when the agent needs a realistic source state in a disposable target.

The exact MCP client UI differs by Codex, Cursor, VS Code, Claude Desktop, and other clients, but the tool inputs are conceptually the same:

```json
{
  "profile": "local-pg17",
  "sourceDatabaseUrl": "postgres://source_user:***@localhost:5432/app_dev",
  "nameHint": "migration-check",
  "ttlMinutes": 120,
  "owner": "codex-task-123",
  "labels": {
    "repo": "example-api",
    "branch": "add-invoice-status"
  },
  "schemaOnly": false
}
```

Do not paste an unmasked production URL into a prompt or transcript. Prefer a secret input, local environment variable, or profile-level configuration. If a human needs to approve the clone source, make that approval explicit before the agent calls the tool.

After the clone returns, the agent should use the sandbox connection string for task SQL:

- Run migrations.
- Insert or update seed data.
- Execute generated SQL.
- Inspect schema with the MCP `describe_schema` tool.
- Check the exact failure the backend code is supposed to fix.
- Delete the sandbox when the work is complete.

The destination is now the agent's workspace. The source is not.

## Source data rules for agent clones

The safest clone is the smallest clone that proves the task.

Use this order of preference:

1. Schema-only clone.
2. Synthetic seed database.
3. Reduced non-production snapshot.
4. Masked production-like snapshot.
5. Production source with explicit human approval and a clear reason.

That ordering is boring on purpose. Agents can copy outputs into logs, summaries, issue comments, or PR descriptions. Even a local MCP workflow can accidentally turn database contents into conversation context. Treat source data as leaving the database boundary once an agent can query it.

For sensitive data, a clone workflow should answer these questions before it runs:

- Who approved this source?
- Does the task need rows, or only schema?
- Is the source masked or reduced?
- How long will the sandbox live?
- Who owns cleanup?
- Can the agent read large tables, or are query results bounded?
- Where could restored data appear after the agent queries it?

The [Postgres MCP server safety checklist](https://pgsandbox-mcp.lvtd.dev/blog/postgres-mcp-server-safety-checklist/) goes deeper on scoped credentials, bounded result sets, and cleanup. The cloning-specific version is simpler: never let "realistic data" become an excuse for broad source access.

## Common mistakes

The most common cloning mistakes are operational, not syntax errors.

### Mistake 1: Restoring into shared development

If the target database is shared by a team, it is not an agent sandbox. The agent may still validate the task, but it can also leave state behind for the next developer. Create a dedicated destination instead.

### Mistake 2: Reusing source credentials for task SQL

The credential that can read the source should not become the credential that runs the agent's task. Separate source read authority from destination write authority.

### Mistake 3: Keeping source ownership and grants

Source ownership often fails to restore cleanly in another environment, and source grants are usually wrong for a task database. Use `--no-owner` and `--no-privileges` unless you have a specific reason not to.

### Mistake 4: Ignoring restore failure

If `pg_restore` fails, the destination is suspect. For disposable task databases, delete it and start again. Do not let the agent continue against a half-restored database unless the task is explicitly to inspect the failure.

### Mistake 5: Treating pg_dump as a production backup strategy

`pg_dump` is useful for logical exports and portable cloning. The PostgreSQL docs also caution that, except in simple cases, it is generally not the right choice for regular production backups (https://www.postgresql.org/docs/current/app-pgdump.html). Do not confuse an agent sandbox clone with your backup and recovery plan.

## FAQ

## What is the safest way to clone a Postgres database for an AI coding agent?

The safest default is to clone from an approved source into a new disposable database, give the agent only the destination credentials, and delete the destination after the task. The agent should not mutate the source database as part of cloning.

## Should I clone production data into an agent sandbox?

Only when the task truly requires production-shaped data and a human has approved the source. Prefer schema-only clones, synthetic data, reduced snapshots, or masked production-like data first.

## Is pg_dump safe to run on a live database?

PostgreSQL documents `pg_dump` as making consistent exports while the database is being used concurrently, without blocking readers or writers. That makes it practical for logical exports, but source-data approval and sensitivity still matter.

## Why restore as a sandbox role instead of an admin role?

Restoring as the sandbox role keeps the destination closer to the authority the agent will actually use. Admin credentials should create and clean up the sandbox; task credentials should run inside it.

## Does PGSandbox replace database branching?

No. Database branching is still useful for preview environments, staging, QA, and team workflows. A disposable Postgres sandbox is better when one agent task needs one temporary database. The [database branching comparison](https://pgsandbox-mcp.lvtd.dev/blog/database-branching-vs-postgres-sandboxes/) explains the split.

## Bottom line

Cloning a Postgres database for agent work is safe only when the destination is disposable and scoped.

Use `pg_dump` and `pg_restore` as the portable mechanics. Put a lifecycle around them: create a task database, restore into it with a scoped role, run the agent's work there, and clean it up when the task ends or the restore fails.

That is the difference between "the agent can clone a database" and "the agent has a safe database clone to prove this task." If you want that lifecycle through MCP, start with the [PGSandbox install guide](https://pgsandbox-mcp.lvtd.dev/docs/install/) and give the agent a real Postgres target that is private, tracked, and disposable.
