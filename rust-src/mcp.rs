use rmcp::{
    handler::server::wrapper::Parameters,
    model::{CallToolResult, Content},
    tool, tool_router, ErrorData, ServiceExt,
};
use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::{
    config::SandboxConfig,
    postgres::{
        CleanupExpiredInput, CloneDatabaseInput, CreateDatabaseInput,
        CreateSandboxFromTemplateInput, CreateSchemaSnapshotInput, CreateTemplateFromSandboxInput,
        DatabaseSelector, DeleteSchemaSnapshotInput, DeleteTemplateInput, DiffSchemaSnapshotInput,
        ExplainQueryInput, ListDatabasesInput, ListProfilesInput, ListSchemaSnapshotsInput,
        ListTemplatesInput, PostgresSandboxManager, PrepareForRepoInput, RunMigrationsInput,
        RunSqlInput, SchemaDiffInput, SeedDatabaseInput, ValidateMigrationInput,
    },
    telemetry::{properties, Telemetry, EVENT_MCP_SERVER_STARTED, EVENT_MCP_TOOL_COMPLETED},
};

pub const PUBLIC_MCP_TOOLS: &[&str] = &[
    "list_profiles",
    "create_database",
    "clone_database",
    "delete_database",
    "get_connection_string",
    "run_sql",
    "describe_schema",
    "schema_digest",
    "schema_diff",
    "explain_query",
    "create_schema_snapshot",
    "list_schema_snapshots",
    "delete_schema_snapshot",
    "diff_schema_snapshot",
    "prepare_for_repo",
    "run_migrations",
    "validate_migration",
    "seed_database",
    "create_template_from_sandbox",
    "create_sandbox_from_template",
    "list_templates",
    "delete_template",
    "list_databases",
    "cleanup_expired",
];

pub const PUBLIC_MCP_TOOL_COUNT: usize = PUBLIC_MCP_TOOLS.len();

#[derive(Clone)]
pub struct PgsandboxServer {
    manager: PostgresSandboxManager,
    telemetry: Telemetry,
}

impl PgsandboxServer {
    pub fn new(config: SandboxConfig) -> Self {
        let telemetry = Telemetry::new(config.telemetry.clone());
        Self {
            manager: PostgresSandboxManager::new(config),
            telemetry,
        }
    }

    async fn tracked_tool<T, Fut>(
        &self,
        tool: &'static str,
        mut event_properties: Map<String, Value>,
        operation: Fut,
    ) -> Result<CallToolResult, ErrorData>
    where
        T: Serialize,
        Fut: std::future::Future<Output = anyhow::Result<T>>,
    {
        let started = std::time::Instant::now();
        let result = operation.await;
        event_properties.insert("tool".to_string(), json!(tool));
        event_properties.insert("success".to_string(), json!(result.is_ok()));
        event_properties.insert(
            "elapsedMs".to_string(),
            json!(started.elapsed().as_millis()),
        );
        self.telemetry
            .capture_background(EVENT_MCP_TOOL_COMPLETED, event_properties);
        tool_json(result)
    }
}

#[tool_router(server_handler)]
impl PgsandboxServer {
    #[tool(description = "List configured and discoverable Postgres profiles.")]
    async fn list_profiles(
        &self,
        Parameters(input): Parameters<ListProfilesInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let event_properties = properties([(
            "includeDiscoveredLocal",
            json!(input.include_discovered_local.unwrap_or(true)),
        )]);
        self.tracked_tool("list_profiles", event_properties, async {
            self.manager.list_profiles(input)
        })
        .await
    }

    #[tool(description = "Create an isolated Postgres sandbox database and login role.")]
    async fn create_database(
        &self,
        Parameters(input): Parameters<CreateDatabaseInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let event_properties = properties([
            ("hasProfile", json!(input.profile.is_some())),
            (
                "hasPostgresVersion",
                json!(input.postgres_version.is_some()),
            ),
            ("hasNameHint", json!(input.name_hint.is_some())),
            ("hasOwner", json!(input.owner.is_some())),
            ("hasTtlMinutes", json!(input.ttl_minutes.is_some())),
            (
                "labelCount",
                json!(input.labels.as_ref().map_or(0, |labels| labels.len())),
            ),
        ]);
        self.tracked_tool(
            "create_database",
            event_properties,
            self.manager.create_database(input),
        )
        .await
    }

