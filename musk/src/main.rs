use std::fs::File;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use actix::prelude::*;
use actix_web::{web, App, Error, HttpRequest, HttpResponse, HttpServer};
use actix_web_actors::ws;
use chrono::prelude::*;
use futures::future::TryFlattenStream;
use futures::prelude::*;
use hyper::client::HttpConnector;
use hyper::client::ResponseFuture;
use rand::prelude::*;
use serde::de;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::mpsc::UnboundedSender;
use tokio::{sync::mpsc::unbounded_channel, time};
use twitter_stream::FutureTwitterStream;
use twitter_stream::Token;

#[derive(Deserialize)]
#[serde(untagged)]
enum StreamMessage {
    Tweet(Tweet),
    Other(de::IgnoredAny),
}

struct Tweet {
    entities: Option<Entities>,
    id: u64,
    text: String,
    is_retweet: bool,
}

#[derive(Deserialize)]
struct Entities {
    user_mentions: Vec<UserMention>,
}

#[derive(Deserialize)]
struct UserMention {
    id: u64,
}

#[derive(Deserialize)]
#[serde(remote = "Token")]
struct TokenDef {
    // The `getter` attribute is required to make the `Deserialize` impl use the `From` conversion,
    // even if we are not deriving `Serialize` here.
    #[serde(getter = "__")]
    consumer_key: String,
    consumer_secret: String,
    access_key: String,
    access_secret: String,
}

impl From<TokenDef> for Token {
    fn from(def: TokenDef) -> Token {
        Token::from_parts(
            def.consumer_key,
            def.consumer_secret,
            def.access_key,
            def.access_secret,
        )
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Bitcoin {
    price: f64,
    time: i64,
}

impl Bitcoin {
    fn fetch_current_price() -> Bitcoin {
        let mut rng = rand::thread_rng();
        let y: f64 = rng.gen();
        Bitcoin {
            price: y * 50000.0,
            time: Utc::now().timestamp_nanos(),
        }
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let (s, r) = unbounded_channel();

    let mut credential_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    credential_path.push("credential.json");

    let conn = HttpConnector::new();
    let mut client = hyper::Client::builder().build::<_, hyper::Body>(conn);

    let credential = File::open(credential_path).unwrap();
    let token =
        TokenDef::deserialize(&mut serde_json::Deserializer::from_reader(credential)).unwrap();

    let stream = twitter_stream::Builder::new(token.as_ref())
        .track("@Elon Musk")
        .listen_with_client(&mut client)
        .try_flatten_stream();

    tokio::spawn(handle_twitter_stream(stream, s));
    let r = Arc::new(Mutex::new(r));

    HttpServer::new(move || {
        App::new()
            .data(r.clone())
            .route("/ws/", web::get().to(index))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}

// The custom `Deserialize` impl is needed to handle Tweets with >140 characters.
// Once the Labs API is available, this impl should be unnecessary.
impl<'de> Deserialize<'de> for Tweet {
    fn deserialize<D: de::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Prototype {
            entities: Option<Entities>,
            id: u64,
            // Add the following attribute if you want to deserialize REST API responses too.
            // #[serde(alias = "full_text")]
            text: String,
            extended_tweet: Option<ExtendedTweet>,
            retweeted_status: Option<de::IgnoredAny>,
        }

        #[derive(Deserialize)]
        struct ExtendedTweet {
            full_text: String,
            entities: Option<Entities>,
        }

        Prototype::deserialize(d).map(|p| {
            let (text, entities) = p
                .extended_tweet
                .map_or((p.text, p.entities), |e| (e.full_text, e.entities));
            Tweet {
                entities,
                id: p.id,
                text,
                is_retweet: p.retweeted_status.is_some(),
            }
        })
    }
}

async fn handle_twitter_stream(
    mut stream: TryFlattenStream<FutureTwitterStream<ResponseFuture>>,
    sender: UnboundedSender<Bitcoin>,
) {
    if let Some(json) = stream.next().await {
        if let Ok(StreamMessage::Tweet(_)) = serde_json::from_str(&json.unwrap()) {
            tokio::spawn(async move {
                let mut interval = time::interval(time::Duration::from_secs(30));
                loop {
                    interval.tick().await;
                    sender.send(Bitcoin::fetch_current_price()).unwrap();
                }
            });
        }
    }
}

#[derive(Debug)]
struct MyWs {
    r: Arc<Mutex<UnboundedReceiver<Bitcoin>>>,
}

impl Actor for MyWs {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let r = self.r.clone();
        ctx.add_stream(async move { r.lock().unwrap().recv().await.unwrap() }.into_stream());
    }
}

impl StreamHandler<Bitcoin> for MyWs {
    fn handle(&mut self, item: Bitcoin, ctx: &mut Self::Context) {
        let bitcoin = serde_json::to_string(&item).unwrap();
        ctx.text(bitcoin)
    }
}

/// Handler for ws::Message message
impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for MyWs {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match msg {
            Ok(ws::Message::Ping(msg)) => ctx.pong(&msg),
            Err(_) => ctx.stop(),
            _ => (),
        }
    }
}

async fn index(
    req: HttpRequest,
    stream: web::Payload,
    r: web::Data<Arc<Mutex<UnboundedReceiver<Bitcoin>>>>,
) -> Result<HttpResponse, Error> {
    let resp = ws::start(
        MyWs {
            r: r.get_ref().to_owned(),
        },
        &req,
        stream,
    );
    resp
}
