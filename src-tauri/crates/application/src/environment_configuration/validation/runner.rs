use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

use super::{
    EnvironmentCandidateValidator, EnvironmentCpuWorkProbe, EnvironmentDomainProjectionPort,
    EnvironmentPreviewBaselinePort, EnvironmentPreviewBaselineRequest,
    EnvironmentProjectedCandidate, EnvironmentValidationCheckpoint, EnvironmentValidationLayer,
    EnvironmentValidationLayerPort, EnvironmentValidationLayerRequest, EnvironmentValidationReport,
    EnvironmentValidationResult, EnvironmentValidationStatus, ORDER,
    outcome::{LayerOutcome, append_skipped, cancelled, cancelled_report, layer_failure, millis},
    projection::ValidationProjection,
};
use crate::{
    AppResult, EnvironmentCandidateId, EnvironmentIdentityAllocator, EnvironmentStatusCode,
    environment_configuration::EnvironmentAdmittedTarget,
};

type Candidate = crate::environment_configuration::EnvironmentConfigurationCandidateV1;

struct CandidateBuffer {
    bytes: Zeroizing<Vec<u8>>,
    probe: Option<Arc<dyn EnvironmentCpuWorkProbe>>,
}

impl CandidateBuffer {
    fn as_slice(&self) -> &[u8] {
        self.bytes.as_slice()
    }
}

impl Drop for CandidateBuffer {
    fn drop(&mut self) {
        if let Some(probe) = &self.probe {
            probe.candidate_buffer_dropped();
        }
    }
}

enum LayerCompletion {
    Schema(Candidate, EnvironmentValidationStatus),
    Domain(ValidationProjection, EnvironmentValidationStatus),
    ProjectedDomain(EnvironmentProjectedCandidate, EnvironmentValidationStatus),
    Port(EnvironmentValidationStatus),
}

impl LayerCompletion {
    const fn status(&self) -> EnvironmentValidationStatus {
        match self {
            Self::Schema(_, status)
            | Self::Domain(_, status)
            | Self::ProjectedDomain(_, status)
            | Self::Port(status) => *status,
        }
    }
}

