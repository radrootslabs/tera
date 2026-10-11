//! Application-owned independently addressable rows over canonical visibility.
//! Thread rows retain a root index without copying profile or media ownership.
use super::*;

const ROW_SCHEMA: u16 = 1;

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub(super) enum TodayRow {
    Card(ProjectedCard),
    Profile {
        event_id: String,
        summary: ProfileSummary,
        metadata: BTreeMap<String, serde_json::Value>,
    },
    Thread(ThreadEntry),
    Quarantined {
        event_id: String,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RowDocument {
    schema_version: u16,
    row: TodayRow,
}

pub(super) fn reduce(
    context: &LocalNetwork,
    stored: &radroots_storage::event::StoredVisibleEvent,
) -> Result<Option<TodayRow>, TodayError> {
    let admitted = verify_nip01_event(stored.event().envelope().clone())
        .ok()
        .and_then(|verified| admit_verified_event(verified).ok());
    let Some(admitted) = admitted else {
        return Ok(Some(TodayRow::Quarantined {
            event_id: stored.event().id().to_hex(),
        }));
    };
    Ok(match &admitted {
        RadrootsAdmittedEvent::Profile(profile) => Some(TodayRow::Profile {
            event_id: profile.event().id_hex(),
            summary: profile_summary(&admitted)?,
            metadata: profile.metadata().raw_fields().clone(),
        }),
        RadrootsAdmittedEvent::Reply(_) => Some(TodayRow::Thread(reply_entry(&admitted)?)),
        RadrootsAdmittedEvent::Comment(_) => comment_entry(&admitted).map(TodayRow::Thread),
        _ => {
            let locality = locality_tags(admitted.event().tags_as_vec());
            let evidence = locality_evidence(context.locality.as_deref(), &locality);
            match classify_admitted_event(&admitted, context.admit(evidence)) {
                ProductEventClassification::Card(card) => Some(TodayRow::Card(ProjectedCard {
                    card: *card,
                    locality,
                })),
                _ => None,
            }
        }
    })
}

impl TodayRow {
    pub(super) fn key(&self) -> String {
        match self {
            Self::Card(value) => format!("card.{}", value.card.card_id.to_hex()),
            Self::Profile { summary, .. } => format!("profile.{}", summary.author_pubkey),
            Self::Thread(entry) => format!(
                "thread.{}.{}",
                hex::encode(Sha256::digest(entry.reference.root.as_bytes())),
                entry.event_id
            ),
            Self::Quarantined { event_id } => format!("quarantine.{event_id}"),
        }
    }

    pub(super) fn document(self) -> Result<ProjectionDocument, TodayError> {
        let key = self.key();
        let value = encode(&RowDocument {
            schema_version: ROW_SCHEMA,
            row: self,
        })?;
        Ok(ProjectionDocument::new(key, value)?)
    }

    pub(super) fn from_document(document: &ProjectionDocument) -> Result<Self, TodayError> {
        let value: RowDocument = decode(document.value())?;
        if value.schema_version != ROW_SCHEMA || value.row.key() != document.key() {
            return Err(TodayError::CorruptProjection);
        }
        Ok(value.row)
    }
}

#[cfg(test)]
#[path = "today_rows_tests.rs"]
mod tests;