    #[tool(description = "Clone an existing Postgres database into a new isolated sandbox.")]
    async fn clone_database(
        &self,
        Parameters(input): Parameters<CloneDatabaseInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let event_properties = properties([
            ("hasProfile", json!(input.profile.is_some())),
            (
                "hasPostgresVersion",
                json!(input.postgres_version.is_some()),
            ),
            ("hasNameHint", json!(input.name_hint.is_some())),
            ("hasOwner", json!(input.owner.is_some())),
            ("hasTtlMinutes", json!(input.ttl_minutes.is_some())),
            (
                "labelCount",
                json!(input.labels.as_ref().map_or(0, |labels| labels.len())),
            ),
            ("schemaOnly", json!(input.schema_only.unwrap_or(false))),
        ]);
        self.tracked_tool(
            "clone_database",
            event_properties,
            self.manager.clone_database(input),
        )
        .await
    }

    #[tool(description = "Delete a sandbox database and role created by PGSandbox.")]
    async fn delete_database(
        &self,
        Parameters(input): Parameters<DatabaseSelector>,
    ) -> Result<CallToolResult, ErrorData> {
        let event_properties = selector_properties(&input);
        self.tracked_tool(
            "delete_database",
            event_properties,
            self.manager.delete_database(input),
        )
        .await
    }

    #[tool(description = "Return the connection string for a sandbox database.")]
    async fn get_connection_string(
        &self,
        Parameters(input): Parameters<DatabaseSelector>,
    ) -> Result<CallToolResult, ErrorData> {
        let event_properties = selector_properties(&input);
        self.tracked_tool(
            "get_connection_string",
            event_properties,
            self.manager.get_connection_string(input),
        )
        .await
    }

    #[tool(description = "Run SQL against a sandbox database.")]
    async fn run_sql(
        &self,
        Parameters(input): Parameters<RunSqlInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let event_properties = properties([
            ("hasProfile", json!(input.profile.is_some())),
            ("hasDatabaseId", json!(input.database_id.is_some())),
            ("hasDatabaseName", json!(input.database_name.is_some())),
            ("readonly", json!(input.readonly.unwrap_or(false))),
            ("hasRowLimit", json!(input.row_limit.is_some())),
        ]);
        self.tracked_tool("run_sql", event_properties, self.manager.run_sql(input))
            .await
    }

    #[tool(description = "Return schema metadata for a sandbox database.")]
    async fn describe_schema(
        &self,
        Parameters(input): Parameters<DatabaseSelector>,
    ) -> Result<CallToolResult, ErrorData> {
        let event_properties = selector_properties(&input);
        self.tracked_tool(
            "describe_schema",
            event_properties,
            self.manager.describe_schema(input),
        )
        .await
    }

    #[tool(description = "Return a compact checksummed schema digest for a sandbox database.")]
    async fn schema_digest(
        &self,
        Parameters(input): Parameters<DatabaseSelector>,
    ) -> Result<CallToolResult, ErrorData> {
        let event_properties = selector_properties(&input);
        self.tracked_tool(
            "schema_digest",
            event_properties,
            self.manager.schema_digest(input),
        )
        .await
    }

    #[tool(description = "Compare a base schema_digest response with the current sandbox schema.")]
    async fn schema_diff(
        &self,
        Parameters(input): Parameters<SchemaDiffInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let event_properties = properties([
            ("hasProfile", json!(input.profile.is_some())),
            ("hasDatabaseId", json!(input.database_id.is_some())),
            ("hasDatabaseName", json!(input.database_name.is_some())),
        ]);
        self.tracked_tool(
            "schema_diff",
            event_properties,
            self.manager.schema_diff(input),
        )
        .await
    }

