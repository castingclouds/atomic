use clap::{Parser, ValueHint};

use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server};
use atomic_config::global_config_dir;
use std::convert::Infallible;
use std::net::SocketAddr;
use tokio::select;
use tokio::sync::mpsc::channel;

#[derive(Parser, Debug)]
pub struct Client {
    /// Url to authenticate to.
    #[clap(value_name = "URL", value_hint = ValueHint::Url)]
    url: String,
}

impl Client {
    pub async fn run(self) -> Result<(), anyhow::Error> {
        let url = url::Url::parse(&self.url)?;

        let mut cache_path = None;
        if let Some(mut cached) = global_config_dir() {
            cached.push("cache");
            if let Some(host) = url.host_str() {
                cached.push(host);
                if let Ok(token) = std::fs::read_to_string(&cached) {
                    println!("Bearer {}", token);
                    return Ok(());
                } else {
                    cache_path = Some(cached);
                }
            }
        }

        let (tx, mut rx) = channel::<String>(1);
        let make_service = make_service_fn(|_conn| {
            let tx = tx.clone();
            async move {
                let handle = move |req: Request<_>| {
                    let qq: Option<String> = if let Some(q) = req.uri().query() {
                        let eq = "token=";
                        if q.starts_with(eq) {
                            Some(q.split_at(eq.len()).1.to_string())
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    let tx = tx.clone();
                    async move {
                        if let Some(qq) = qq {
                            tx.send(qq).await.unwrap();
                            let resp = Response::builder()
                                .header("Content-Type", "text/html")
                                .body(Body::from(include_str!("client.html")))
                                .unwrap();
                            Ok::<_, Infallible>(resp)
                        } else {
                            Ok::<_, Infallible>(
                                Response::builder()
                                    .status(404)
                                    .body("Not found".into())
                                    .unwrap(),
                            )
                        }
                    }
                };
                Ok::<_, Infallible>(service_fn(handle))
            }
        });
        let mut port = 3000;
        loop {
            let addr = SocketAddr::from(([127, 0, 0, 1], port));
            if let Ok(server) = Server::try_bind(&addr) {
                let mut url = url::Url::parse(&self.url)?;
                url.query_pairs_mut().append_pair("port", &port.to_string());
                open::that(&url.to_string()).unwrap_or(());
                eprintln!(
                    "If the URL doesn't open automatically, please visit {}",
                    url
                );
                let server = server.serve(make_service);
                select! {
                    x = server => {
                        if let Err(e) = x {
                            eprintln!("server error: {}", e);
                        }
                        break
                    }
                    x = rx.recv() => {
                        if let Some(x) = x {
                            if let Some(cache_path) = cache_path {
                                if let Some(c) = cache_path.parent() {
                                    std::fs::create_dir_all(c)?
                                }
                                if let Err(e) = std::fs::write(&cache_path, &x) {
                                    log::debug!("Error while writing file {:?}: {:?}", cache_path, e)
                                }
                            }
                            println!("Bearer {}", x);
                        }
                        break
                    }
                }
            }
            if port < u16::MAX {
                port += 1
            } else {
                break;
            }
        }
        Ok(())
    }
}
