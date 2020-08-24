use crate::*;
use ghost_actor::dependencies::tracing;

/// Apply some tracing to spawned task loops.
pub fn err_spawn<F>(hint: &'static str, f: F)
where
    F: std::future::Future<Output = LairResult<()>> + 'static + Send,
{
    tokio::task::spawn(async move {
        match f.await {
            Ok(_) => tracing::debug!("FUTURE {} ENDED Ok!!!", hint),
            Err(e) => tracing::warn!("FUTURE {} ENDED Err: {:?}", hint, e),
        }
    });
}
