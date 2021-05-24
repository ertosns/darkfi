use std::io;
use std::net::SocketAddr;

use crate::serial::{deserialize, serialize};
use crate::{Decodable, Encodable, Result};

use bytes::Bytes;
use futures::FutureExt;
use rand::Rng;
use zeromq::*;

enum NetEvent {
    Receive(zeromq::ZmqMessage),
    Send(Reply),
}

pub fn addr_to_string(addr: SocketAddr) -> String {
    format!("tcp://{}", addr.to_string())
}

pub struct RepProtocol {
    addr: SocketAddr,
    socket: zeromq::RepSocket,
    recv_queue: async_channel::Receiver<Reply>,
    send_queue: async_channel::Sender<Request>,
    channels: (
        async_channel::Sender<Reply>,
        async_channel::Receiver<Request>,
    ),
}

impl RepProtocol {
    pub fn new(addr: SocketAddr) -> RepProtocol {
        let socket = zeromq::RepSocket::new();
        let (send_queue, recv_channel) = async_channel::unbounded::<Request>();
        let (send_channel, recv_queue) = async_channel::unbounded::<Reply>();

        let channels = (send_channel.clone(), recv_channel.clone());

        RepProtocol {
            addr,
            socket,
            recv_queue,
            send_queue,
            channels,
        }
    }

    pub async fn start(
        &mut self,
    ) -> Result<(
        async_channel::Sender<Reply>,
        async_channel::Receiver<Request>,
    )> {
        let addr = addr_to_string(self.addr);
        self.socket.bind(addr.as_str()).await?;
        Ok(self.channels.clone())
    }

    pub async fn run(&mut self) -> Result<()> {
        loop {
            let event = futures::select! {
                request = self.socket.recv().fuse() => NetEvent::Receive(request?),
                reply = self.recv_queue.recv().fuse() => NetEvent::Send(reply?)
            };

            match event {
                NetEvent::Receive(request) => {
                    let request: &Bytes = request.get(0).unwrap();
                    let request: Vec<u8> = request.to_vec();
                    let req: Request = deserialize(&request)?;
                    self.send_queue.send(req).await?;
                }
                NetEvent::Send(reply) => {
                    let reply: Vec<u8> = serialize(&reply);
                    let reply = Bytes::from(reply);
                    self.socket.send(reply.into()).await?;
                }
            }
        }
    }
}

pub struct ReqProtocol {
    addr: SocketAddr,
    socket: zeromq::ReqSocket,
}

impl ReqProtocol {
    pub fn new(addr: SocketAddr) -> ReqProtocol {
        let socket = zeromq::ReqSocket::new();
        ReqProtocol { addr, socket }
    }

    pub async fn start(&mut self) -> Result<()> {
        let addr = addr_to_string(self.addr);
        self.socket.connect(addr.as_str()).await?;
        Ok(())
    }

    pub async fn request(&mut self, command: u8, data: Vec<u8>) -> Result<Vec<u8>> {
        let request = Request::new(command, data);
        let req = serialize(&request);
        let req = bytes::Bytes::from(req);

        self.socket.send(req.into()).await?;

        let rep: zeromq::ZmqMessage = self.socket.recv().await?;
        let rep: &Bytes = rep.get(0).unwrap();
        let rep: Vec<u8> = rep.to_vec();

        let reply: Reply = deserialize(&rep)?;

        if reply.has_error() {
            return Err(crate::Error::ServicesError("response has an error"));
        }

        assert!(reply.get_id() == request.get_id());

        Ok(reply.get_payload())
    }
}

pub struct Publisher {
    addr: SocketAddr,
    socket: zeromq::PubSocket,
}

impl Publisher {
    pub fn new(addr: SocketAddr) -> Publisher {
        let socket = zeromq::PubSocket::new();
        Publisher { addr, socket }
    }
    pub async fn start(&mut self) -> Result<()> {
        let addr = addr_to_string(self.addr);
        self.socket.bind(addr.as_str()).await?;
        Ok(())
    }

    pub async fn publish(&mut self, data: Vec<u8>) -> Result<()> {
        let data = Bytes::from(data);
        self.socket.send(data.into()).await?;
        Ok(())
    }
}

