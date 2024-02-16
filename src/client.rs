use std::time::Duration;
use anyhow::Result;
use tokio::time;
use tokio_stream::StreamExt;
use tokio_util::compat::Compat;
use tracing::info;
use yamux::Stream;
use kv::{ClientConfig, CommandRequest, KvError, ProstClientStream, start_client_with_config};

#[tokio::main]
async fn main() -> Result<()> {
    let config: ClientConfig = toml::from_str(include_str!("../fixtures/client.conf"))?;
    // 打开一个 yamux ctrl
    let mut ctrl = start_client_with_config(&config).await?;

    let channel = "lobby";
    start_publishing(ctrl.open_stream().await?, channel)?;

    let mut stream = ctrl.open_stream().await?;

    // 生成一个 HSET 命令
    let cmd = CommandRequest::new_hset("table1", "hello", "world".to_string().into());
    let data = stream.execute_unary(&cmd).await?;
    info!("Got Response {:?}", data);

    // 生成一个 Subscribe 命令
    let cmd = CommandRequest::new_subscribe(channel);
    let mut stream = stream.execute_streaming(&cmd).await?;
    let id = stream.id;
    start_unsubscribe(ctrl.open_stream().await?, channel, id)?;

    while let Some(Ok(data)) = stream.next().await {
        println!("Got published data {:?}", data);
    }

    println!("Done!");
    Ok(())
}

fn start_unsubscribe(mut stream: ProstClientStream<Compat<Stream>>, name: &str, id: u32) -> Result<(), KvError> {
    let cmd = CommandRequest::new_unsubscribe(name, id as _);
    tokio::spawn(async move {
        time::sleep(Duration::from_millis(2000)).await;
        let res = stream.execute_unary(&cmd).await.unwrap();
        println!("Finished unsubscribing {:?}", res);
    });

    Ok(())
}

fn start_publishing(mut stream: ProstClientStream<Compat<yamux::Stream>>, name: &str) -> Result<(), KvError> {
    let cmd = CommandRequest::new_publish(name, vec![1.into(), 2.into(), "hello".into()]);
    tokio::spawn(async move {
        time::sleep(Duration::from_millis(1000)).await;
        let res = stream.execute_unary(&cmd).await.unwrap();
        println!("Finished publishing: {:?}", res);
    });

    Ok(())
}

