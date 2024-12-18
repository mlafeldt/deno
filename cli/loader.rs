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

use std::io::Read as _;
use std::sync::Arc;

use crate::args::Flags;
use crate::factory::CliFactory;
use crate::util::display;

use deno_core::error::AnyError;
use deno_runtime::deno_node::NodeExtInitServices;
use deno_runtime::fmt_errors::format_js_error;
use deno_runtime::tokio_util::create_and_run_current_thread_with_maybe_metrics;
use deno_runtime::worker::{MainWorker, WorkerOptions, WorkerServiceOptions};
pub use deno_runtime::UNSTABLE_GRANULAR_FLAGS;
use deno_runtime::{BootstrapOptions, WorkerExecutionMode};
use deno_terminal::colors;
use util::checksum;
use worker::{
  create_isolate_create_params, create_web_worker_callback,
  get_cache_storage_dir, CreateModuleLoaderResult, SharedWorkerState,
};

fn main() {
  util::unix::raise_fd_limit();
  util::logger::init(None);
  deno_core::JsRuntime::init_platform(None, false);

  // let args: Vec<_> = env::args_os().collect();

  let future = async move {
    // let flags = flags_from_vec(args)?;
    let mut flags = Flags::default();
    flags.permissions.allow_all = true;
    flags.code_cache_enabled = true;
    // dbg!(&flags);

    run_from_stdin(flags.into()).await
  };

  create_and_run_current_thread_with_maybe_metrics(future).unwrap();
  // let rt = tokio::runtime::Builder::new_current_thread()
  //   .enable_all()
  //   .build()
  //   .unwrap();
  // rt.block_on(future).unwrap();
}

async fn run_from_stdin(flags: Arc<Flags>) -> Result<i32, AnyError> {
  // tools::run::run_script(WorkerExecutionMode::Run, flags.clone(), None).await

  let mut source = Vec::new();
  std::io::stdin().read_to_end(&mut source)?;

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
  file_fetcher.insert_memory_files(file_fetcher::File {
    specifier: main_module.clone(),
    maybe_headers: None,
    source: source.into(),
  });

  let worker_factory = cli_factory.create_cli_main_worker_factory().await?;

  // let mut worker = worker_factory
  //   .create_main_worker(WorkerExecutionMode::Run, main_module.clone())
  //   .await?;

  let permissions = cli_factory.root_permissions_container()?.clone();

  // create_custom_worker
  let shared = &worker_factory.shared;
  let stdio = deno_runtime::deno_io::Stdio::default();

  let CreateModuleLoaderResult {
    module_loader,
    node_require_loader,
  } = shared
    .module_loader_factory
    .create_for_main(permissions.clone());

  let create_web_worker_cb =
    create_web_worker_callback(shared.clone(), stdio.clone());

  let maybe_storage_key = shared
    .storage_key_resolver
    .resolve_storage_key(&main_module);
  let origin_storage_dir = maybe_storage_key.as_ref().map(|key| {
    shared
      .options
      .origin_data_folder_path
      .as_ref()
      .unwrap() // must be set if storage key resolver returns a value
      .join(checksum::gen(&[key.as_bytes()]))
  });
  let cache_storage_dir = maybe_storage_key
    .map(|key| get_cache_storage_dir().join(checksum::gen(&[key.as_bytes()])));

  let feature_checker = shared.feature_checker.clone();
  let mut unstable_features =
    Vec::with_capacity(crate::UNSTABLE_GRANULAR_FLAGS.len());
  for granular_flag in crate::UNSTABLE_GRANULAR_FLAGS {
    if feature_checker.check(granular_flag.name) {
      unstable_features.push(granular_flag.id);
    }
  }

  let services = WorkerServiceOptions {
    root_cert_store_provider: Some(shared.root_cert_store_provider.clone()),
    module_loader,
    fs: shared.fs.clone(),
    node_services: Some(shared.create_node_init_services(node_require_loader)),
    npm_process_state_provider: Some(shared.npm_process_state_provider()),
    blob_store: shared.blob_store.clone(),
    broadcast_channel: shared.broadcast_channel.clone(),
    fetch_dns_resolver: Default::default(),
    shared_array_buffer_store: Some(shared.shared_array_buffer_store.clone()),
    compiled_wasm_module_store: Some(shared.compiled_wasm_module_store.clone()),
    feature_checker,
    permissions,
    v8_code_cache: shared.code_cache.clone().map(|c| c.as_code_cache()),
  };

  let options = WorkerOptions {
    bootstrap: BootstrapOptions {
      deno_version: crate::version::DENO_VERSION_INFO.deno.to_string(),
      args: shared.options.argv.clone(),
      cpu_count: std::thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(1),
      log_level: shared.options.log_level,
      enable_op_summary_metrics: shared.options.enable_op_summary_metrics,
      enable_testing_features: shared.options.enable_testing_features,
      locale: deno_core::v8::icu::get_language_tag(),
      location: shared.options.location.clone(),
      no_color: !colors::use_color(),
      is_stdout_tty: deno_terminal::is_stdout_tty(),
      is_stderr_tty: deno_terminal::is_stderr_tty(),
      color_level: colors::get_color_level(),
      unstable_features,
      user_agent: version::DENO_VERSION_INFO.user_agent.to_string(),
      inspect: shared.options.is_inspecting,
      has_node_modules_dir: shared.options.has_node_modules_dir,
      argv0: shared.options.argv0.clone(),
      node_debug: shared.options.node_debug.clone(),
      node_ipc_fd: shared.options.node_ipc,
      mode: WorkerExecutionMode::Run,
      serve_port: shared.options.serve_port,
      serve_host: shared.options.serve_host.clone(),
      otel_config: shared.otel_config.clone(),
    },
    extensions: vec![], // TODO
    startup_snapshot: crate::js::deno_isolate_init(),
    create_params: create_isolate_create_params(),
    unsafely_ignore_certificate_errors: shared
      .options
      .unsafely_ignore_certificate_errors
      .clone(),
    seed: shared.options.seed,
    format_js_error_fn: Some(Arc::new(format_js_error)),
    create_web_worker_cb,
    maybe_inspector_server: None,
    should_break_on_first_statement: shared.options.inspect_brk,
    should_wait_for_inspector_session: shared.options.inspect_wait,
    strace_ops: shared.options.strace_ops.clone(),
    get_error_class_fn: Some(&errors::get_error_class_name),
    cache_storage_dir,
    origin_storage_dir,
    stdio,
    skip_op_registration: shared.options.skip_op_registration,
    enable_stack_trace_arg_in_ops: crate::args::has_trace_permissions_enabled(),
  };

  let mut worker =
    MainWorker::bootstrap_from_options(main_module.clone(), services, options);

  worker.execute_main_module(&main_module).await?;
  worker.run_event_loop(false).await?;
  Ok(worker.exit_code())

  // let mut worker = worker_factory
  //   .create_custom_worker(
  //     WorkerExecutionMode::Run,
  //     main_module.clone(),
  //     cli_factory.root_permissions_container()?.clone(),
  //     vec![],
  //     Default::default(),
  //   )
  //   .await?;

  // let exit_code = worker.run().await?;
  // Ok(exit_code)
}

pub(crate) fn unstable_exit_cb(feature: &str, api_name: &str) {
  log::error!(
    "Unstable API '{api_name}'. The `--unstable-{}` flag must be provided.",
    feature
  );
  deno_runtime::exit(70);
}
