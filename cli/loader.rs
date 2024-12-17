mod args;
mod auth_tokens;
mod cache;
mod cdp;
mod emit;
mod errors;
mod factory;
mod file_fetcher;
mod graph_container;
mod graph_util;
mod http_util;
mod js;
mod jsr;
mod lsp;
mod module_loader;
mod node;
mod npm;
mod ops;
mod resolver;
mod shared;
mod standalone;
mod task_runner;
mod tools;
mod tsc;
mod util;
mod version;
mod worker;

use crate::args::flags_from_vec;
use crate::args::DenoSubcommand;
use crate::args::Flags;
use crate::util::display;
use crate::util::v8::get_v8_flags_from_env;
use crate::util::v8::init_v8_flags;

use args::TaskFlags;
use deno_resolver::npm::ByonmResolvePkgFolderFromDenoReqError;
use deno_resolver::npm::ResolvePkgFolderFromDenoReqError;
use deno_runtime::WorkerExecutionMode;
pub use deno_runtime::UNSTABLE_GRANULAR_FLAGS;

use deno_core::anyhow::Context;
use deno_core::error::AnyError;
use deno_core::error::JsError;
use deno_core::futures::FutureExt;
use deno_core::unsync::JoinHandle;
use deno_npm::resolution::SnapshotFromLockfileError;
use deno_runtime::fmt_errors::format_js_error;
use deno_runtime::tokio_util::create_and_run_current_thread_with_maybe_metrics;
use deno_terminal::colors;
use factory::CliFactory;
use standalone::MODULE_NOT_FOUND;
use standalone::UNSUPPORTED_SCHEME;
use std::env;
use std::future::Future;
use std::io::IsTerminal;
use std::io::Read as _;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;

pub(crate) fn unstable_exit_cb(feature: &str, api_name: &str) {
  log::error!(
    "Unstable API '{api_name}'. The `--unstable-{}` flag must be provided.",
    feature
  );
  deno_runtime::exit(70);
}

fn main() {
  util::unix::raise_fd_limit();
  util::logger::init(None);
  deno_core::JsRuntime::init_platform(None, false);

  let args: Vec<_> = env::args_os().collect();

  let future = async move {
    let flags = flags_from_vec(args)?;
    dbg!(&flags);
    run_from_stdin(flags.into()).await
  };

  create_and_run_current_thread_with_maybe_metrics(future).unwrap();
}

async fn run_from_stdin(flags: Arc<Flags>) -> Result<i32, AnyError> {
  // dbg!(&flags);
  // tools::run::run_script(WorkerExecutionMode::Run, flags.clone(), None).await

  let factory = CliFactory::from_flags(flags);
  let cli_options = factory.cli_options()?;
  let main_module = cli_options.resolve_main_module()?;
  // maybe_npm_install(&factory).await?;

  let file_fetcher = factory.file_fetcher()?;
  let worker_factory = factory.create_cli_main_worker_factory().await?;
  let mut source = Vec::new();
  std::io::stdin().read_to_end(&mut source)?;
  // Save a fake file into file fetcher cache
  // to allow module access by TS compiler
  file_fetcher.insert_memory_files(file_fetcher::File {
    specifier: main_module.clone(),
    maybe_headers: None,
    source: source.into(),
  });

  let mut worker = worker_factory
    .create_main_worker(WorkerExecutionMode::Run, main_module.clone())
    .await?;
  let exit_code = worker.run().await?;
  Ok(exit_code)
}
