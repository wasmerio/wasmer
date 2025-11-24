use super::*;

pub fn wait_for_snapshot(
    env: &WasiEnv,
) -> Pin<Box<dyn Future<Output = ()> + Send + Sync + 'static>> {
    env.process.wait_for_checkpoint()
}