    #[tool(
        description = "Return a JSON EXPLAIN plan and compact summary for one sandbox SQL statement."
    )]
    async fn explain_query(
        &self,
        Parameters(input): Parameters<ExplainQueryInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let event_properties = properties([
            ("hasProfile", json!(input.profile.is_some())),
            ("hasDatabaseId", json!(input.database_id.is_some())),
            ("hasDatabaseName", json!(input.database_name.is_some())),
        ]);
        self.tracked_tool(
            "explain_query",
            event_properties,
            self.manager.explain_query(input),
        )
        .await
    }

    #[tool(description = "Create a named schema snapshot for a PGSandbox-owned database.")]
    async fn create_schema_snapshot(
        &self,
        Parameters(input): Parameters<CreateSchemaSnapshotInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let event_properties = properties([
            ("hasProfile", json!(input.profile.is_some())),
            ("hasDatabaseId", json!(input.database_id.is_some())),
            ("hasDatabaseName", json!(input.database_name.is_some())),
            ("hasNotes", json!(input.notes.is_some())),
        ]);
        self.tracked_tool(
            "create_schema_snapshot",
            event_properties,
            self.manager.create_schema_snapshot(input),
        )
        .await
    }

    #[tool(description = "List schema snapshots for a PGSandbox-owned database.")]
    async fn list_schema_snapshots(
        &self,
        Parameters(input): Parameters<ListSchemaSnapshotsInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let event_properties = properties([
            ("hasProfile", json!(input.profile.is_some())),
            ("hasDatabaseId", json!(input.database_id.is_some())),
            ("hasDatabaseName", json!(input.database_name.is_some())),
        ]);
        self.tracked_tool(
            "list_schema_snapshots",
            event_properties,
            self.manager.list_schema_snapshots(input),
        )
        .await
    }

    #[tool(description = "Delete a named schema snapshot for a PGSandbox-owned database.")]
    async fn delete_schema_snapshot(
        &self,
        Parameters(input): Parameters<DeleteSchemaSnapshotInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let event_properties = properties([
            ("hasProfile", json!(input.profile.is_some())),
            ("hasDatabaseId", json!(input.database_id.is_some())),
            ("hasDatabaseName", json!(input.database_name.is_some())),
        ]);
        self.tracked_tool(
            "delete_schema_snapshot",
            event_properties,
            self.manager.delete_schema_snapshot(input),
        )
        .await
    }

    #[tool(description = "Diff a named schema snapshot against the current sandbox schema.")]
    async fn diff_schema_snapshot(
        &self,
        Parameters(input): Parameters<DiffSchemaSnapshotInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let event_properties = properties([
            ("hasProfile", json!(input.profile.is_some())),
            ("hasDatabaseId", json!(input.database_id.is_some())),
            ("hasDatabaseName", json!(input.database_name.is_some())),
        ]);
        self.tracked_tool(
            "diff_schema_snapshot",
            event_properties,
            self.manager.diff_schema_snapshot(input),
        )
        .await
    }

    #[tool(description = "Detect a Django repo and write a secret-free PG Sandbox project config.")]
    async fn prepare_for_repo(
        &self,
        Parameters(input): Parameters<PrepareForRepoInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let event_properties = properties([
            ("hasProfile", json!(input.profile.is_some())),
            ("hasDatabaseId", json!(input.database_id.is_some())),
            ("hasDatabaseName", json!(input.database_name.is_some())),
        ]);
        self.tracked_tool(
            "prepare_for_repo",
            event_properties,
            self.manager.prepare_for_repo(input),
        )
        .await
    }

    #[tool(description = "Run an explicit Django migration command against a sandbox database.")]
    async fn run_migrations(
        &self,
        Parameters(input): Parameters<RunMigrationsInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let event_properties = properties([
            ("hasProfile", json!(input.profile.is_some())),
            ("hasDatabaseId", json!(input.database_id.is_some())),
            ("hasDatabaseName", json!(input.database_name.is_some())),
            ("hasCommand", json!(input.command.is_some())),
            ("hasTimeout", json!(input.timeout_seconds.is_some())),
        ]);
        self.tracked_tool(
            "run_migrations",
            event_properties,
            self.manager.run_migrations(input),
        )
        .await
    }

    #[tool(description = "Run Django migrations in a sandbox and return before/after schema diff.")]
    async fn validate_migration(
        &self,
        Parameters(input): Parameters<ValidateMigrationInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let event_properties = properties([
            ("hasProfile", json!(input.profile.is_some())),
            ("hasDatabaseId", json!(input.database_id.is_some())),
            ("hasDatabaseName", json!(input.database_name.is_some())),
            ("hasCommand", json!(input.command.is_some())),
            ("hasTimeout", json!(input.timeout_seconds.is_some())),
            ("hasTtlMinutes", json!(input.ttl_minutes.is_some())),
            ("hasOwner", json!(input.owner.is_some())),
            (
                "labelCount",
                json!(input.labels.as_ref().map_or(0, |labels| labels.len())),
            ),
        ]);
        self.tracked_tool(
            "validate_migration",
            event_properties,
            self.manager.validate_migration(input),
        )
        .await
    }

    #[tool(description = "Run an explicit seed command against a sandbox database.")]
    async fn seed_database(
        &self,
        Parameters(input): Parameters<SeedDatabaseInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let event_properties = properties([
            ("hasProfile", json!(input.profile.is_some())),
            ("hasDatabaseId", json!(input.database_id.is_some())),
            ("hasDatabaseName", json!(input.database_name.is_some())),
            ("hasCommand", json!(input.command.is_some())),
            ("hasTimeout", json!(input.timeout_seconds.is_some())),
        ]);
        self.tracked_tool(
            "seed_database",
            event_properties,
            self.manager.seed_database(input),
        )
        .await
    }

    #[tool(description = "Export a PGSandbox-owned database to a local template artifact.")]
    async fn create_template_from_sandbox(
        &self,
        Parameters(input): Parameters<CreateTemplateFromSandboxInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let event_properties = properties([
            ("hasProfile", json!(input.profile.is_some())),
            ("hasDatabaseId", json!(input.database_id.is_some())),
            ("hasDatabaseName", json!(input.database_name.is_some())),
            ("hasCreatedBy", json!(input.created_by.is_some())),
            ("hasNotes", json!(input.notes.is_some())),
        ]);
        self.tracked_tool(
            "create_template_from_sandbox",
            event_properties,
            self.manager.create_template_from_sandbox(input),
        )
        .await
    }

    #[tool(description = "Create a new sandbox database from a local template artifact.")]
    async fn create_sandbox_from_template(
        &self,
        Parameters(input): Parameters<CreateSandboxFromTemplateInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let event_properties = properties([
            ("hasProfile", json!(input.profile.is_some())),
            ("hasNameHint", json!(input.name_hint.is_some())),
            ("hasOwner", json!(input.owner.is_some())),
            ("hasTtlMinutes", json!(input.ttl_minutes.is_some())),
            (
                "labelCount",
                json!(input.labels.as_ref().map_or(0, |labels| labels.len())),
            ),
        ]);
        self.tracked_tool(
            "create_sandbox_from_template",
            event_properties,
            self.manager.create_sandbox_from_template(input),
        )
        .await
    }

    #[tool(description = "List local template artifacts for a profile.")]
    async fn list_templates(
        &self,
        Parameters(input): Parameters<ListTemplatesInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let event_properties = properties([
            ("hasProfile", json!(input.profile.is_some())),
            (
                "hasPostgresVersion",
                json!(input.postgres_version.is_some()),
            ),
        ]);
        self.tracked_tool(
            "list_templates",
            event_properties,
            self.manager.list_templates(input),
        )
        .await
    }

    #[tool(description = "Delete a local template artifact.")]
    async fn delete_template(
        &self,
        Parameters(input): Parameters<DeleteTemplateInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let event_properties = properties([
            ("hasProfile", json!(input.profile.is_some())),
            (
                "hasPostgresVersion",
                json!(input.postgres_version.is_some()),
            ),
        ]);
        self.tracked_tool(
            "delete_template",
            event_properties,
            self.manager.delete_template(input),
        )
        .await
    }

    #[tool(description = "List sandbox databases known to PGSandbox.")]
    async fn list_databases(
        &self,
        Parameters(input): Parameters<ListDatabasesInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let event_properties = properties([
            ("hasProfile", json!(input.profile.is_some())),
            (
                "hasPostgresVersion",
                json!(input.postgres_version.is_some()),
            ),
            ("hasOwnerFilter", json!(input.owner.is_some())),
        ]);
        self.tracked_tool(
            "list_databases",
            event_properties,
            self.manager.list_databases(input),
        )
        .await
    }

    #[tool(description = "Delete expired sandbox databases.")]
    async fn cleanup_expired(
        &self,
        Parameters(input): Parameters<CleanupExpiredInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let event_properties = properties([
            ("hasProfile", json!(input.profile.is_some())),
            (
                "hasPostgresVersion",
                json!(input.postgres_version.is_some()),
            ),
            ("dryRun", json!(input.dry_run.unwrap_or(false))),
        ]);
        self.tracked_tool(
            "cleanup_expired",
            event_properties,
            self.manager.cleanup_expired(input),
        )
        .await
    }
}

