//! A CLI tool for executing BigML jobs in parallel.

use bigml::{
    self,
    resource::{execution, Execution, Id, Resource, Script},
    try_wait,
    wait::{wait, BackoffType, WaitOptions, WaitStatus},
    Client,
};
use common_failures::{quick_main, Result};
use env_logger;
use failure::{Error, ResultExt};
use futures::{self, stream, FutureExt, StreamExt, TryStreamExt};
use log::debug;
use std::{env, sync::Arc, time::Duration};
use structopt::StructOpt;
use tokio::{io, runtime::Runtime};
use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec};

mod execution_input;
mod line_delimited_json_codec;

use execution_input::ExecutionInput;
use line_delimited_json_codec::LineDelimitedJsonCodec;

/// Our standard stream type, containing values of type `T`.
type BoxStream<T> = futures::stream::BoxStream<'static, Result<T>>;

/// Our standard future type, yield a value of type `T`.
type BoxFuture<T> = futures::future::BoxFuture<'static, Result<T>>;

/// Our command-line arguments.
#[derive(Debug, StructOpt)]
#[structopt(
    name = "bigml-parallel",
    about = "Execute WhizzML script in parallel over one or more BigML resources"
)]
struct Opt {
    /// The WhizzML script ID to run.
    #[structopt(long = "script", short = "s")]
    script: Id<Script>,

    /// The name to use for our execution objects.
    #[structopt(long = "name", short = "n")]
    name: Option<String>,

    /// The resource IDs to process. (Alternatively, pipe resource IDs on standard
    /// input, one per line.)
    #[structopt(long = "resource", short = "r")]
    resources: Vec<String>,

    /// The input name used to pass the dataset.
    #[structopt(
        long = "resource-input-name",
        short = "R",
        default_value = "resource"
    )]
    resource_input_name: String,

    /// Extra inputs to our WhizzML script, specified as "name=value". These
    /// will be parsed as JSON if possible, or treated as strings otherwise.
    #[structopt(long = "input", short = "i")]
    inputs: Vec<ExecutionInput>,

    /// Expected outputs to our WhizzML script, specified as "name".
    #[structopt(long = "output", short = "o")]
    outputs: Vec<String>,

    /// How many BigML tasks should we use at a time?
    #[structopt(long = "max-tasks", short = "J", default_value = "2")]
    max_tasks: usize,

    /// Apply a tag to the BigML resources we create.
    #[structopt(long = "tag")]
    tags: Vec<String>,
}

// Generate a `main` function that prints out pretty errors.
quick_main!(run);

/// Our real `main` function, called by the code generated by `quick_main!`.
fn run() -> Result<()> {
    env_logger::init();
    let opt = Opt::from_args();
    debug!("command-line options: {:?}", opt);

    // Create a future for our async code, and pass it to an async runtime.
    let fut = run_async(opt);
    let mut runtime = Runtime::new().expect("Unable to create a runtime");
    runtime.block_on(fut.boxed())?;
    Ok(())
}

/// And finally, a third `main` function, but this time asynchronous. This runs
/// the actual BigML script executions using the configuration in `opt`.
async fn run_async(opt: Opt) -> Result<()> {
    // We want to represent our input resource IDs as an asynchronous stream,
    // which will make it very easy to have controlled parallel execution.
    let resources: BoxStream<String> = if !opt.resources.is_empty() {
        // Turn our `--resource` arguments into a stream.
        let resources = opt.resources.clone();
        stream::iter(resources.into_iter().map(Ok)).boxed()
    } else {
        // Parse standard input as a stream of dataset IDs.
        let lines = FramedRead::new(io::stdin(), LinesCodec::new());
        lines.map_err(|e| -> Error { e.into() }).boxed()
    };

    // Wrap our command line arguments in a thread-safe reference counter, so
    // that all our parallel tasks can access them.
    let opt = Arc::new(opt);

    // Transform our stream of IDs into a stream of _futures_, each of which will
    // return an `Execution` object from BigML.
    let opt2 = opt.clone();
    let execution_futures: BoxStream<BoxFuture<Execution>> = resources
        .map_ok(move |resource| {
            resource_id_to_execution(opt2.clone(), resource).boxed()
        })
        .boxed();

    // Now turn the stream of futures into a stream of executions, using
    // `buffer_unordered` to execute up to `opt.max_tasks` in parallel. This is
    // basically the "payoff" for all the async code up above, and it is
    // wonderful.
    //
    // TODO: In tokio 0.1, this had weird buffering behavior, and
    // appeared to wait until it buffered `opt.max_tasks` items. I have
    // not verified this in tokio 0.2.
    let executions: BoxStream<Execution> = execution_futures
        .try_buffer_unordered(opt.max_tasks)
        .boxed();

    // Copy our stream of `Execution`s to standard output as line-delimited
    // JSON.
    //
    // TODO: `forward` may also have weird buffering behavior.
    let stdout = FramedWrite::new(io::stdout(), LineDelimitedJsonCodec::new());
    executions.forward(stdout).await?;
    Ok(())
}

/// Use our command-line options and a resource ID to create and run a BigML
/// execution.
async fn resource_id_to_execution(
    opt: Arc<Opt>,
    resource: String,
) -> Result<Execution> {
    debug!("running {} on {}", opt.script, resource);

    // Specify what script to run.
    let mut args = execution::Args::default();
    args.script = Some(opt.script.clone());

    // Optionally set the script name.
    if let Some(name) = opt.name.as_ref() {
        args.name = Some(name.to_owned());
    }

    // Specify the input dataset.
    args.add_input(&opt.resource_input_name, &resource)?;

    // Add any other inputs.
    for input in &opt.inputs {
        args.add_input(&input.name, &input.value)?;
    }

    // Add outputs.
    for output in &opt.outputs {
        args.add_output(output);
    }

    // Add tags.
    args.tags = opt.tags.clone();

    // Execute our script, retrying the creation of the execution if needed.
    let client = new_client()?;
    let opt = WaitOptions::default()
        .retry_interval(Duration::from_secs(60))
        .backoff_type(BackoffType::Exponential)
        .allowed_errors(6)
        .timeout(Duration::from_secs(2 * 60 * 60));
    let mut execution = wait(&opt, || {
        async {
            // We use `try_wait`, because it knows which errors are permanent
            // and which are temporary.
            WaitStatus::Finished(try_wait!(client.create(&args).await))
        }
    })
    .await?;
    // This has its own retry logic, so we don't wrap it above.
    execution = client.wait(&execution.id()).await?;
    debug!("finished {} on {}", execution.id(), resource);
    Ok(execution)
}

/// Create a BigML client using environment varaibles to authenticate.
fn new_client() -> Result<Client> {
    let username =
        env::var("BIGML_USERNAME").context("must specify BIGML_USERNAME")?;
    let api_key = env::var("BIGML_API_KEY").context("must specify BIGML_API_KEY")?;
    Ok(Client::new(username, api_key)?)
}