pub struct Subscriber {
    addr: SocketAddr,
    socket: zeromq::SubSocket,
}

impl Subscriber {
    pub fn new(addr: SocketAddr) -> Subscriber {
        let socket = zeromq::SubSocket::new();
        Subscriber { addr, socket }
    }

    pub async fn start(&mut self) -> Result<()> {
        let addr = addr_to_string(self.addr);
        self.socket.connect(addr.as_str()).await?;

        self.socket.subscribe("").await?;

        Ok(())
    }

    pub async fn fetch(&mut self) -> Result<Vec<u8>> {
        let data = self.socket.recv().await?;
        let data: &Bytes = data.get(0).unwrap();
        let data = data.to_vec();
        Ok(data)
    }
}

#[derive(Debug, PartialEq)]
pub struct Request {
    command: u8,
    id: u32,
    payload: Vec<u8>,
}

impl Request {
    pub fn new(command: u8, payload: Vec<u8>) -> Request {
        let id = Self::gen_id();
        Request {
            command,
            id,
            payload,
        }
    }
    fn gen_id() -> u32 {
        let mut rng = rand::thread_rng();
        rng.gen()
    }

    pub fn get_id(&self) -> u32 {
        self.id
    }

    pub fn get_command(&self) -> u8 {
        self.command
    }

    pub fn get_payload(&self) -> Vec<u8> {
        self.payload.clone()
    }
}

#[derive(Debug, PartialEq)]
pub struct Reply {
    id: u32,
    error: u32,
    payload: Vec<u8>,
}

impl Reply {
    pub fn from(request: &Request, error: u32, payload: Vec<u8>) -> Reply {
        Reply {
            id: request.get_id(),
            error,
            payload,
        }
    }

    pub fn has_error(&self) -> bool {
        if self.error == 0 {
            false
        } else {
            true
        }
    }

    pub fn get_payload(&self) -> Vec<u8> {
        self.payload.clone()
    }

    pub fn get_id(&self) -> u32 {
        self.id
    }
}

impl Encodable for Request {
    fn encode<S: io::Write>(&self, mut s: S) -> Result<usize> {
        let mut len = 0;
        len += self.command.encode(&mut s)?;
        len += self.id.encode(&mut s)?;
        len += self.payload.encode(&mut s)?;
        Ok(len)
    }
}

impl Encodable for Reply {
    fn encode<S: io::Write>(&self, mut s: S) -> Result<usize> {
        let mut len = 0;
        len += self.id.encode(&mut s)?;
        len += self.error.encode(&mut s)?;
        len += self.payload.encode(&mut s)?;
        Ok(len)
    }
}

impl Decodable for Request {
    fn decode<D: io::Read>(mut d: D) -> Result<Self> {
        Ok(Self {
            command: Decodable::decode(&mut d)?,
            id: Decodable::decode(&mut d)?,
            payload: Decodable::decode(&mut d)?,
        })
    }
}

impl Decodable for Reply {
    fn decode<D: io::Read>(mut d: D) -> Result<Self> {
        Ok(Self {
            id: Decodable::decode(&mut d)?,
            error: Decodable::decode(&mut d)?,
            payload: Decodable::decode(&mut d)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Reply, Request, Result};
    use crate::serial::{deserialize, serialize};

    #[test]
    fn serialize_and_deserialize_request_test() {
        let request = Request::new(2, vec![2, 3, 4, 6, 4]);
        let serialized_request = serialize(&request);
        assert!((deserialize(&serialized_request) as Result<bool>).is_err());
        let deserialized_request = deserialize(&serialized_request).ok();
        assert_eq!(deserialized_request, Some(request));
    }

    #[test]
    fn serialize_and_deserialize_reply_test() {
        let request = Request::new(2, vec![2, 3, 4, 6, 4]);
        let reply = Reply::from(&request, 0, vec![2, 3, 4, 6, 4]);
        let serialized_reply = serialize(&reply);
        assert!((deserialize(&serialized_reply) as Result<bool>).is_err());
        let deserialized_reply = deserialize(&serialized_reply).ok();
        assert_eq!(deserialized_reply, Some(reply));
    }
}