pub async fn serve_stdio(config: SandboxConfig) -> anyhow::Result<()> {
    let profile_count = config.profiles.len();
    let server = PgsandboxServer::new(config);
    let telemetry = server.telemetry.clone();
    let service = server.serve(rmcp::transport::stdio()).await?;
    telemetry.capture_background(
        EVENT_MCP_SERVER_STARTED,
        properties([("profileCount", json!(profile_count))]),
    );
    service.waiting().await?;
    Ok(())
}

fn selector_properties(input: &DatabaseSelector) -> Map<String, Value> {
    properties([
        ("hasProfile", json!(input.profile.is_some())),
        (
            "hasPostgresVersion",
            json!(input.postgres_version.is_some()),
        ),
        ("hasDatabaseId", json!(input.database_id.is_some())),
        ("hasDatabaseName", json!(input.database_name.is_some())),
    ])
}

fn tool_json<T: Serialize>(result: anyhow::Result<T>) -> Result<CallToolResult, ErrorData> {
    match result {
        Ok(value) => {
            let text = serde_json::to_string_pretty(&value).map_err(internal_error)?;
            Ok(CallToolResult::success(vec![Content::text(text)]))
        }
        Err(error) => {
            let text = serde_json::to_string_pretty(&ToolErrorResponse::from_error(&error))
                .map_err(internal_error)?;
            Ok(CallToolResult::error(vec![Content::text(text)]))
        }
    }
}

