use super::FfiLocalNetworkRecord;

impl std::fmt::Debug for FfiLocalNetworkRecord {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FfiLocalNetworkRecord")
            .field("schema_version", &self.schema_version)
            .field("relay_count", &self.relay_urls.len())
            .field("followed_author_count", &self.followed_authors.len())
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}
