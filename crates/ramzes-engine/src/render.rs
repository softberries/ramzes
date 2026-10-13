//! Wording a message for a set of events and finding the connector that will carry it.

use std::sync::Arc;

use ramzes_connectors::{ConnectionConfig, Connector};
use ramzes_core::shaping::digest;
use ramzes_core::{Event, Message, Template, apply_template};
use ramzes_store::Connection;

use crate::{Engine, Result};

/// A message ready to send, with a note when the route's template could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Composed {
    pub message: Message,
    pub warning: Option<String>,
}

/// Words the message for `events`. A template that fails is set aside: the default wording is
/// used and the reason is returned as a warning. `None` when there are no events.
///
/// Several events are summarised in one message; `notification_id` is what its link points to.
pub fn compose(
    events: &[Event],
    template: Option<&Template>,
    public_url: Option<&str>,
    notification_id: &str,
) -> Option<Composed> {
    events.first()?;
    let default = digest(events, public_url, notification_id);
    let Some(template) = template else {
        return Some(Composed {
            message: default,
            warning: None,
        });
    };
    Some(match apply_template(template, &default, events) {
        Ok(message) => Composed {
            message,
            warning: None,
        },
        Err(reason) => Composed {
            message: default,
            warning: Some(format!(
                "the template {:?} failed ({reason}); the default wording was sent",
                template.name
            )),
        },
    })
}

impl Engine {
    /// The address of the UI, for links in messages.
    pub async fn public_url(&self, workspace_id: &str) -> Result<Option<String>> {
        if let Some(url) = &self.config.public_url {
            return Ok(Some(url.clone()));
        }
        Ok(self
            .config
            .store
            .get_setting(workspace_id, "public_url")
            .await?
            .filter(|url| !url.is_empty()))
    }

    /// Words the message for `events` with the stored template `template_id`, if one is named.
    pub async fn compose_for(
        &self,
        workspace_id: &str,
        notification_id: &str,
        events: &[Event],
        template_id: Option<&str>,
    ) -> Result<Option<Composed>> {
        let public_url = self.public_url(workspace_id).await?;
        let Some(template_id) = template_id else {
            return Ok(compose(
                events,
                None,
                public_url.as_deref(),
                notification_id,
            ));
        };
        let template = self
            .config
            .store
            .get_template(workspace_id, template_id)
            .await?;
        let mut composed = compose(
            events,
            template.as_ref(),
            public_url.as_deref(),
            notification_id,
        );
        if let (None, Some(composed)) = (&template, &mut composed) {
            composed.warning =
                Some("the route's template was deleted; the default wording was sent".into());
        }
        Ok(composed)
    }

    /// The connector of a connection and its opened configuration, or why it cannot be used.
    pub fn connector_for(
        &self,
        connection: &Connection,
    ) -> std::result::Result<(Arc<dyn Connector>, ConnectionConfig), String> {
        let connector = self.config.registry.get(&connection.kind).ok_or_else(|| {
            format!(
                "this instance has no connector of kind {:?}",
                connection.kind
            )
        })?;
        let credentials = self
            .config
            .opener
            .open(&connection.credentials)
            .map_err(|e| format!("the connection's credentials could not be read: {e}"))?;
        Ok((
            connector,
            ConnectionConfig {
                settings: connection.settings.clone(),
                credentials,
            },
        ))
    }
}
