pub mod pb;
pub mod error;
pub mod storage;
pub mod service;
pub mod network;
pub mod config;

pub use pb::abi::*;
pub use error::KvError;
pub use storage::*;
pub use service::*;
pub use network::*;
pub use config::*;

use anyhow::Result;
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::client;
use tokio_util::compat::FuturesAsyncReadCompatExt;
use tracing::{info, instrument, span};
use crate::multiplex::YamuxCtrl;


pub fn add(left: usize, right: usize) -> usize {
    left + right
}

#[instrument(skip_all)]
pub async fn start_server_with_config(config: &ServerConfig) -> Result<()> {
    let acceptor = TlsServerAcceptor::new(&config.tls.cert, &config.tls.key, config.tls.ca.as_deref())?;

    let addr = &config.general.addr;
    match &config.storage {
        StorageConfig::MemTable => start_tls_server(addr, MemTable::new(), acceptor).await?,
        StorageConfig::SledDb(path) => start_tls_server(addr, SledDb::new(path), acceptor).await?,
    }

    Ok(())
}

#[instrument(skip_all)]
pub async fn start_client_with_config(config: &ClientConfig) -> Result<YamuxCtrl<client::TlsStream<TcpStream>>> {
    let addr = &config.general.addr;
    let tls = &config.tls;

    let identity = tls.identity.as_ref().map(|(c, k)| {(c.as_str(), k.as_str())});
    let connector = TlsClientConnector::new(&tls.domain, identity, tls.ca.as_deref())?;
    let stream = TcpStream::connect(addr).await?;
    let stream = connector.connect(stream).await?;

    // 打开一个 stream
    Ok(YamuxCtrl::new_client(stream, None))
}

async fn start_tls_server<Store: Storage>(addr: &str, store: Store, acceptor: TlsServerAcceptor) -> Result<()> {
    let service: Service<Store> = ServiceInner::new(store).into();
    let listener = TcpListener::bind(addr).await?;
    info!("Start listening on {}", addr);
    loop {
        let root = span!(tracing::Level::INFO, "server_process");
        let _enter = root.enter();
        let tls = acceptor.clone();
        let (stream, addr) = listener.accept().await?;
        info!("Client {:?} connected", addr);

        let svc = service.clone();
        tokio::spawn(async move {
            let stream = tls.accept(stream).await.unwrap();
            YamuxCtrl::new_server(stream, None, move |stream| {
                let svc1 = svc.clone();
                async move {
                    let stream = ProstServerStream::new(stream.compat(),svc1.clone());
                    // 延迟 100ms 处理
                    // time::sleep(Duration::from_millis(100)).await;
                    stream.process().await.unwrap();
                    Ok(())
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
