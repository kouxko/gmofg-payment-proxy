use std::future::Future;

use futures_util::{TryStreamExt, stream};
use intercept_proxy_application::{AppError, AppResult};

pub(super) async fn run_bounded_probes<T, F, Fut>(
    items: Vec<T>,
    limit: usize,
    probe: F,
) -> AppResult<()>
where
    F: FnMut(T) -> Fut,
    Fut: Future<Output = AppResult<()>>,
{
    stream::iter(items.into_iter().map(Ok::<_, AppError>))
        .try_for_each_concurrent(limit, probe)
        .await
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use super::*;

    struct ActiveGuard(Arc<AtomicUsize>);

    impl Drop for ActiveGuard {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn first_failure_drops_siblings_and_never_exceeds_four_probes() {
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(AtomicUsize::new(0));
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            run_bounded_probes((0..12).collect(), 4, {
                let active = Arc::clone(&active);
                let peak = Arc::clone(&peak);
                let started = Arc::clone(&started);
                move |index| {
                    let active = Arc::clone(&active);
                    let peak = Arc::clone(&peak);
                    let started = Arc::clone(&started);
                    async move {
                        let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                        peak.fetch_max(current, Ordering::SeqCst);
                        started.fetch_add(1, Ordering::SeqCst);
                        let _guard = ActiveGuard(active);
                        if index == 0 {
                            while started.load(Ordering::SeqCst) < 4 {
                                tokio::task::yield_now().await;
                            }
                            return Err(AppError::new("FIRST", "first probe failed"));
                        }
                        std::future::pending().await
                    }
                }
            }),
        )
        .await
        .expect("first failure cancels pending siblings");

        assert_eq!(result.unwrap_err().view_model.code, "FIRST");
        assert_eq!(peak.load(Ordering::SeqCst), 4);
        assert_eq!(started.load(Ordering::SeqCst), 4);
        assert_eq!(active.load(Ordering::SeqCst), 0);
    }
}
