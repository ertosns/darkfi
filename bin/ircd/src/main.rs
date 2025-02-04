#[macro_use]
extern crate clap;
use async_executor::Executor;
use async_std::io::BufReader;
use async_trait::async_trait;
use futures::{AsyncBufReadExt, AsyncReadExt, FutureExt};
use log::{debug, error, info, warn};
use serde_json::{json, Value};
use simplelog::{ColorChoice, LevelFilter, TermLogger, TerminalMode};
use smol::Async;
use std::{
    net::{SocketAddr, TcpListener, TcpStream},
    sync::Arc,
};

use darkfi::{
    net,
    rpc::{
        jsonrpc::{
            error as jsonerr, request as jsonreq, response as jsonresp, ErrorCode::*, JsonRequest,
            JsonResult,
        },
        rpcserver::{listen_and_serve, RequestHandler, RpcServerConfig},
    },
    util::{expand_path, sleep},
    Error, Result,
};

mod irc_server;
mod privmsg;
mod program_options;
mod protocol_privmsg;

use crate::{
    irc_server::IrcServerConnection,
    privmsg::{PrivMsg, PrivMsgId, SeenPrivMsgIds, SeenPrivMsgIdsPtr},
    program_options::ProgramOptions,
    protocol_privmsg::ProtocolPrivMsg,
};

async fn process(
    recvr: async_channel::Receiver<Arc<PrivMsg>>,
    stream: Async<TcpStream>,
    peer_addr: SocketAddr,
    p2p: net::P2pPtr,
    seen_privmsg_ids: SeenPrivMsgIdsPtr,
    _executor: Arc<Executor<'_>>,
) -> Result<()> {
    let (reader, writer) = stream.split();

    let mut reader = BufReader::new(reader);
    let mut connection = IrcServerConnection::new(writer, seen_privmsg_ids);

    loop {
        let mut line = String::new();
        futures::select! {
            privmsg = recvr.recv().fuse() => {
                let privmsg = privmsg.expect("internal message queue error");
                debug!("ABOUT TO SEND {:?}", privmsg);
                let irc_msg = format!(
                    ":{}!darkfi@127.0.0.1 PRIVMSG {} :{}\n",
                    privmsg.nickname,
                    privmsg.channel,
                    privmsg.message
                );

                connection.reply(&irc_msg).await?;
            }
            err = reader.read_line(&mut line).fuse() => {
                if let Err(err) = err {
                    warn!("Read line error. Closing stream for {}: {}", peer_addr, err);
                    return Ok(())
                }
                process_user_input(line, peer_addr, &mut connection, p2p.clone()).await?;
            }
        };
    }
}

async fn process_user_input(
    mut line: String,
    peer_addr: SocketAddr,
    connection: &mut IrcServerConnection,
    p2p: net::P2pPtr,
) -> Result<()> {
    if line.len() == 0 {
        warn!("Received empty line from {}. Closing connection.", peer_addr);
        return Err(Error::ChannelStopped)
    }
    assert!(&line[(line.len() - 1)..] == "\n");
    // Remove the \n character
    line.pop();

    debug!("Received '{}' from {}", line, peer_addr);

    if let Err(err) = connection.update(line, p2p.clone()).await {
        warn!("Connection error: {} for {}", err, peer_addr);
        return Err(Error::ChannelStopped)
    }

    Ok(())
}

async fn channel_loop(
    p2p: net::P2pPtr,
    sender: async_channel::Sender<Arc<PrivMsg>>,
    seen_privmsg_ids: SeenPrivMsgIdsPtr,
    executor: Arc<Executor<'_>>,
) -> Result<()> {
    let new_channel_sub = p2p.subscribe_channel().await;

    loop {
        let channel = new_channel_sub.receive().await?;

        let protocol_privmsg =
            ProtocolPrivMsg::new(channel, sender.clone(), seen_privmsg_ids.clone(), p2p.clone())
                .await;

        protocol_privmsg.start(executor.clone()).await;
    }
}

async fn start(executor: Arc<Executor<'_>>, options: ProgramOptions) -> Result<()> {
    let listener = match Async::<TcpListener>::bind(options.irc_accept_addr) {
        Ok(listener) => listener,
        Err(err) => {
            error!("Bind listener failed: {}", err);
            return Err(Error::OperationFailed)
        }
    };
    let local_addr = match listener.get_ref().local_addr() {
        Ok(addr) => addr,
        Err(err) => {
            error!("Failed to get local address: {}", err);
            return Err(Error::OperationFailed)
        }
    };
    info!("Listening on {}", local_addr);

    let server_config = RpcServerConfig {
        socket_addr: local_addr,
        use_tls: false,
        // this is all random filler that is meaningless bc tls is disabled
        // TODO: cleanup
        identity_path: expand_path("../..")?,
        identity_pass: "test".to_string(),
    };

    let seen_privmsg_ids = SeenPrivMsgIds::new();

    let p2p = net::P2p::new(options.network_settings);
    // Performs seed session
    p2p.clone().start(executor.clone()).await?;
    // Actual main p2p session
    let ex2 = executor.clone();
    let p2p2 = p2p.clone();
    executor
        .spawn(async move {
            if let Err(err) = p2p2.run(ex2).await {
                error!("Error: p2p run failed {}", err);
            }
        })
        .detach();

    let (sender, recvr) = async_channel::unbounded();
    // for now the p2p and channel sub sessions just run forever
    // so detach them as background processes.
    executor
        .spawn(channel_loop(p2p.clone(), sender, seen_privmsg_ids.clone(), executor.clone()))
        .detach();

    let ex2 = executor.clone();
    let ex3 = ex2.clone();
    let rpc_interface = Arc::new(JsonRpcInterface {});
    executor
        .spawn(async move { listen_and_serve(server_config, rpc_interface, ex3).await })
        .detach();

    loop {
        let (stream, peer_addr) = match listener.accept().await {
            Ok((s, a)) => (s, a),
            Err(err) => {
                error!("Error listening for connections: {}", err);
                return Err(Error::ServiceStopped)
            }
        };
        info!("Accepted client: {}", peer_addr);

        let p2p2 = p2p.clone();
        let ex2 = executor.clone();
        executor
            .spawn(process(recvr.clone(), stream, peer_addr, p2p2, seen_privmsg_ids.clone(), ex2))
            .detach();
    }
}

struct JsonRpcInterface {}

#[async_trait]
impl RequestHandler for JsonRpcInterface {
    async fn handle_request(&self, req: JsonRequest, _executor: Arc<Executor<'_>>) -> JsonResult {
        if req.params.as_array().is_none() {
            return JsonResult::Err(jsonerr(InvalidParams, None, req.id))
        }

        debug!(target: "RPC", "--> {}", serde_json::to_string(&req).unwrap());

        match req.method.as_str() {
            Some("say_hello") => return self.say_hello(req.id, req.params).await,
            Some(_) | None => return JsonResult::Err(jsonerr(MethodNotFound, None, req.id)),
        }
    }
}

impl JsonRpcInterface {
    // --> {"method": "say_hello", "params": []}
    // <-- {"result": "hello world"}
    async fn say_hello(&self, id: Value, _params: Value) -> JsonResult {
        JsonResult::Resp(jsonresp(json!("hello world"), id))
    }
}

fn main() -> Result<()> {
    TermLogger::init(
        LevelFilter::Debug,
        simplelog::Config::default(),
        TerminalMode::Mixed,
        ColorChoice::Auto,
    )?;

    let options = ProgramOptions::load()?;

    let ex = Arc::new(Executor::new());
    smol::block_on(ex.run(start(ex.clone(), options)))
}
