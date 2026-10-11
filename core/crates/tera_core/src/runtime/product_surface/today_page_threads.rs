//! Merge the two existing root indexes in canonical event-ID order.
use super::*;

struct Stream {
    root: String,
    prefix: String,
    after: Option<String>,
    head: Option<ThreadEntry>,
    done: bool,
}

impl Stream {
    fn new(root: &str, after: Option<CardId>) -> Self {
        let prefix = format!("thread.{}.", hex::encode(Sha256::digest(root.as_bytes())));
        let after = after.map(|id| format!("{prefix}{}", id.to_hex()));
        Self {
            root: root.into(),
            prefix,
            after,
            head: None,
            done: false,
        }
    }

    async fn fill(
        &mut self,
        runtime: &TeraRuntime,
        scope: &page_scope::Scope,
        budget: &mut page_budget::Budget,
    ) -> Result<(), TodayError> {
        if self.done || self.head.is_some() {
            return Ok(());
        }
        let storage = runtime
            .client
            .storage()
            .map_err(|_| TodayError::RuntimeUnavailable)?;
        let page = budget
            .query(
                storage,
                ProjectionId::parse(build::progress::ID)?,
                scope.local.generation,
                (self.prefix.clone(), format!("{}~", self.prefix)),
                self.after.clone(),
                Some(scope.source),
            )
            .await?;
        self.done = page.next_cursor().is_none();
        if let Some(record) = page.records().first() {
            match rows::TodayRow::from_document(
                record.document().ok_or(TodayError::CorruptProjection)?,
            )? {
                rows::TodayRow::Thread(entry) if entry.reference.root == self.root => {
                    CardId::parse(&entry.event_id).map_err(|_| TodayError::CorruptProjection)?;
                    self.after = Some(record.key().into());
                    self.head = Some(entry);
                }
                _ => return Err(TodayError::CorruptProjection),
            }
        }
        Ok(())
    }
}

pub(super) async fn page(
    runtime: &TeraRuntime,
    scope: &page_scope::Scope,
    card: &ClassifiedCard,
    after: Option<CardId>,
    limit: u16,
    budget: &mut page_budget::Budget,
) -> Result<(Vec<ThreadEntry>, Option<CardId>), TodayError> {
    let mut streams = vec![Stream::new(&card.source_event_id, after)];
    if let Some(address) = card.source_address.as_deref()
        && address != card.source_event_id
    {
        streams.push(Stream::new(address, after));
    }
    let mut items: Vec<ThreadEntry> = Vec::new();
    loop {
        for stream in &mut streams {
            stream.fill(runtime, scope, budget).await?;
        }
        let selected = streams
            .iter()
            .enumerate()
            .filter_map(|(index, stream)| {
                stream.head.as_ref().map(|entry| (index, &entry.event_id))
            })
            .min_by(|left, right| left.1.cmp(right.1))
            .map(|(index, _)| index);
        let Some(selected) = selected else {
            return Ok((items, None));
        };
        if items.len() == usize::from(limit) {
            let last = CardId::parse(&items.last().ok_or(TodayError::CorruptProjection)?.event_id)
                .map_err(|_| TodayError::CorruptProjection)?;
            return Ok((items, Some(last)));
        }
        let entry = streams[selected].head.take().expect("selected head");
        if items
            .last()
            .is_some_and(|last| last.event_id >= entry.event_id)
        {
            return Err(TodayError::CorruptProjection);
        }
        items.push(entry);
    }
}

pub(super) async fn validate_after(
    runtime: &TeraRuntime,
    scope: &page_scope::Scope,
    card: &ClassifiedCard,
    after: CardId,
    budget: &mut page_budget::Budget,
) -> Result<(), TodayError> {
    for root in [
        Some(card.source_event_id.as_str()),
        card.source_address.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        let key = format!(
            "thread.{}.{}",
            hex::encode(Sha256::digest(root.as_bytes())),
            after.to_hex()
        );
        if let Some(row) = page_hydration::row(runtime, scope, key, budget).await? {
            return match row {
                rows::TodayRow::Thread(entry)
                    if entry.event_id == after.to_hex() && entry.reference.root == root =>
                {
                    Ok(())
                }
                _ => Err(TodayError::CorruptProjection),
            };
        }
    }
    Err(TodayError::CursorPositionMissing)
}
