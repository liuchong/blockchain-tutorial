use dotenv::dotenv;
use futures::{future, Future, Stream};
use hex::encode as hex_encode;
use hyper::service::service_fn;
use hyper::{header, Body, Method, Request, Response, Server, StatusCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::env;
use std::net::{SocketAddr, ToSocketAddrs};
use std::process::exit;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Default, Serialize)]
struct Block {
    index: u64,
    timestamp: String,
    bpm: u64,
    hash: String,
    prev_hash: String,
}

#[derive(Debug)]
struct Blockchain(Vec<Block>);

#[derive(Deserialize)]
struct Message {
    bpm: u64,
}

type ResponseFuture = Box<Future<Item = Response<Body>, Error = hyper::Error> + Send>;

static NOTFOUND: &[u8] = b"Not Found";

fn calculate_hash(block: &Block) -> String {
    let record =
        block.index.to_string() + &block.timestamp + &block.bpm.to_string() + &block.prev_hash;

    let mut hasher = Sha256::new();
    hasher.input(record);

    hex_encode(hasher.result().as_slice())
}

fn generate_block(old_block: &Block, bpm: u64) -> Block {
    let mut new_block = Block::default();

    use chrono::prelude::*;
    let t = Utc::now();

    new_block.index = old_block.index + 1;
    new_block.timestamp = t.to_string();
    new_block.bpm = bpm;
    new_block.prev_hash = old_block.hash.to_owned();
    new_block.hash = calculate_hash(&new_block);

    new_block
}

fn init_blockchain() -> Arc<Mutex<Blockchain>> {
    let mut init_block = Block::default();

    use chrono::prelude::*;
    let t = Utc::now();

    init_block.index = 0;
    init_block.timestamp = t.to_string();
    init_block.bpm = 0;
    init_block.prev_hash = "".to_string();
    init_block.hash = calculate_hash(&init_block);

    println!("{:?}", init_block);

    Arc::new(Mutex::new(Blockchain(vec![init_block])))
}

fn is_block_valid(new_block: &Block, old_block: &Block) -> bool {
    if old_block.index + 1 != new_block.index {
        return false;
    }

    if old_block.hash != new_block.prev_hash {
        return false;
    }

    if calculate_hash(new_block) != new_block.hash {
        return false;
    }

    true
}

fn handle_get_blockchain(block_chain: Arc<Mutex<Blockchain>>) -> ResponseFuture {
    Box::new(future::ok(respond_with_json(
        &block_chain.lock().unwrap().0,
    )))
}

fn handle_post_blockchain(
    req: Request<Body>,
    block_chain: Arc<Mutex<Blockchain>>,
) -> ResponseFuture {
    let res = req.into_body().concat2().map(move |chunk| {
        let msg: Message = serde_json::from_slice(&chunk.into_bytes()).unwrap();

        let mut block_chain = block_chain.lock().unwrap();
        let old_block = &block_chain.0[block_chain.0.len() - 1];
        let new_block = generate_block(old_block, msg.bpm);

        if is_block_valid(&new_block, old_block) {
            println!("{:?}", &new_block);
            (*block_chain).0.push(new_block.clone());
        }

        respond_with_json(&new_block)
    });

    Box::new(res)
}

fn respond_with_json<T: Serialize>(obj: &T) -> Response<Body> {
    match serde_json::to_string_pretty(obj) {
        Ok(json) => Response::builder()
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json))
            .unwrap(),
        Err(e) => {
            eprintln!("serializing json: {}", e);
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("Internal Server Error"))
                .unwrap()
        }
    }
}

fn router(req: Request<Body>, block_chain: &Arc<Mutex<Blockchain>>) -> ResponseFuture {
    Box::new(match *req.method() {
        Method::GET => handle_get_blockchain(block_chain.clone()),
        Method::POST => handle_post_blockchain(req, block_chain.clone()),
        _ => {
            let body = Body::from(NOTFOUND);
            Box::new(future::ok(
                Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(body)
                    .unwrap(),
            ))
        }
    })
}

fn run(addr: SocketAddr) {
    let block_chain = init_blockchain();

    hyper::rt::run(future::lazy(move || {
        let new_service = move || {
            let block_chain = block_chain.clone();
            service_fn(move |req| router(req, &block_chain))
        };

        let server = Server::bind(&addr)
            .serve(new_service)
            .map_err(|e| eprintln!("server error: {}", e));

        println!("Listening on http://{}", addr);

        server
    }));
}

fn main() {
    dotenv().ok();

    let host = env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_owned());
    let port = env::var("PORT").unwrap_or_else(|_| "8080".to_owned());
    let addr = format!("{}:{}", host, port)
        .to_socket_addrs()
        .unwrap_or_else(|_| {
            eprintln!("Failed to parse socket addr from {}:{}", host, port);
            exit(1);
        })
        .next()
        .unwrap_or_else(|| {
            eprintln!("Failed to get socket addr");
            exit(1);
        });

    run(addr);
}
