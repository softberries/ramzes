use std::collections::BTreeMap;

use ramzes_core::{Route, Template};
use serde::{Deserialize, Serialize};
use sqlx::{AssertSqlSafe, Row};

use crate::definitions::DefinitionRow;
use crate::sql::BeginWrite;
use crate::{Result, Store, StoreError, with_db};

/// Revisions kept per workspace.
const KEPT_REVISIONS: i64 = 200;

/// Settings that are part of the configuration.
const CONFIG_SETTINGS: [&str; 3] = ["retention_days", "retention_overrides", "public_url"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct RevisionMeta {
    pub id: String,
    pub author: String,
    /// What changed, for example "changed route billing".
    pub summary: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revision {
    pub meta: RevisionMeta,
    /// The whole configuration after the change, as JSON.
    pub snapshot: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectionSnapshot {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub settings: serde_json::Value,
    /// Sealed credentials. `None` keeps whatever is stored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credentials: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSnapshot {
    pub id: String,
    pub name: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mapping: Option<String>,
}

/// Everything a user configures, in one value.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ConfigSnapshot {
    pub routes: Vec<Route>,
    pub templates: Vec<Template>,
    pub definitions: Vec<DefinitionRow>,
    pub connections: Vec<ConnectionSnapshot>,
    pub sources: Vec<SourceSnapshot>,
    pub settings: BTreeMap<String, String>,
}

fn json<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|e| StoreError::Data(e.to_string()))
}