impl<P> EnvironmentCandidateValidator<P>
where
    P: EnvironmentValidationLayerPort + ?Sized,
{
    pub async fn validate(
        &self,
        candidate_json: &[u8],
        cancellation: CancellationToken,
    ) -> EnvironmentValidationReport {
        self.run_validation(candidate_json, cancellation.clone(), cancellation, None)
            .await
    }

    pub(crate) async fn validate_for_candidate(
        &self,
        candidate_id: &EnvironmentCandidateId,
        candidate_json: &[u8],
        request_cancellation: CancellationToken,
        candidate_cancellation: CancellationToken,
        preview_port: &dyn EnvironmentPreviewBaselinePort,
    ) -> EnvironmentValidationReport {
        self.run_validation(
            candidate_json,
            request_cancellation,
            candidate_cancellation,
            Some((
                candidate_id,
                preview_port,
                preview_port.domain_projection_port(),
            )),
        )
        .await
    }

    #[expect(
        clippy::too_many_lines,
        reason = "the fixed seven-layer select loop keeps cancellation and deadline precedence auditable"
    )]
    async fn run_validation(
        &self,
        candidate_json: &[u8],
        request_cancellation: CancellationToken,
        candidate_cancellation: CancellationToken,
        preview: Option<(
            &EnvironmentCandidateId,
            &dyn EnvironmentPreviewBaselinePort,
            Option<&dyn EnvironmentDomainProjectionPort>,
        )>,
    ) -> EnvironmentValidationReport {
        let total_deadline = Instant::now() + self.total_deadline;
        let bytes = Arc::new(CandidateBuffer {
            bytes: Zeroizing::new(candidate_json.to_vec()),
            probe: self.cpu_work_probe.clone(),
        });
        let mut candidate = None;
        let mut projection: Option<ValidationProjection> = None;
        let mut projected_candidate: Option<EnvironmentProjectedCandidate> = None;
        let mut layers = Vec::with_capacity(ORDER.len());

        for (index, layer) in ORDER.into_iter().enumerate() {
            let layer_started = Instant::now();
            let layer_deadline = layer_started + self.layer_budgets[index];
            let outcome = match layer {
                EnvironmentValidationLayer::Schema => {
                    let input = Arc::clone(&bytes);
                    let port = self.port.clone();
                    let control = EnvironmentValidationControl::new(
                        &request_cancellation,
                        &candidate_cancellation,
                        total_deadline,
                        layer_deadline,
                        self.cpu_work_probe.as_ref(),
                        layer,
                    );
                    let cpu = run_cpu_stage(
                        || ValidationProjection::parse_schema(input.as_slice()),
                        &control,
                    );
                    finish_stage(
                        cpu,
                        move |parsed| async move {
                            let status = port
                                .validate_layer(EnvironmentValidationLayerRequest::empty(layer))
                                .await?;
                            Ok(LayerCompletion::Schema(parsed, status))
                        },
                        &control,
                    )
                    .await
                }
                EnvironmentValidationLayer::Domain => {
                    let parsed = candidate.take().expect("schema passed before domain");
                    let port = self.port.clone();
                    let control = EnvironmentValidationControl::new(
                        &request_cancellation,
                        &candidate_cancellation,
                        total_deadline,
                        layer_deadline,
                        self.cpu_work_probe.as_ref(),
                        layer,
                    );
                    let cpu = if let Some(domain_port) = preview.and_then(|(_, _, port)| port) {
                        let control_ref = &control;
                        run_controlled_async(
                            async move {
                                domain_port
                                    .project_environment_candidate(parsed, control_ref)
                                    .await
                                    .map(EitherDomain::Projected)
                            },
                            &control,
                        )
                        .await
                    } else {
                        run_cpu_stage(
                            || {
                                if matches!(
                                    parsed.lifecycle_target(),
                                    EnvironmentAdmittedTarget::New { .. }
                                ) {
                                    let allocator = EnvironmentIdentityAllocator::random();
                                    EnvironmentProjectedCandidate::project(
                                        parsed,
                                        None,
                                        allocator.port(),
                                    )
                                    .map(EitherDomain::Projected)
                                } else {
                                    ValidationProjection::project(parsed)
                                        .map(EitherDomain::Stateless)
                                }
                            },
                            &control,
                        )
                    };
                    finish_stage(
                        cpu,
                        move |completed| async move {
                            match completed {
                                EitherDomain::Stateless(projected) => {
                                    let status =
                                        port.validate_layer(projected.request(layer)).await?;
                                    Ok(LayerCompletion::Domain(projected, status))
                                }
                                EitherDomain::Projected(projected) => {
                                    let status =
                                        port.validate_layer(projected.request(layer)).await?;
                                    Ok(LayerCompletion::ProjectedDomain(projected, status))
                                }
                            }
                        },
                        &control,
                    )
                    .await
                }
                _ => {
                    let work: Pin<
                        Box<dyn Future<Output = AppResult<LayerCompletion>> + Send + '_>,
                    > = match layer {
                        EnvironmentValidationLayer::PreviewBaseline
                            if let Some((candidate_id, preview_port, _)) = preview =>
                        {
                            let request = EnvironmentPreviewBaselineRequest {
                                candidate_id,
                                #[cfg(test)]
                                validated_candidate_json: bytes.as_slice(),
                                prior_layers: &layers,
                                projected_candidate: projected_candidate.as_ref(),
                            };
                            Box::pin(async move {
                                preview_port
                                    .validate_preview_baseline(request)
                                    .await
                                    .map(|()| {
                                        LayerCompletion::Port(EnvironmentValidationStatus::Passed)
                                    })
                            })
                        }
                        _ => {
                            let request = projected_candidate.as_ref().map_or_else(
                                || {
                                    projection
                                        .as_ref()
                                        .expect("domain passed before dependent validation")
                                        .request(layer)
                                },
                                |projected| projected.request(layer),
                            );
                            Box::pin(async move {
                                self.port
                                    .validate_layer(request)
                                    .await
                                    .map(LayerCompletion::Port)
                            })
                        }
                    };
                    tokio::select! {
                        biased;
                        () = candidate_cancellation.cancelled() => LayerOutcome::Cancelled,
                        () = request_cancellation.cancelled() => LayerOutcome::Cancelled,
                        () = tokio::time::sleep_until(total_deadline) => LayerOutcome::TotalDeadline,
                        () = tokio::time::sleep_until(layer_deadline) => LayerOutcome::LayerDeadline,
                        result = work => LayerOutcome::Completed(result),
                    }
                }
            };
            let duration_ms = millis(layer_started.elapsed());
            match outcome {
                LayerOutcome::Completed(Ok(completed)) => {
                    let status = completed.status();
                    match completed {
                        LayerCompletion::Schema(parsed, _) => candidate = Some(parsed),
                        LayerCompletion::Domain(projected, _) => projection = Some(projected),
                        LayerCompletion::ProjectedDomain(projected, _) => {
                            projected_candidate = Some(projected);
                        }
                        LayerCompletion::Port(_) => {}
                    }
                    layers.push(EnvironmentValidationResult {
                        layer,
                        status,
                        code: None,
                        reason: None,
                        duration_ms,
                    });
                    if !matches!(
                        status,
                        EnvironmentValidationStatus::Passed
                            | EnvironmentValidationStatus::NotApplicable
                    ) {
                        append_skipped(&mut layers, layer, "dependency_not_satisfied");
                        return EnvironmentValidationReport {
                            layers,
                            status_code: Some(EnvironmentStatusCode::ValidationLayerFailed),
                        };
                    }
                }
                LayerOutcome::Completed(Err(error)) => {
                    return layer_failure(&error, layers, layer, duration_ms);
                }
                LayerOutcome::Cancelled => {
                    let code = crate::environment_configuration::lifecycle::take_validation_cancellation_code(
                        &candidate_cancellation,
                    )
                    .unwrap_or(EnvironmentStatusCode::CandidateCancelled);
                    return cancelled_report(layers, layer, duration_ms, "request_cancelled", code);
                }
                LayerOutcome::TotalDeadline => {
                    layers.push(cancelled(layer, duration_ms, "create_deadline_exceeded"));
                    append_skipped(&mut layers, layer, "create_deadline_exceeded");
                    return EnvironmentValidationReport {
                        layers,
                        status_code: Some(EnvironmentStatusCode::McpCreateDeadlineExceeded),
                    };
                }
                LayerOutcome::LayerDeadline => {
                    layers.push(EnvironmentValidationResult {
                        layer,
                        status: EnvironmentValidationStatus::Failed,
                        code: Some(EnvironmentStatusCode::ValidationLayerFailed),
                        reason: Some("layer_budget_exceeded"),
                        duration_ms,
                    });
                    append_skipped(&mut layers, layer, "layer_budget_exceeded");
                    return EnvironmentValidationReport {
                        layers,
                        status_code: Some(EnvironmentStatusCode::ValidationLayerFailed),
                    };
                }
            }
        }

        EnvironmentValidationReport {
            layers,
            status_code: None,
        }
    }
}

