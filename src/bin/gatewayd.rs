use std::net::SocketAddr;
use std::sync::Arc;

extern crate clap;
use async_executor::Executor;
use easy_parallel::Parallel;

use drk::Result;

use drk::service::{GatewayService, ProgramOptions};

fn setup_addr(address: Option<SocketAddr>, default: SocketAddr) -> SocketAddr {
    match address {
        Some(addr) => addr,
        None => default,
    }
}

async fn start(executor: Arc<Executor<'_>>, options: ProgramOptions) -> Result<()> {
    let accept_addr: SocketAddr = setup_addr(options.accept_addr, "127.0.0.1:3333".parse()?);
    let pub_addr: SocketAddr = setup_addr(options.pub_addr, "127.0.0.1:4444".parse()?);
    let slabstore_path = options.slabstore_path.as_path();

    let gateway = GatewayService::new(accept_addr, pub_addr, slabstore_path)?;

    gateway.start(executor.clone()).await?;
    Ok(())
}

fn main() -> Result<()> {
    use simplelog::*;

    let ex = Arc::new(Executor::new());
    let (signal, shutdown) = async_channel::unbounded::<()>();

    let options = ProgramOptions::load()?;

    let logger_config = ConfigBuilder::new().set_time_format_str("%T%.6f").build();

    let debug_level = if options.verbose {
        LevelFilter::Debug
    } else {
        LevelFilter::Off
    };

    CombinedLogger::init(vec![
        TermLogger::new(debug_level, logger_config, TerminalMode::Mixed).unwrap(),
        WriteLogger::new(
            LevelFilter::Debug,
            Config::default(),
            std::fs::File::create(options.log_path.as_path()).unwrap(),
        ),
    ])
    .unwrap();

    let ex2 = ex.clone();

    let (_, result) = Parallel::new()
        // Run four executor threads.
        .each(0..3, |_| smol::future::block_on(ex.run(shutdown.recv())))
        // Run the main future on the current thread.
        .finish(|| {
            smol::future::block_on(async move {
                start(ex2, options).await?;
                drop(signal);
                Ok::<(), drk::Error>(())
            })
        });

    result
}