impl Store {
    /// Records the configuration after a change and drops revisions beyond the newest 200.
    pub async fn add_revision(
        &self,
        workspace_id: &str,
        author: &str,
        summary: &str,
        snapshot: &str,
        now_ms: i64,
    ) -> Result<()> {
        let insert = self.sql(
            "INSERT INTO config_revisions (id, workspace_id, author, summary, created_at, snapshot) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        );
        let trim = self.sql(
            "DELETE FROM config_revisions WHERE workspace_id = $1 AND id NOT IN \
             (SELECT id FROM config_revisions WHERE workspace_id = $1 ORDER BY id DESC LIMIT $2)",
        );
        let id = ramzes_core::new_id();
        with_db!(self, |pool| {
            sqlx::query(AssertSqlSafe(&*insert))
                .bind(&id)
                .bind(workspace_id)
                .bind(author)
                .bind(summary)
                .bind(now_ms)
                .bind(snapshot)
                .execute(pool)
                .await?;
            sqlx::query(AssertSqlSafe(&*trim))
                .bind(workspace_id)
                .bind(KEPT_REVISIONS)
                .execute(pool)
                .await
                .map(|_| ())
        })?;
        Ok(())
    }

    /// Revisions without their snapshots, newest first.
    pub async fn list_revisions(&self, workspace_id: &str) -> Result<Vec<RevisionMeta>> {
        let sql = self.sql(
            "SELECT id, author, summary, created_at FROM config_revisions WHERE workspace_id = $1 ORDER BY id DESC",
        );
        with_db!(self, |pool| {
            let rows = sqlx::query(AssertSqlSafe(&*sql))
                .bind(workspace_id)
                .fetch_all(pool)
                .await?;
            rows.iter()
                .map(|row| {
                    Ok(RevisionMeta {
                        id: row.try_get("id")?,
                        author: row.try_get("author")?,
                        summary: row.try_get("summary")?,
                        created_at: row.try_get("created_at")?,
                    })
                })
                .collect()
        })
    }

    pub async fn get_revision(&self, workspace_id: &str, id: &str) -> Result<Option<Revision>> {
        let sql = self.sql(
            "SELECT id, author, summary, created_at, snapshot FROM config_revisions WHERE workspace_id = $1 AND id = $2",
        );
        with_db!(self, |pool| {
            let row = sqlx::query(AssertSqlSafe(&*sql))
                .bind(workspace_id)
                .bind(id)
                .fetch_optional(pool)
                .await?;
            row.map(|row| {
                Ok(Revision {
                    meta: RevisionMeta {
                        id: row.try_get("id")?,
                        author: row.try_get("author")?,
                        summary: row.try_get("summary")?,
                        created_at: row.try_get("created_at")?,
                    },
                    snapshot: row.try_get("snapshot")?,
                })
            })
            .transpose()
        })
    }

    /// The current configuration, with sealed credentials.
    pub async fn read_config(&self, workspace_id: &str) -> Result<ConfigSnapshot> {
        let mut settings = BTreeMap::new();
        for key in CONFIG_SETTINGS {
            if let Some(value) = self.get_setting(workspace_id, key).await? {
                settings.insert(key.to_owned(), value);
            }
        }
        Ok(ConfigSnapshot {
            routes: self.list_routes(workspace_id).await?,
            templates: self.list_templates(workspace_id).await?,
            definitions: self.list_definitions(workspace_id).await?,
            connections: self
                .list_connections(workspace_id)
                .await?
                .into_iter()
                .map(|c| ConnectionSnapshot {
                    id: c.id,
                    kind: c.kind,
                    name: c.name,
                    settings: c.settings,
                    credentials: Some(c.credentials),
                })
                .collect(),
            sources: self
                .list_sources(workspace_id)
                .await?
                .into_iter()
                .map(|s| SourceSnapshot {
                    id: s.id,
                    name: s.name,
                    kind: s.kind,
                    mapping: s.mapping,
                })
                .collect(),
            settings,
        })
    }

    /// Makes the stored configuration equal to `config`, in one transaction.
    ///
    /// Routes, templates and connector definitions are replaced. Connections are replaced too,
    /// except that one without credentials in `config` keeps its stored credentials. Sources are
    /// only renamed and re-mapped: their keys are never part of a configuration, so a source
    /// that does not exist is not created and none is deleted.
    pub async fn apply_config(
        &self,
        workspace_id: &str,
        config: &ConfigSnapshot,
        now_ms: i64,
    ) -> Result<()> {
        let q = |text: &str| self.sql(text).into_owned();
        let delete_routes = q("DELETE FROM routes WHERE workspace_id = $1");
        let insert_route = q(
            "INSERT INTO routes (id, workspace_id, name, position, enabled, stop, match_json, targets_json, \
             shaping_json, escalation_json, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, '{}', '[]', $9, $9)",
        );
        let delete_templates = q("DELETE FROM templates WHERE workspace_id = $1");
        let insert_template = q(
            "INSERT INTO templates (id, workspace_id, connector_kind, name, title, body, created_at, updated_at) \
             VALUES ($1, $2, '*', $3, $4, $5, $6, $6)",
        );
        let delete_definitions = q("DELETE FROM connector_definitions WHERE workspace_id = $1");
        let insert_definition = q(
            "INSERT INTO connector_definitions (id, workspace_id, kind, name, definition, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        );
        let stored_credentials =
            q("SELECT id, credentials, created_at FROM connections WHERE workspace_id = $1");
        let delete_connections = q("DELETE FROM connections WHERE workspace_id = $1");
        let insert_connection = q(
            "INSERT INTO connections (id, workspace_id, kind, name, settings, credentials, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        );
        let update_source =
            q("UPDATE sources SET name = $1, mapping = $2 WHERE workspace_id = $3 AND id = $4");
        let set_setting = q(
            "INSERT INTO settings (workspace_id, key, value) VALUES ($1, $2, $3) \
             ON CONFLICT (workspace_id, key) DO UPDATE SET value = excluded.value",
        );
        let mut routes = Vec::with_capacity(config.routes.len());
        for route in &config.routes {
            routes.push((route, json(&route.r#match)?, json(&route.targets)?));
        }
        with_db!(self, |pool| {
            let mut tx = pool.begin_write().await?;
            sqlx::query(AssertSqlSafe(delete_routes.as_str()))
                .bind(workspace_id)
                .execute(&mut *tx)
                .await?;
            for (position, (route, match_json, targets_json)) in (1_i64..).zip(&routes) {
                sqlx::query(AssertSqlSafe(insert_route.as_str()))
                    .bind(&route.id)
                    .bind(workspace_id)
                    .bind(&route.name)
                    .bind(position)
                    .bind(i64::from(route.enabled))
                    .bind(i64::from(route.stop))
                    .bind(match_json)
                    .bind(targets_json)
                    .bind(now_ms)
                    .execute(&mut *tx)
                    .await?;
            }
            sqlx::query(AssertSqlSafe(delete_templates.as_str()))
                .bind(workspace_id)
                .execute(&mut *tx)
                .await?;
            for t in &config.templates {
                sqlx::query(AssertSqlSafe(insert_template.as_str()))
                    .bind(&t.id)
                    .bind(workspace_id)
                    .bind(&t.name)
                    .bind(&t.title)
                    .bind(&t.body)
                    .bind(now_ms)
                    .execute(&mut *tx)
                    .await?;
            }
            sqlx::query(AssertSqlSafe(delete_definitions.as_str()))
                .bind(workspace_id)
                .execute(&mut *tx)
                .await?;
            for d in &config.definitions {
                sqlx::query(AssertSqlSafe(insert_definition.as_str()))
                    .bind(&d.id)
                    .bind(workspace_id)
                    .bind(&d.kind)
                    .bind(&d.name)
                    .bind(&d.definition)
                    .bind(d.created_at)
                    .execute(&mut *tx)
                    .await?;
            }
            let stored: Vec<(String, String, i64)> =
                sqlx::query_as(AssertSqlSafe(stored_credentials.as_str()))
                    .bind(workspace_id)
                    .fetch_all(&mut *tx)
                    .await?;
            sqlx::query(AssertSqlSafe(delete_connections.as_str()))
                .bind(workspace_id)
                .execute(&mut *tx)
                .await?;
            for c in &config.connections {
                let existing = stored.iter().find(|(id, _, _)| *id == c.id);
                let credentials = match (&c.credentials, existing) {
                    (Some(given), _) => given.as_str(),
                    (None, Some((_, kept, _))) => kept.as_str(),
                    (None, None) => "",
                };
                sqlx::query(AssertSqlSafe(insert_connection.as_str()))
                    .bind(&c.id)
                    .bind(workspace_id)
                    .bind(&c.kind)
                    .bind(&c.name)
                    .bind(c.settings.to_string())
                    .bind(credentials)
                    .bind(existing.map_or(now_ms, |(_, _, created_at)| *created_at))
                    .bind(now_ms)
                    .execute(&mut *tx)
                    .await?;
            }
            for s in &config.sources {
                sqlx::query(AssertSqlSafe(update_source.as_str()))
                    .bind(&s.name)
                    .bind(&s.mapping)
                    .bind(workspace_id)
                    .bind(&s.id)
                    .execute(&mut *tx)
                    .await?;
            }
            for (key, value) in &config.settings {
                if CONFIG_SETTINGS.contains(&key.as_str()) {
                    sqlx::query(AssertSqlSafe(set_setting.as_str()))
                        .bind(workspace_id)
                        .bind(key)
                        .bind(value)
                        .execute(&mut *tx)
                        .await?;
                }
            }
            tx.commit().await?;
            Ok(())
        })
    }
}
