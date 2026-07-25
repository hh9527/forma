use forma::{EngineConfig, Quota};
use forma_lsp::Server;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::current_dir()?;
    run(
        root,
        EngineConfig {
            module_quota: Quota::with_fuel(1_000_000),
            session_quota: Quota::with_fuel(1_000_000),
        },
    )
    .await
}

async fn run(
    root: std::path::PathBuf,
    config: EngineConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
            let (main_loop, _) =
                async_lsp::MainLoop::new_server(|client| Server::new(root, config, client));

            #[cfg(unix)]
            let (stdin, stdout) = (
                async_lsp::stdio::PipeStdin::lock_tokio()?,
                async_lsp::stdio::PipeStdout::lock_tokio()?,
            );
            #[cfg(not(unix))]
            let (stdin, stdout) = (
                tokio_util::compat::TokioAsyncReadCompatExt::compat(tokio::io::stdin()),
                tokio_util::compat::TokioAsyncWriteCompatExt::compat_write(tokio::io::stdout()),
            );

            main_loop.run_buffered(stdin, stdout).await
        })
        .await?;
    Ok(())
}
