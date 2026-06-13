use std::collections::BTreeMap;

use anyhow::Context;
use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, Utc};
use regex::Regex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio_postgres::{
    types::{ToSql, Type},
    Client, NoTls, Row, SimpleQueryMessage,
};
use url::Url;
use uuid::Uuid;

use crate::{
    config::{find_profile, SandboxConfig, SandboxProfile},
    names::{make_sandbox_names, quote_ident, quote_literal},
};

const METADATA_TABLE: &str = "pgsandbox_databases";
const DEFAULT_ROW_LIMIT: usize = 100;

#[derive(Clone)]
pub struct PostgresSandboxManager {
    config: SandboxConfig,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateDatabaseInput {
    pub profile: Option<String>,
    pub name_hint: Option<String>,
    pub ttl_minutes: Option<u32>,
    pub owner: Option<String>,
    pub labels: Option<BTreeMap<String, Value>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseSelector {
    pub profile: Option<String>,
    pub database_id: Option<String>,
    pub database_name: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunSqlInput {
    pub profile: Option<String>,
    pub database_id: Option<String>,
    pub database_name: Option<String>,
    pub sql: String,
    pub readonly: Option<bool>,
    pub row_limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListDatabasesInput {
    pub profile: Option<String>,
    pub owner: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CleanupExpiredInput {
    pub profile: Option<String>,
    pub dry_run: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDatabaseOutput {
    pub database_id: String,
    pub profile: String,
    pub database_name: String,
    pub role_name: String,
    pub expires_at: DateTime<Utc>,
    pub connection_string: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteDatabaseOutput {
    pub database_id: String,
    pub database_name: String,
    pub deleted: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStringOutput {
    pub database_id: String,
    pub database_name: String,
    pub expires_at: DateTime<Utc>,
    pub connection_string: String,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RunSqlOutput {
    pub database_id: String,
    pub database_name: String,
    pub row_count: Option<u64>,
    pub rows: Vec<Value>,
    pub truncated: bool,
    pub elapsed_ms: u128,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeSchemaOutput {
    pub database_id: String,
    pub database_name: String,
    pub tables: Vec<Value>,
    pub columns: Vec<Value>,
    pub indexes: Vec<Value>,
    pub extensions: Vec<Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupExpiredOutput {
    pub dry_run: bool,
    pub selected: Option<Vec<Value>>,
    pub deleted: Option<Vec<String>>,
    pub failures: Option<Vec<Value>>,
}

#[derive(Debug)]
struct SandboxRecord {
    database_id: String,
    profile_name: String,
    database_name: String,
    role_name: String,
    role_password: String,
    owner: Option<String>,
    purpose: Option<String>,
    labels: Value,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
}

struct QueryExecutionResult {
    row_count: Option<u64>,
    rows: Vec<Value>,
    truncated: bool,
}

impl PostgresSandboxManager {
    pub fn new(config: SandboxConfig) -> Self {
        Self { config }
    }

    pub async fn create_database(
        &self,
        input: CreateDatabaseInput,
    ) -> anyhow::Result<CreateDatabaseOutput> {
        let profile = find_profile(&self.config, input.profile.as_deref())?.clone();
        let ttl_minutes = clamp_ttl(input.ttl_minutes, &profile)?;
        let names = make_sandbox_names(&profile.database_prefix, input.name_hint.as_deref());
        let role_password = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let expires_at = Utc::now() + Duration::minutes(ttl_minutes.into());

        let (client, connection_task) = connect_admin(&profile).await?;
        ensure_metadata_table(&client).await?;

        let mut created_role = false;
        let mut created_database = false;
        let result = async {
            client
                .batch_execute(&format!(
                    "CREATE ROLE {} LOGIN PASSWORD {}",
                    quote_ident(&names.role_name)?,
                    quote_literal(&role_password)
                ))
                .await?;
            created_role = true;

            client
                .batch_execute(&format!(
                    "CREATE DATABASE {} OWNER {}",
                    quote_ident(&names.database_name)?,
                    quote_ident(&names.role_name)?
                ))
                .await?;
            created_database = true;

            let labels = serde_json::to_value(input.labels.unwrap_or_default())?;
            client
                .execute(
                    &format!(
                        r#"
                          INSERT INTO {}
                            (database_id, profile_name, database_name, role_name, role_password, owner, purpose, labels, expires_at)
                          VALUES ($1, $2, $3, $4, $5, $6, $7, $8::jsonb, $9)
                        "#,
                        quote_ident(METADATA_TABLE)?
                    ),
                    &[
                        &names.database_id as &(dyn ToSql + Sync),
                        &profile.name,
                        &names.database_name,
                        &names.role_name,
                        &role_password,
                        &input.owner,
                        &input.name_hint,
                        &labels,
                        &expires_at,
                    ],
                )
                .await?;
            anyhow::Ok(())
        }
        .await;

        if let Err(error) = result {
            if created_database {
                let _ = terminate_database_connections(&client, &names.database_name).await;
                let _ = client
                    .batch_execute(&format!(
                        "DROP DATABASE IF EXISTS {}",
                        quote_ident(&names.database_name)?
                    ))
                    .await;
            }
            if created_role {
                let _ = client
                    .batch_execute(&format!(
                        "DROP ROLE IF EXISTS {}",
                        quote_ident(&names.role_name)?
                    ))
                    .await;
            }
            drop(client);
            let _ = connection_task.await;
            return Err(error);
        }

        drop(client);
        let _ = connection_task.await;

        Ok(CreateDatabaseOutput {
            database_id: names.database_id,
            profile: profile.name.clone(),
            database_name: names.database_name.clone(),
            role_name: names.role_name.clone(),
            expires_at,
            connection_string: build_connection_string(
                &profile.admin_url,
                &names.database_name,
                &names.role_name,
                &role_password,
            )?,
        })
    }

    pub async fn delete_database(
        &self,
        input: DatabaseSelector,
    ) -> anyhow::Result<DeleteDatabaseOutput> {
        let profile = find_profile(&self.config, input.profile.as_deref())?.clone();
        let (client, connection_task) = connect_admin(&profile).await?;
        ensure_metadata_table(&client).await?;
        let record = find_record(&client, &profile.name, &input)
            .await?
            .context("Database was not found in PGSandbox metadata.")?;

        terminate_database_connections(&client, &record.database_name).await?;
        client
            .batch_execute(&format!(
                "DROP DATABASE IF EXISTS {}",
                quote_ident(&record.database_name)?
            ))
            .await?;
        client
            .batch_execute(&format!(
                "DROP ROLE IF EXISTS {}",
                quote_ident(&record.role_name)?
            ))
            .await?;
        client
            .execute(
                &format!(
                    "UPDATE {} SET deleted_at = now() WHERE database_id = $1",
                    quote_ident(METADATA_TABLE)?
                ),
                &[&record.database_id],
            )
            .await?;

        drop(client);
        let _ = connection_task.await;

        Ok(DeleteDatabaseOutput {
            database_id: record.database_id,
            database_name: record.database_name,
            deleted: true,
        })
    }

    pub async fn get_connection_string(
        &self,
        input: DatabaseSelector,
    ) -> anyhow::Result<ConnectionStringOutput> {
        let profile = find_profile(&self.config, input.profile.as_deref())?.clone();
        let (client, connection_task) = connect_admin(&profile).await?;
        ensure_metadata_table(&client).await?;
        let record = find_record(&client, &profile.name, &input)
            .await?
            .context("Database was not found in PGSandbox metadata.")?;
        drop(client);
        let _ = connection_task.await;

        Ok(ConnectionStringOutput {
            database_id: record.database_id,
            database_name: record.database_name.clone(),
            expires_at: record.expires_at,
            connection_string: build_connection_string(
                &profile.admin_url,
                &record.database_name,
                &record.role_name,
                &record.role_password,
            )?,
        })
    }

    pub async fn list_databases(&self, input: ListDatabasesInput) -> anyhow::Result<Vec<Value>> {
        let profile = find_profile(&self.config, input.profile.as_deref())?.clone();
        let (client, connection_task) = connect_admin(&profile).await?;
        ensure_metadata_table(&client).await?;
        let rows = client
            .query(
                &format!(
                    r#"
                      SELECT database_id, profile_name, database_name, role_name, owner, purpose, labels,
                             created_at, expires_at, deleted_at
                      FROM {}
                      WHERE profile_name = $1
                        AND deleted_at IS NULL
                        AND ($2::text IS NULL OR owner = $2)
                      ORDER BY created_at DESC
                      LIMIT 100
                    "#,
                    quote_ident(METADATA_TABLE)?
                ),
                &[&profile.name, &input.owner],
            )
            .await?;
        drop(client);
        let _ = connection_task.await;

        Ok(rows.iter().map(record_summary_to_json).collect())
    }

    pub async fn run_sql(&self, input: RunSqlInput) -> anyhow::Result<RunSqlOutput> {
        let connection = self
            .get_connection_string(DatabaseSelector {
                profile: input.profile.clone(),
                database_id: input.database_id.clone(),
                database_name: input.database_name.clone(),
            })
            .await?;
        let started = std::time::Instant::now();
        let row_limit = input.row_limit.unwrap_or(DEFAULT_ROW_LIMIT).min(1000);
        let (client, connection_task) = connect_url(&connection.connection_string).await?;

        let result = async {
            if input.readonly.unwrap_or(false) {
                assert_safe_readonly_sql(&input.sql)?;
                client
                    .batch_execute(
                        "SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY; BEGIN READ ONLY",
                    )
                    .await?;
            }

            let result = if input.readonly.unwrap_or(false) {
                run_cursor_query(&client, &input.sql, row_limit, false).await
            } else if looks_row_producing(&input.sql) {
                run_cursor_query(&client, &input.sql, row_limit, true).await
            } else {
                run_direct_query(&client, &input.sql, row_limit).await
            };

            if input.readonly.unwrap_or(false) {
                let _ = client.batch_execute("ROLLBACK").await;
            }

            result
        }
        .await;

        if result.is_err() && input.readonly.unwrap_or(false) {
            let _ = client.batch_execute("ROLLBACK").await;
        }

        drop(client);
        let _ = connection_task.await;

        let result = result?;
        Ok(RunSqlOutput {
            database_id: connection.database_id,
            database_name: connection.database_name,
            row_count: result.row_count,
            rows: result.rows,
            truncated: result.truncated,
            elapsed_ms: started.elapsed().as_millis(),
        })
    }

    pub async fn describe_schema(
        &self,
        input: DatabaseSelector,
    ) -> anyhow::Result<DescribeSchemaOutput> {
        let connection = self.get_connection_string(input).await?;
        let (client, connection_task) = connect_url(&connection.connection_string).await?;

        let tables = client
            .query(
                r#"
                  SELECT table_schema, table_name
                  FROM information_schema.tables
                  WHERE table_schema NOT IN ('pg_catalog', 'information_schema')
                  ORDER BY table_schema, table_name
                "#,
                &[],
            )
            .await?;
        let columns = client
            .query(
                r#"
                  SELECT table_schema, table_name, column_name, data_type, is_nullable
                  FROM information_schema.columns
                  WHERE table_schema NOT IN ('pg_catalog', 'information_schema')
                  ORDER BY table_schema, table_name, ordinal_position
                "#,
                &[],
            )
            .await?;
        let indexes = client
            .query(
                r#"
                  SELECT schemaname, tablename, indexname, indexdef
                  FROM pg_indexes
                  WHERE schemaname NOT IN ('pg_catalog', 'information_schema')
                  ORDER BY schemaname, tablename, indexname
                "#,
                &[],
            )
            .await?;
        let extensions = client
            .query(
                "SELECT extname, extversion FROM pg_extension ORDER BY extname",
                &[],
            )
            .await?;

        drop(client);
        let _ = connection_task.await;

        Ok(DescribeSchemaOutput {
            database_id: connection.database_id,
            database_name: connection.database_name,
            tables: rows_to_json(tables),
            columns: rows_to_json(columns),
            indexes: rows_to_json(indexes),
            extensions: rows_to_json(extensions),
        })
    }

    pub async fn cleanup_expired(
        &self,
        input: CleanupExpiredInput,
    ) -> anyhow::Result<CleanupExpiredOutput> {
        let profile = find_profile(&self.config, input.profile.as_deref())?.clone();
        let (client, connection_task) = connect_admin(&profile).await?;
        ensure_metadata_table(&client).await?;
        let expired = client
            .query(
                &format!(
                    r#"
                      SELECT *
                      FROM {}
                      WHERE profile_name = $1
                        AND deleted_at IS NULL
                        AND expires_at <= now()
                      ORDER BY expires_at ASC
                      LIMIT 50
                    "#,
                    quote_ident(METADATA_TABLE)?
                ),
                &[&profile.name],
            )
            .await?;
        let records = expired
            .iter()
            .map(sandbox_record_from_row)
            .collect::<Vec<_>>();

        if input.dry_run.unwrap_or(false) {
            drop(client);
            let _ = connection_task.await;
            return Ok(CleanupExpiredOutput {
                dry_run: true,
                selected: Some(records.iter().map(record_to_json).collect()),
                deleted: None,
                failures: None,
            });
        }

        let mut deleted = Vec::new();
        let mut failures = Vec::new();
        for record in records {
            let deletion = async {
                terminate_database_connections(&client, &record.database_name).await?;
                client
                    .batch_execute(&format!(
                        "DROP DATABASE IF EXISTS {}",
                        quote_ident(&record.database_name)?
                    ))
                    .await?;
                client
                    .batch_execute(&format!(
                        "DROP ROLE IF EXISTS {}",
                        quote_ident(&record.role_name)?
                    ))
                    .await?;
                client
                    .execute(
                        &format!(
                            "UPDATE {} SET deleted_at = now() WHERE database_id = $1",
                            quote_ident(METADATA_TABLE)?
                        ),
                        &[&record.database_id],
                    )
                    .await?;
                anyhow::Ok(())
            }
            .await;

            match deletion {
                Ok(()) => deleted.push(record.database_id),
                Err(error) => failures.push(json!({
                    "databaseId": record.database_id,
                    "message": error.to_string()
                })),
            }
        }

        drop(client);
        let _ = connection_task.await;

        Ok(CleanupExpiredOutput {
            dry_run: false,
            selected: None,
            deleted: Some(deleted),
            failures: Some(failures),
        })
    }
}

async fn connect_admin(
    profile: &SandboxProfile,
) -> anyhow::Result<(
    Client,
    tokio::task::JoinHandle<Result<(), tokio_postgres::Error>>,
)> {
    connect_url(&profile.admin_url).await
}

async fn connect_url(
    url: &str,
) -> anyhow::Result<(
    Client,
    tokio::task::JoinHandle<Result<(), tokio_postgres::Error>>,
)> {
    let (client, connection) = tokio_postgres::connect(url, NoTls).await?;
    let task = tokio::spawn(async move { connection.await });
    Ok((client, task))
}

async fn ensure_metadata_table(client: &Client) -> anyhow::Result<()> {
    client
        .batch_execute(&format!(
            r#"
              CREATE TABLE IF NOT EXISTS {} (
                database_id text PRIMARY KEY,
                profile_name text NOT NULL,
                database_name text NOT NULL UNIQUE,
                role_name text NOT NULL UNIQUE,
                role_password text NOT NULL,
                owner text,
                purpose text,
                labels jsonb NOT NULL DEFAULT '{{}}'::jsonb,
                created_at timestamptz NOT NULL DEFAULT now(),
                expires_at timestamptz NOT NULL,
                deleted_at timestamptz
              )
            "#,
            quote_ident(METADATA_TABLE)?
        ))
        .await?;
    Ok(())
}

async fn find_record(
    client: &Client,
    profile_name: &str,
    input: &DatabaseSelector,
) -> anyhow::Result<Option<SandboxRecord>> {
    if input.database_id.is_none() && input.database_name.is_none() {
        anyhow::bail!("Provide databaseId or databaseName.");
    }

    let rows = client
        .query(
            &format!(
                r#"
                  SELECT *
                  FROM {}
                  WHERE deleted_at IS NULL
                    AND profile_name = $3
                    AND (($1::text IS NOT NULL AND database_id = $1)
                      OR ($2::text IS NOT NULL AND database_name = $2))
                  LIMIT 1
                "#,
                quote_ident(METADATA_TABLE)?
            ),
            &[&input.database_id, &input.database_name, &profile_name],
        )
        .await?;

    Ok(rows.first().map(sandbox_record_from_row))
}

async fn terminate_database_connections(
    client: &Client,
    database_name: &str,
) -> anyhow::Result<()> {
    client
        .execute(
            r#"
              SELECT pg_terminate_backend(pid)
              FROM pg_stat_activity
              WHERE datname = $1
                AND pid <> pg_backend_pid()
            "#,
            &[&database_name],
        )
        .await?;
    Ok(())
}

fn build_connection_string(
    admin_url: &str,
    database_name: &str,
    role_name: &str,
    role_password: &str,
) -> anyhow::Result<String> {
    let mut url = Url::parse(admin_url)?;
    url.set_username(role_name)
        .map_err(|_| anyhow::anyhow!("failed to set database username"))?;
    url.set_password(Some(role_password))
        .map_err(|_| anyhow::anyhow!("failed to set database password"))?;
    url.set_path(database_name);
    Ok(url.to_string())
}

fn clamp_ttl(ttl_minutes: Option<u32>, profile: &SandboxProfile) -> anyhow::Result<u32> {
    let ttl_minutes = ttl_minutes.unwrap_or(profile.default_ttl_minutes);
    if ttl_minutes > profile.max_ttl_minutes {
        anyhow::bail!(
            "ttlMinutes exceeds maxTtlMinutes ({}) for profile {}",
            profile.max_ttl_minutes,
            profile.name
        );
    }
    Ok(ttl_minutes)
}

pub fn assert_safe_readonly_sql(sql: &str) -> anyhow::Result<()> {
    let guard = Regex::new(r"(?i)\b(begin|commit|rollback|abort|end|savepoint|release|set\s+(session|transaction)|reset)\b")
        .expect("readonly SQL guard regex compiles");
    if guard.is_match(sql) {
        anyhow::bail!(
            "readonly SQL cannot include transaction-control or session-setting statements."
        );
    }
    Ok(())
}

async fn run_cursor_query(
    client: &Client,
    sql: &str,
    row_limit: usize,
    owns_transaction: bool,
) -> anyhow::Result<QueryExecutionResult> {
    let cursor_name = format!("pgsandbox_cursor_{}", Uuid::new_v4().simple());
    let quoted_cursor = quote_ident(&cursor_name)?;
    if owns_transaction {
        client.batch_execute("BEGIN").await?;
    }
    let declare_result = client
        .batch_execute(&format!(
            "DECLARE {quoted_cursor} NO SCROLL CURSOR FOR {sql}"
        ))
        .await;
    if let Err(error) = declare_result {
        if owns_transaction {
            let _ = client.batch_execute("ROLLBACK").await;
        }
        return Err(error.into());
    }

    let rows_result = client
        .query(
            &format!("FETCH FORWARD {} FROM {quoted_cursor}", row_limit + 1),
            &[],
        )
        .await;
    let rows = match rows_result {
        Ok(rows) => rows,
        Err(error) => {
            let _ = client
                .batch_execute(&format!("CLOSE {quoted_cursor}"))
                .await;
            if owns_transaction {
                let _ = client.batch_execute("ROLLBACK").await;
            }
            return Err(error.into());
        }
    };
    let _ = client
        .batch_execute(&format!("CLOSE {quoted_cursor}"))
        .await;
    if owns_transaction {
        let _ = client.batch_execute("COMMIT").await;
    }

    let truncated = rows.len() > row_limit;
    let visible_rows = rows.into_iter().take(row_limit).collect::<Vec<_>>();
    Ok(QueryExecutionResult {
        row_count: if truncated {
            None
        } else {
            Some(visible_rows.len() as u64)
        },
        rows: rows_to_json(visible_rows),
        truncated,
    })
}

async fn run_direct_query(
    client: &Client,
    sql: &str,
    row_limit: usize,
) -> anyhow::Result<QueryExecutionResult> {
    let messages = client.simple_query(sql).await?;
    Ok(format_simple_query_result(messages, row_limit))
}

fn format_simple_query_result(
    messages: Vec<SimpleQueryMessage>,
    row_limit: usize,
) -> QueryExecutionResult {
    let mut current_rows = Vec::new();
    let mut current_had_rows = false;
    let mut final_rows = Vec::new();
    let mut final_row_count = None;

    for message in messages {
        match message {
            SimpleQueryMessage::Row(row) => {
                if !current_had_rows {
                    current_rows.clear();
                    current_had_rows = true;
                }
                let mut object = serde_json::Map::new();
                for (index, column) in row.columns().iter().enumerate() {
                    object.insert(
                        column.name().to_string(),
                        row.get(index)
                            .map_or(Value::Null, |value| Value::String(value.to_string())),
                    );
                }
                current_rows.push(Value::Object(object));
            }
            SimpleQueryMessage::CommandComplete(count) => {
                final_row_count = Some(count);
                final_rows = if current_had_rows {
                    std::mem::take(&mut current_rows)
                } else {
                    Vec::new()
                };
                current_had_rows = false;
            }
            _ => {}
        }
    }

    let truncated = final_rows.len() > row_limit;
    QueryExecutionResult {
        row_count: final_row_count,
        rows: final_rows.into_iter().take(row_limit).collect(),
        truncated,
    }
}

fn looks_row_producing(sql: &str) -> bool {
    let trimmed = sql.trim_start().to_ascii_lowercase();
    ["select", "with", "values", "show", "explain"]
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
}

fn sandbox_record_from_row(row: &Row) -> SandboxRecord {
    SandboxRecord {
        database_id: row.get("database_id"),
        profile_name: row.get("profile_name"),
        database_name: row.get("database_name"),
        role_name: row.get("role_name"),
        role_password: row.get("role_password"),
        owner: row.get("owner"),
        purpose: row.get("purpose"),
        labels: row.get("labels"),
        created_at: row.get("created_at"),
        expires_at: row.get("expires_at"),
        deleted_at: row.get("deleted_at"),
    }
}

fn record_summary_to_json(row: &Row) -> Value {
    json!({
        "database_id": row.get::<_, String>("database_id"),
        "profile_name": row.get::<_, String>("profile_name"),
        "database_name": row.get::<_, String>("database_name"),
        "role_name": row.get::<_, String>("role_name"),
        "owner": row.get::<_, Option<String>>("owner"),
        "purpose": row.get::<_, Option<String>>("purpose"),
        "labels": row.get::<_, Value>("labels"),
        "created_at": row.get::<_, DateTime<Utc>>("created_at"),
        "expires_at": row.get::<_, DateTime<Utc>>("expires_at"),
        "deleted_at": row.get::<_, Option<DateTime<Utc>>>("deleted_at"),
    })
}

fn record_to_json(record: &SandboxRecord) -> Value {
    json!({
        "database_id": record.database_id,
        "profile_name": record.profile_name,
        "database_name": record.database_name,
        "role_name": record.role_name,
        "owner": record.owner,
        "purpose": record.purpose,
        "labels": record.labels,
        "created_at": record.created_at,
        "expires_at": record.expires_at,
        "deleted_at": record.deleted_at,
    })
}

fn rows_to_json(rows: Vec<Row>) -> Vec<Value> {
    rows.iter().map(row_to_json).collect()
}

fn row_to_json(row: &Row) -> Value {
    let mut object = serde_json::Map::new();
    for (index, column) in row.columns().iter().enumerate() {
        object.insert(
            column.name().to_string(),
            cell_to_json(row, index, column.type_()),
        );
    }
    Value::Object(object)
}

fn cell_to_json(row: &Row, index: usize, value_type: &Type) -> Value {
    if matches!(
        value_type,
        &Type::TEXT | &Type::VARCHAR | &Type::BPCHAR | &Type::NAME
    ) {
        return row
            .try_get::<_, Option<String>>(index)
            .ok()
            .flatten()
            .map(Value::String)
            .unwrap_or(Value::Null);
    }

    match *value_type {
        Type::BOOL => row
            .try_get::<_, Option<bool>>(index)
            .ok()
            .flatten()
            .map(Value::Bool)
            .unwrap_or(Value::Null),
        Type::INT2 => row
            .try_get::<_, Option<i16>>(index)
            .ok()
            .flatten()
            .map(|value| json!(value))
            .unwrap_or(Value::Null),
        Type::INT4 => row
            .try_get::<_, Option<i32>>(index)
            .ok()
            .flatten()
            .map(|value| json!(value))
            .unwrap_or(Value::Null),
        Type::INT8 => row
            .try_get::<_, Option<i64>>(index)
            .ok()
            .flatten()
            .map(|value| json!(value))
            .unwrap_or(Value::Null),
        Type::FLOAT4 => row
            .try_get::<_, Option<f32>>(index)
            .ok()
            .flatten()
            .map(|value| json!(value))
            .unwrap_or(Value::Null),
        Type::FLOAT8 => row
            .try_get::<_, Option<f64>>(index)
            .ok()
            .flatten()
            .map(|value| json!(value))
            .unwrap_or(Value::Null),
        Type::JSON | Type::JSONB => row
            .try_get::<_, Option<Value>>(index)
            .ok()
            .flatten()
            .unwrap_or(Value::Null),
        Type::TIMESTAMPTZ => row
            .try_get::<_, Option<DateTime<Utc>>>(index)
            .ok()
            .flatten()
            .map(|value| Value::String(value.to_rfc3339()))
            .unwrap_or(Value::Null),
        Type::TIMESTAMP => row
            .try_get::<_, Option<NaiveDateTime>>(index)
            .ok()
            .flatten()
            .map(|value| Value::String(value.to_string()))
            .unwrap_or(Value::Null),
        Type::DATE => row
            .try_get::<_, Option<NaiveDate>>(index)
            .ok()
            .flatten()
            .map(|value| Value::String(value.to_string()))
            .unwrap_or(Value::Null),
        Type::UUID => row
            .try_get::<_, Option<Uuid>>(index)
            .ok()
            .flatten()
            .map(|value| Value::String(value.to_string()))
            .unwrap_or(Value::Null),
        _ => row
            .try_get::<_, Option<String>>(index)
            .ok()
            .flatten()
            .map(Value::String)
            .unwrap_or(Value::Null),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readonly_guard_rejects_transaction_control() {
        assert!(assert_safe_readonly_sql("select * from users").is_ok());
        assert!(assert_safe_readonly_sql("rollback; drop table users").is_err());
        assert!(
            assert_safe_readonly_sql("set session characteristics as transaction read write")
                .is_err()
        );
    }
}