enum EitherDomain {
    Stateless(ValidationProjection),
    Projected(EnvironmentProjectedCandidate),
}

async fn finish_stage<T, Next, NextFuture>(
    work: LayerOutcome<T>,
    next: Next,
    control: &EnvironmentValidationControl<'_>,
) -> LayerOutcome<LayerCompletion>
where
    Next: FnOnce(T) -> NextFuture,
    NextFuture: Future<Output = AppResult<LayerCompletion>> + Send,
{
    let value = match work {
        LayerOutcome::Completed(Ok(value)) => value,
        LayerOutcome::Completed(Err(error)) => return LayerOutcome::Completed(Err(error)),
        LayerOutcome::Cancelled => return LayerOutcome::Cancelled,
        LayerOutcome::TotalDeadline => return LayerOutcome::TotalDeadline,
        LayerOutcome::LayerDeadline => return LayerOutcome::LayerDeadline,
    };
    let next = next(value);
    tokio::pin!(next);
    tokio::select! {
        biased;
        () = control.candidate_cancellation.cancelled() => LayerOutcome::Cancelled,
        () = control.request_cancellation.cancelled() => LayerOutcome::Cancelled,
        () = tokio::time::sleep_until(control.total_deadline) => LayerOutcome::TotalDeadline,
        () = tokio::time::sleep_until(control.layer_deadline) => LayerOutcome::LayerDeadline,
        result = &mut next => LayerOutcome::Completed(result),
    }
}

