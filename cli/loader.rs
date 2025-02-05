pub mod args;
pub mod cache;
pub mod cdp;
pub mod emit;
pub mod factory;
pub mod file_fetcher;
pub mod graph_container;
pub mod graph_util;
pub mod http_util;
pub mod js;
pub mod jsr;
pub mod lsp;
pub mod module_loader;
pub mod node;
pub mod npm;
pub mod ops;
pub mod resolver;
pub mod standalone;
pub mod task_runner;
pub mod tools;
pub mod tsc;
pub mod util;
pub mod worker;

pub mod sys {
  #[allow(clippy::disallowed_types)] // ok, definition
  pub type CliSys = sys_traits::impls::RealSys;
}

use std::io::Read as _;
use std::sync::Arc;

use crate::args::Flags;
use crate::factory::CliFactory;
use crate::util::display;

use deno_core::error::AnyError;
use deno_runtime::tokio_util::create_and_run_current_thread_with_maybe_metrics;
use deno_runtime::WorkerExecutionMode;
pub use deno_runtime::UNSTABLE_GRANULAR_FLAGS;
use deno_terminal::colors;

fn main() {
  util::unix::raise_fd_limit();
  // deno_lib::util::logger::init
  deno_core::JsRuntime::init_platform(None, false);

  // let args: Vec<_> = env::args_os().collect();

  let future = async move {
    // let flags = flags_from_vec(args)?;
    let mut flags = Flags::default();
    flags.permissions.allow_all = true;
    flags.code_cache_enabled = true;
    // dbg!(&flags);

    let mut source = Vec::new();
    std::io::stdin().read_to_end(&mut source)?;

    run(&source, flags.into()).await
  };

  create_and_run_current_thread_with_maybe_metrics(future).unwrap();
  // let rt = tokio::runtime::Builder::new_current_thread()
  //   .enable_all()
  //   .build()
  //   .unwrap();
  // rt.block_on(future).unwrap();
}

// run_from_stdin
async fn run(source: &[u8], flags: Arc<Flags>) -> Result<i32, AnyError> {
  // tools::run::run_script(WorkerExecutionMode::Run, flags.clone(), None).await

  let cli_factory = CliFactory::from_flags(flags);
  // let cli_options = cli_factory.cli_options()?;
  // let main_module = cli_options.resolve_main_module()?;
  let main_module = deno_core::resolve_url_or_path(
    "./$deno$stdin.mts",
    &std::env::current_dir()?,
  )?;

  // maybe_npm_install(&factory).await?;

  // Save a fake file into file fetcher cache to allow module access by TS compiler
  let file_fetcher = cli_factory.file_fetcher()?;
  file_fetcher.insert_memory_files(deno_cache_dir::file_fetcher::File {
    url: main_module.clone(),
    maybe_headers: None,
    source: source.into(),
  });

  let worker_factory = cli_factory.create_cli_main_worker_factory().await?;

  let mut worker = worker_factory
    .create_main_worker(WorkerExecutionMode::Run, main_module.clone())
    .await?;

  let exit_code = worker.run().await?;
  Ok(exit_code)
}

pub(crate) fn unstable_exit_cb(feature: &str, api_name: &str) {
  log::error!(
    "Unstable API '{api_name}'. The `--unstable-{}` flag must be provided.",
    feature
  );
  deno_runtime::exit(70);
}