fn internal_error(error: impl std::fmt::Display) -> ErrorData {
    ErrorData::internal_error(error.to_string(), None)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolErrorResponse {
    ok: bool,
    error: ToolErrorBody,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolErrorBody {
    code: &'static str,
    category: &'static str,
    message: String,
    hint: String,
}

impl ToolErrorResponse {
    fn from_error(error: &anyhow::Error) -> Self {
        let chain = sanitize_error_message(&format!("{error:#}"));
        let lower = chain.to_ascii_lowercase();
        let body = if lower.contains("password authentication failed")
            || lower.contains("authentication failed")
        {
            ToolErrorBody {
                code: "postgres_auth_failed",
                category: "postgres",
                message: chain,
                hint: "Run `pgsandbox-mcp doctor` to identify the active config source. If an MCP client config has a stale explicit PGSANDBOX_ADMIN_DATABASE_URL, run `pgsandbox-mcp setup --client <client>` without --admin-url, restart the MCP client, and retry.".to_string(),
            }
        } else if lower.contains("could not find local postgres")
            || lower.contains("failed to prepare local postgres profile")
            || lower.contains("failed to prepare default local postgres profile")
        {
            ToolErrorBody {
                code: "local_postgres_unavailable",
                category: "local_postgres",
                message: chain,
                hint: "Install local PostgreSQL server binaries for the requested major version, set PGSANDBOX_POSTGRES_BIN_DIR or PGSANDBOX_POSTGRES_<MAJOR>_BIN_DIR, or choose a version shown by list_profiles.".to_string(),
            }
        } else if lower.contains("no configured profile advertises postgresversion")
            || lower.contains("unknown postgres version")
        {
            ToolErrorBody {
                code: "postgres_version_unavailable",
                category: "config",
                message: chain,
                hint: "Use a postgresVersion listed by list_profiles, add a matching explicit profile, or rerun setup without --admin-url to use managed local version discovery.".to_string(),
            }
        } else if lower.contains("connection refused")
            || lower.contains("connection timed out")
            || lower.contains("failed to connect")
        {
            ToolErrorBody {
                code: "postgres_connection_failed",
                category: "postgres",
                message: chain,
                hint: "Run `pgsandbox-mcp doctor` to verify the configured profile and connectivity. For managed local, try `pgsandbox-mcp local status` or `pgsandbox-mcp local start`.".to_string(),
            }
        } else {
            ToolErrorBody {
                code: "pgsandbox_tool_failed",
                category: "unknown",
                message: chain,
                hint: "Run `pgsandbox-mcp doctor` for a local diagnostic, then retry the tool with the same profile or postgresVersion.".to_string(),
            }
        };

        Self {
            ok: false,
            error: body,
        }
    }
}

fn sanitize_error_message(message: &str) -> String {
    let mut sanitized = String::with_capacity(message.len());
    let mut cursor = 0;

    while let Some(relative_start) = find_postgres_url_start(&message[cursor..]) {
        let start = cursor + relative_start;
        sanitized.push_str(&message[cursor..start]);

        let tail = &message[start..];
        let end = tail.find(char::is_whitespace).unwrap_or(tail.len());
        sanitized.push_str(&sanitize_error_token(&tail[..end]));
        cursor = start + end;
    }

    sanitized.push_str(&message[cursor..]);
    sanitized
}

fn find_postgres_url_start(message: &str) -> Option<usize> {
    let lower = message.to_ascii_lowercase();
    [lower.find("postgres://"), lower.find("postgresql://")]
        .into_iter()
        .flatten()
        .min()
}

fn sanitize_error_token(token: &str) -> String {
    let Some((scheme, rest)) = token.split_once("://") else {
        return token.to_string();
    };
    if !matches!(scheme, "postgres" | "postgresql") {
        return token.to_string();
    }
    let Some((credentials, suffix)) = rest.split_once('@') else {
        return token.to_string();
    };
    let Some((user, _password)) = credentials.split_once(':') else {
        return token.to_string();
    };
    format!("{scheme}://{user}:****@{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_errors_are_structured_and_actionable() {
        let result = tool_json::<()>(Err(anyhow::anyhow!(
            "failed to connect to Postgres admin profile default at postgresql://postgres:****@localhost:5432/postgres?sslmode=disable: db error: FATAL: password authentication failed for user \"postgres\""
        )))
        .unwrap();

        assert!(result.is_error.unwrap_or(false));
        let text = result.content[0].as_text().unwrap().text.clone();
        let value = serde_json::from_str::<Value>(&text).unwrap();

        assert_eq!(value["ok"], false);
        assert_eq!(value["error"]["code"], "postgres_auth_failed");
        assert_eq!(value["error"]["category"], "postgres");
        assert!(value["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("pgsandbox-mcp doctor"));
        assert!(!text.contains("postgres:postgres@"));
    }

    #[test]
    fn sanitizes_postgres_urls_inside_punctuation() {
        let sanitized = sanitize_error_message(
            "failed (postgresql://postgres:secret@localhost:5432/postgres), retry 'postgres://pg:another-secret@127.0.0.1/db'",
        );

        assert!(sanitized.contains("(postgresql://postgres:****@localhost:5432/postgres),"));
        assert!(sanitized.contains("'postgres://pg:****@127.0.0.1/db'"));
        assert!(!sanitized.contains("secret@"));
        assert!(!sanitized.contains("another-secret@"));
    }

    #[test]
    fn public_tool_metadata_matches_tool_registrations() {
        let source = include_str!("mcp.rs");
        let tool_attribute = concat!("#[", "tool(");

        assert_eq!(
            PUBLIC_MCP_TOOL_COUNT,
            source.matches(tool_attribute).count(),
            "PUBLIC_MCP_TOOLS must be updated when adding or removing #[tool] registrations"
        );
        for tool_name in PUBLIC_MCP_TOOLS {
            assert!(
                source.contains(&format!("async fn {tool_name}(")),
                "PUBLIC_MCP_TOOLS contains {tool_name}, but no matching tool method exists"
            );
        }
    }
}
