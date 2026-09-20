use super::*;

impl TeraRuntime {
    /// Resolves one renderable artifact only after rechecking the exact local
    /// file. Missing or corrupt bytes atomically revoke all matching receipts.
    #[cfg(feature = "mobile-social")]
    pub async fn phase1_verified_media_artifact(
        &self,
        context: &LocalNetwork,
        artifact_id: Phase1MediaArtifactId,
        observed_at_unix_ms: u64,
    ) -> Result<Option<Phase1LocalMediaArtifact>, TodayError> {
        let _command = self.lifecycle.enter()?;
        let _guard = self.inbound_media_lock.lock().await;
        let directory = self
            .inbound_media_directory
            .as_deref()
            .ok_or(Phase1InboundMediaError::CacheUnavailable)?;
        let storage = self
            .client
            .storage()
            .map_err(|_| TodayError::RuntimeUnavailable)?;
        let _projection = self.today_projection_lock.lock().await;
        let generation = projection_generation()?;
        let mut state = media_visibility::current_state(self, context).await?;
        let Some(receipt) = verified_receipt(&state, artifact_id) else {
            return Ok(None);
        };
        match super::super::media::verified_artifact(directory, &receipt).await {
            Ok(artifact) => {
                state = media_visibility::current_state(self, context).await?;
                if state.media_cache.touch(artifact_id, observed_at_unix_ms)? {
                    persist_media_state(self, storage, context, generation, &mut state).await?;
                }
                Ok(Some(artifact))
            }
            Err(error) => {
                state = media_visibility::current_state(self, context).await?;
                state.media_cache.invalidate_artifact(artifact_id);
                invalidate_artifact_references(&mut state, artifact_id);
                persist_media_state(self, storage, context, generation, &mut state).await?;
                let _ = media_collection::collect(
                    self,
                    directory,
                    &[artifact_id],
                    &_guard,
                    &_projection,
                )
                .await;
                Err(error.into())
            }
        }
    }

    /// Completes one bounded BUD-01 retrieval, exact-byte verification, and
    /// atomic content-addressed cache commit under the configured Blossom slot.
    #[cfg(feature = "mobile-social")]
    pub async fn phase1_retrieve_media(
        &self,
        context: &LocalNetwork,
        reference_fingerprint: [u8; 32],
        operation_id: [u8; 16],
        policy: Phase1MediaCachePolicy,
        cancellation: BlossomCancellation,
    ) -> Result<Phase1LocalMediaArtifact, TodayError> {
        let _command = self.lifecycle.enter()?;
        let directory = self
            .inbound_media_directory
            .as_deref()
            .ok_or(Phase1InboundMediaError::CacheUnavailable)?;
        let blossom = self
            .client
            .blossom()
            .map_err(|_| TodayError::RuntimeUnavailable)?
            .cloned()
            .ok_or(TodayError::RuntimeUnavailable)?;
        let sdk_configuration = blossom
            .config_fingerprint()
            .ok_or(TodayError::RuntimeUnavailable)?;
        let configuration =
            Phase1MediaConfigurationFingerprint::new(*sdk_configuration.as_bytes())?;
        let structural = load_structural_reference(self, context, reference_fingerprint).await?;
        let started_at_unix_ms = inbound_now_unix_ms()?;
        if !self
            .phase1_begin_media_retrieval(
                context,
                reference_fingerprint,
                Phase1InboundMediaPending::new(operation_id, configuration, started_at_unix_ms)?,
            )
            .await?
        {
            return Err(TodayError::InvalidRequest);
        }
        let request = match inbound_request(&structural) {
            Ok(request) => request,
            Err(error) => {
                record_inbound_failure(
                    self,
                    context,
                    reference_fingerprint,
                    operation_id,
                    "invalid_reference",
                    false,
                )
                .await;
                return Err(error);
            }
        };
        let sdk_receipt = match blossom.retrieve(request, cancellation).await {
            Ok(receipt) => receipt,
            Err(error) => {
                record_inbound_failure(
                    self,
                    context,
                    reference_fingerprint,
                    operation_id,
                    error.code().trim_start_matches("blossom_"),
                    error.retryable(),
                )
                .await;
                return Err(error.into());
            }
        };
        if sdk_receipt.config_fingerprint() != sdk_configuration {
            record_inbound_failure(
                self,
                context,
                reference_fingerprint,
                operation_id,
                "configuration_changed",
                false,
            )
            .await;
            return Err(Phase1InboundMediaError::ConfigurationMismatch.into());
        }
        let dimensions = sdk_receipt.dimensions();
        let receipt = match Phase1VerifiedMediaReceipt::from_commitment(
            &structural,
            sdk_receipt.final_url().clone(),
            sdk_receipt.commitment(),
            dimensions.width(),
            dimensions.height(),
            configuration,
            sdk_receipt.verified_at_unix_ms(),
        ) {
            Ok(receipt) => receipt,
            Err(error) => {
                record_inbound_failure(
                    self,
                    context,
                    reference_fingerprint,
                    operation_id,
                    "verification_failed",
                    false,
                )
                .await;
                return Err(error.into());
            }
        };
        // Serialize only the local file/receipt phase, never the network wait.
        let _guard = self.inbound_media_lock.lock().await;
        // A completed network request does not retain obsolete visibility.
        if load_structural_reference(self, context, reference_fingerprint).await? != structural {
            return Err(TodayError::InvalidRequest);
        }
        let artifact = match super::super::media::write_verified_artifact(
            directory,
            &receipt,
            sdk_receipt.bytes(),
        )
        .await
        {
            Ok(artifact) => artifact,
            Err(error) => {
                record_inbound_failure(
                    self,
                    context,
                    reference_fingerprint,
                    operation_id,
                    "cache_write_failed",
                    true,
                )
                .await;
                return Err(error.into());
            }
        };
        let evicted = match self
            .phase1_commit_media_receipt(
                context,
                reference_fingerprint,
                operation_id,
                receipt,
                policy,
                sdk_receipt.verified_at_unix_ms(),
            )
            .await
        {
            Ok(evicted) => evicted,
            Err(error) => {
                record_inbound_failure(
                    self,
                    context,
                    reference_fingerprint,
                    operation_id,
                    "cache_commit_failed",
                    true,
                )
                .await;
                return Err(error);
            }
        };
        let _projection = self.today_projection_lock.lock().await;
        media_collection::collect(self, directory, &evicted, &_guard, &_projection).await?;
        Ok(artifact)
    }
}

#[cfg(feature = "mobile-social")]
async fn load_structural_reference(
    runtime: &TeraRuntime,
    context: &LocalNetwork,
    reference_fingerprint: [u8; 32],
) -> Result<Phase1StructuralMediaReference, TodayError> {
    let state = media_visibility::current_state(runtime, context).await?;
    state
        .cards
        .iter()
        .flat_map(|projected| projected.card.media.iter())
        .chain(
            state
                .profiles
                .values()
                .flat_map(|profile| [&profile.picture, &profile.banner].into_iter().flatten()),
        )
        .find(|media| media.structural().fingerprint() == &reference_fingerprint)
        .map(|media| media.structural().clone())
        .ok_or(TodayError::InvalidRequest)
}
