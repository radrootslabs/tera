//! Logical allocation and selected/decoded work are charged before payload loads.
use super::*;
use radroots_storage::projection::document_query::{
    ProjectionDocumentGenerations, ProjectionDocumentKeyRange, ProjectionDocumentPage,
    ProjectionDocumentQuery,
};

pub(super) const BYTES: usize = 16 * 1024 * 1024;
pub(super) const UNITS: usize = 2048;
const METADATA: usize = 1024;
const OVERHEAD: usize = 4096;

pub(super) struct Budget {
    bytes: usize,
    units: usize,
    limit_bytes: usize,
    limit_units: usize,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            bytes: 0,
            units: 0,
            limit_bytes: BYTES,
            limit_units: UNITS,
        }
    }
}

impl Budget {
    pub(super) fn charge(&mut self, bytes: usize, units: usize) -> Result<(), TodayError> {
        let next_bytes = self.bytes.checked_add(bytes);
        let next_units = self.units.checked_add(units);
        match (next_bytes, next_units) {
            (Some(bytes), Some(units))
                if bytes <= self.limit_bytes && units <= self.limit_units =>
            {
                self.bytes = bytes;
                self.units = units;
                Ok(())
            }
            _ => Err(radroots_storage::Error::SpaceInsufficient.into()),
        }
    }

    // Existing visibility accounting reserves sixteen encoded bytes per byte,
    // plus fixed overhead, for decoded values, collection nodes and intermediates.
    // This is a conservative logical envelope, not allocator/RSS measurement.
    fn payload_budget(&self) -> Result<usize, TodayError> {
        self.limit_bytes
            .checked_sub(self.bytes + OVERHEAD)
            .map(|remaining| remaining / 16)
            .filter(|remaining| *remaining != 0)
            .ok_or_else(|| radroots_storage::Error::SpaceInsufficient.into())
    }

    pub(super) fn can_select(&self) -> bool {
        self.units + 4 <= self.limit_units
            && self.bytes + 2 * METADATA + OVERHEAD + 16 <= self.limit_bytes
    }

    /// Reserve the measured scope-load envelope for a final fresh comparison.
    /// Both budgets together remain within the original per-command limits.
    pub(super) fn reserve_scope(&mut self) -> Result<Self, TodayError> {
        let finish = Self {
            bytes: 0,
            units: 0,
            limit_bytes: self.bytes,
            limit_units: self.units,
        };
        self.limit_bytes = self
            .limit_bytes
            .checked_sub(self.bytes)
            .ok_or(radroots_storage::Error::SpaceInsufficient)?;
        self.limit_units = self
            .limit_units
            .checked_sub(self.units)
            .ok_or(radroots_storage::Error::SpaceInsufficient)?;
        self.charge(0, 0)?;
        Ok(finish)
    }

    pub(super) async fn query(
        &mut self,
        storage: &dyn radroots_storage::Storage,
        id: ProjectionId,
        generation: ProjectionGeneration,
        range: (String, String),
        after: Option<String>,
        source: Option<radroots_storage::event::EventSourceToken>,
    ) -> Result<ProjectionDocumentPage, TodayError> {
        // One selected record plus the provider's scalar lookahead are bounded
        // before allocation. Every query, including an empty/rejected one, counts.
        self.charge(2 * METADATA, 2)?;
        let mut query =
            ProjectionDocumentQuery::new(id, ProjectionDocumentGenerations::Exact(generation), 1)?
                .with_key_range(ProjectionDocumentKeyRange::new(range.0, range.1)?)?
                .with_payload_budget(self.payload_budget()?)?;
        if let Some(after) = after {
            query = query.with_after_key(after)?;
        }
        let page = match source {
            Some(source) => {
                ProjectionStore::query_projection_documents_at(storage, query, source, generation)
                    .await
                    .map_err(|error| match error {
                        radroots_storage::Error::SourceEpochChanged
                        | radroots_storage::Error::ProjectionRevisionConflict => {
                            TodayError::Cursor(CursorError::Stale)
                        }
                        error => TodayError::Storage(error),
                    })?
            }
            None => ProjectionStore::query_projection_documents(storage, query).await?,
        };
        for record in page.records() {
            let document = record.document().ok_or(TodayError::CorruptProjection)?;
            self.charge(
                document
                    .value()
                    .len()
                    .checked_mul(16)
                    .and_then(|bytes| bytes.checked_add(OVERHEAD))
                    .ok_or(radroots_storage::Error::SpaceInsufficient)?,
                1,
            )?;
        }
        Ok(page)
    }

    pub(super) async fn document(
        &mut self,
        storage: &dyn radroots_storage::Storage,
        id: ProjectionId,
        generation: ProjectionGeneration,
        key: String,
        source: Option<radroots_storage::event::EventSourceToken>,
    ) -> Result<Option<ProjectionDocument>, TodayError> {
        let page = self
            .query(storage, id, generation, (key.clone(), key), None, source)
            .await?;
        Ok(page
            .into_records()
            .pop()
            .and_then(|record| record.document().cloned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_logical_budget_accepts_exact_maxima_and_refuses_one_more() {
        let mut budget = Budget::default();
        budget.charge(BYTES, UNITS).unwrap();
        assert!(budget.charge(0, 1).is_err());
        assert!(budget.charge(1, 0).is_err());
        assert!(budget.charge(usize::MAX, usize::MAX).is_err());
        assert!(!budget.can_select());
    }
}
