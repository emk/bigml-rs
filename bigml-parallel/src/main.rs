//! A CLI tool for executing BigML jobs in parallel.

#![feature(async_await)]

use bigml::{
    resource::{execution, Execution, Id, Resource, Script},
    Client,
};
use bytes::Bytes;
use common_failures::{quick_main, Result};
use env_logger;
use failure::{format_err, Error, ResultExt};
use futures::{compat::Future01CompatExt, Future, FutureExt, TryFutureExt};
use log::{debug, warn};
use serde_json::Value;
use std::{env, pin::Pin, str::FromStr, sync::Arc};
use structopt::StructOpt;
use tokio::{
    codec::{BytesCodec, FramedRead, FramedWrite, LinesCodec},
    io,
    prelude::{stream, Stream},
    runtime::Runtime,
};

/// Our standard stream type, containing values of type `T`.
type BoxStream<T> = Box<dyn Stream<Item = T, Error = Error> + Send + 'static>;

/// Our standard future type, yield a value of type `T`.
type BoxFuture<T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'static>>;

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

    /// Extra inputs to our WhizzML script, passed as "name=value". These
    /// will be parsed as JSON if possible, or treated as strings otherwise.
    #[structopt(long = "input", short = "i")]
    inputs: Vec<Input>,

    /// Expected outputs to our WhizzML script, passed as "name".
    #[structopt(long = "output", short = "o")]
    outputs: Vec<String>,

    /// How many BigML tasks should we use at a time? We default to 2, because
    /// that's what the free plan currently advertises.
    #[structopt(long = "max-tasks", short = "J", default_value = "2")]
    max_tasks: usize,
}

/// An input argument.
#[derive(Debug)]
struct Input {
    /// The name of this input.
    name: String,

    /// The JSON value of this input.
    value: Value,
}

/// Declare a `FromStr` implementation for `Input` so that `structopt` can parse
/// command-line arguments directly into `Input` values.
impl FromStr for Input {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let split = s.splitn(2, '=').collect::<Vec<&str>>();
        if split.len() != 2 {
            return Err(format_err!("input {:?} must have form \"key=value\"", s,));
        }
        let name = split[0].to_owned();
        let value = match serde_json::from_str(split[1]) {
            Ok(value) => value,
            Err(err) => {
                warn!(
                    "could not parse input {:?} as JSON (treating as string): {}",
                    s, err,
                );
                Value::String(split[1].to_owned())
            }
        };
        Ok(Input { name, value })
    }
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
    runtime.block_on(fut.boxed().compat())?;
    Ok(())
}

/// And finally, a third `main` function, but this time asynchronous. This runs
/// the actual BigML script executions using the configuration in `opt`.
async fn run_async(opt: Opt) -> Result<()> {
    // We want to represent our input dataset IDs as an asynchronous stream,
    // which will make it very easy to have controlled parallel execution.
    let dataset_ids: BoxStream<String> = if !opt.resources.is_empty() {
        // Turn our `--dataset` arguments into a stream.
        let datasets = opt.resources.clone();
        Box::new(stream::iter_ok(datasets.into_iter()))
    } else {
        // Parse standard input as a stream of dataset IDs.
        let lines = FramedRead::new(io::stdin(), LinesCodec::new());
        Box::new(lines.map_err(|e| -> Error { e.into() }))
    };

    // Wrap our command line arguments in a thread-safe reference counter, so
    // that all our parallel tasks can access them.
    let opt = Arc::new(opt);

    // Transform our stream of IDs into a stream of _futures_, each of which will
    // return an `Execution` object from BigML.
    let opt2 = opt.clone();
    let execution_futures: BoxStream<BoxFuture<Execution>> =
        Box::new(dataset_ids.map(move |resource| {
            resource_id_to_execution(opt2.clone(), resource).boxed()
        }));

    // Now turn the stream of futures into a stream of executions, using
    // `buffer_unordered` to execute up to `opt.max_tasks` in parallel. This is
    // basically the "payoff" for all the async code up above, and it is
    // wonderful.
    let executions: BoxStream<Execution> = Box::new(
        execution_futures
            // Convert back to legacy `tokio::Future` for `buffered_unordered`.
            .map(|fut| fut.compat())
            .buffer_unordered(opt.max_tasks),
    );

    // Convert our `Execution` objects to raw JSON data, with one JSON value per
    // line.
    let jsons: BoxStream<Bytes> = Box::new(executions.and_then(|execution| {
        let mut bytes = serde_json::to_vec(&execution)?;
        bytes.push(b'\n');
        Ok(Bytes::from(bytes))
    }));

    // Dump our stream of raw JSON chunks to standard output. We used
    // `FramedWrite`, because that's the idiomatic way of dumping a `tokio`
    // stream of values to an `AsyncWrite`.
    //
    // TODO: Make sure this flushes after each chunk!
    let stdout_sink = FramedWrite::new(io::stdout(), BytesCodec::new());
    jsons.forward(stdout_sink).compat().await?;
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
    args.add_input(&opt.resource_input_name, resource)?;

    // Add any other inputs.
    for input in &opt.inputs {
        args.add_input(&input.name, &input.value)?;
    }

    // Add outputs.
    for output in &opt.outputs {
        args.add_output(output);
    }

    // Execute our script, retrying if needed.
    //
    // TODO: Add retry logic? Do we need it?
    let client = new_client()?;
    let mut execution = client.create(&args).await?;
    execution = client.wait(&execution.id()).await?;
    Ok(execution)
}

/// Create a BigML client using environment varaibles to authenticate.
fn new_client() -> Result<Client> {
    let username =
        env::var("BIGML_USERNAME").context("must specify BIGML_USERNAME")?;
    let api_key = env::var("BIGML_API_KEY").context("must specify BIGML_API_KEY")?;
    Ok(Client::new(username, api_key)?)
}
