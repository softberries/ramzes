use std::collections::BTreeMap;

use crate::{Engine, Result};

const DAY_MS: i64 = 86_400_000;
const DEFAULT_RETENTION_DAYS: i64 = 30;
/// Events deleted per statement, so a large backlog does not hold one long transaction.
const BATCH: i64 = 500;

impl Engine {
    /// Deletes events older than the retention settings allow. Returns how many were removed.
    ///
    /// `retention_days` applies to every topic; `retention_overrides` maps a topic to its own
    /// number of days. Zero days means keep forever.
    pub async fn retention_once(&self, workspace_id: &str) -> Result<u64> {
        let store = &self.config.store;
        let days = match store.get_setting(workspace_id, "retention_days").await? {
            Some(value) => value.parse::<i64>().unwrap_or(DEFAULT_RETENTION_DAYS),
            None => DEFAULT_RETENTION_DAYS,
        };
        let overrides: BTreeMap<String, i64> = match store
            .get_setting(workspace_id, "retention_overrides")
            .await?
        {
            Some(value) => serde_json::from_str(&value).unwrap_or_default(),
            None => BTreeMap::new(),
        };
        let now = self.now();
        let mut deleted = 0;

        for (topic, topic_days) in &overrides {
            if *topic_days > 0 {
                deleted += self
                    .delete_all_before(workspace_id, now - topic_days * DAY_MS, Some(topic), &[])
                    .await?;
            }
        }
        if days > 0 {
            let excluded: Vec<String> = overrides.keys().cloned().collect();
            deleted += self
                .delete_all_before(workspace_id, now - days * DAY_MS, None, &excluded)
                .await?;
        }
        Ok(deleted)
    }

    async fn delete_all_before(
        &self,
        workspace_id: &str,
        cutoff: i64,
        topic: Option<&str>,
        except: &[String],
    ) -> Result<u64> {
        let mut total = 0;
        loop {
            let deleted = self
                .config
                .store
                .delete_events_before(workspace_id, cutoff, topic, except, BATCH)
                .await?;
            total += deleted;
            if deleted == 0 {
                return Ok(total);
            }
        }
    }
}