struct EnvironmentValidationControl<'a> {
    request_cancellation: &'a CancellationToken,
    candidate_cancellation: &'a CancellationToken,
    total_deadline: Instant,
    layer_deadline: Instant,
    probe: Option<&'a Arc<dyn EnvironmentCpuWorkProbe>>,
    layer: EnvironmentValidationLayer,
    checkpoint_index: AtomicUsize,
}

impl<'a> EnvironmentValidationControl<'a> {
    const fn new(
        request_cancellation: &'a CancellationToken,
        candidate_cancellation: &'a CancellationToken,
        total_deadline: Instant,
        layer_deadline: Instant,
        probe: Option<&'a Arc<dyn EnvironmentCpuWorkProbe>>,
        layer: EnvironmentValidationLayer,
    ) -> Self {
        Self {
            request_cancellation,
            candidate_cancellation,
            total_deadline,
            layer_deadline,
            probe,
            layer,
            checkpoint_index: AtomicUsize::new(0),
        }
    }

    fn checkpoint<T>(&self) -> Option<LayerOutcome<T>> {
        let checkpoint_index = self.checkpoint_index.fetch_add(1, Ordering::Relaxed);
        if let Some(probe) = self.probe {
            probe.checkpoint(self.layer, checkpoint_index);
        }
        self.check_stop()
    }

    fn check_stop<T>(&self) -> Option<LayerOutcome<T>> {
        if self.candidate_cancellation.is_cancelled() || self.request_cancellation.is_cancelled() {
            Some(LayerOutcome::Cancelled)
        } else if Instant::now() >= self.total_deadline {
            Some(LayerOutcome::TotalDeadline)
        } else if Instant::now() >= self.layer_deadline {
            Some(LayerOutcome::LayerDeadline)
        } else {
            None
        }
    }
}

impl EnvironmentValidationCheckpoint for EnvironmentValidationControl<'_> {
    fn checkpoint(&self) -> bool {
        self.checkpoint::<()>().is_some()
    }
}

fn run_cpu_stage<T>(
    work: impl FnOnce() -> AppResult<T>,
    control: &EnvironmentValidationControl<'_>,
) -> LayerOutcome<T> {
    if let Some(outcome) = control.checkpoint() {
        return outcome;
    }
    let result = work();
    if let Some(outcome) = control.checkpoint() {
        return outcome;
    }
    LayerOutcome::Completed(result)
}

async fn run_controlled_async<T>(
    work: impl Future<Output = AppResult<T>>,
    control: &EnvironmentValidationControl<'_>,
) -> LayerOutcome<T> {
    if let Some(outcome) = control.checkpoint() {
        return outcome;
    }
    tokio::pin!(work);
    let result = tokio::select! {
        biased;
        () = control.candidate_cancellation.cancelled() => return LayerOutcome::Cancelled,
        () = control.request_cancellation.cancelled() => return LayerOutcome::Cancelled,
        () = tokio::time::sleep_until(control.total_deadline) => return LayerOutcome::TotalDeadline,
        () = tokio::time::sleep_until(control.layer_deadline) => return LayerOutcome::LayerDeadline,
        result = &mut work => result,
    };
    if let Some(outcome) = control.check_stop() {
        return outcome;
    }
    LayerOutcome::Completed(result)
}
