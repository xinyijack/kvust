use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll, ready};
use bytes::BytesMut;
use futures::{FutureExt, Sink, Stream};
use tokio::io::{AsyncRead, AsyncWrite};
use crate::{FrameCoder, KvError};
use crate::frame::read_frame;

/// 处理 KV server prost frame 的 stream
pub struct ProstStream<S, In, Out> {
    // inner stream
    stream: S,
    // 写缓存
    wbuf: BytesMut,
    // 写入多少字节
    written: usize,
    // 读缓存
    rbuf: BytesMut,

    //类型占位符
    _in: PhantomData<In>,
    _out: PhantomData<Out>,
}

impl<S, In, Out> Stream for ProstStream<S, In, Out>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send,
        In: Unpin + Send + FrameCoder,
        Out: Unpin + Send,
{
    /// 当调用 next() 时，得到 Result<In, KvError>
    type Item = Result<In, KvError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // 上次调用结束后，rbuf 应该为空
        assert!(self.rbuf.len() == 0);

        // 从rbuf 中分离出 rest （摆脱对 self 的引用）
        let mut rest = self.rbuf.split_off(0);

        // 使用read_frame 来获取数据
        let fut = read_frame(&mut self.stream, &mut rest);
        ready!(Box::pin(fut).poll_unpin(cx))?;

        // 拿到一个 frame的数据，把buffer合并回去
        self.rbuf.unsplit(rest);

        // 调用 decode_frame 获取解包后的数据
        Poll::Ready(Some(In::decode_frame(&mut self.rbuf)))
    }
}

impl<S, In, Out> Sink<&Out> for ProstStream<S, In, Out>
    where
        S: AsyncRead + AsyncWrite + Unpin,
        In: Unpin + Send,
        Out: Unpin + Send + FrameCoder,
{
    /// 如果发送出错，会返回 KvError
    type Error = KvError;

    fn poll_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, item: &Out) -> Result<(), Self::Error> {
        let this = self.get_mut();
        item.encode_frame(&mut this.wbuf)?;

        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();

        // 循环写入 stream 中
        while this.written != this.wbuf.len() {
            let n = ready!(Pin::new(&mut this.stream).poll_write(cx, &this.wbuf[this.written..]))?;
            this.written += n;
        }

        // 清除 wbuf
        this.wbuf.clear();
        this.written = 0;

        // 调用 stream 的 poll_flush 确保写入
        ready!(Pin::new(&mut this.stream).poll_flush(cx)?);
        Poll::Ready(Ok(()))
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // 调用 stream 的 poll_flush 确保写入
        ready!(self.as_mut().poll_flush(cx))?;

        // 调用 stream 的 poll_shutdown 确保 stream 关闭
        ready!(Pin::new(&mut self.stream).poll_shutdown(cx))?;
        Poll::Ready(Ok(()))
    }
}

// 一般来说，如果我们的 Stream 是 Unpin，最好实现一下
// Unpin 不像 Send/Sync 会自动实现
impl<S, In, Out> Unpin for ProstStream<S, In, Out> where S: Unpin {}

impl<S, In, Out> ProstStream<S, In, Out>
where S: AsyncRead + AsyncWrite + Send + Unpin,
{
    /// 创建一个 ProstStream
    pub fn new(stream: S) -> Self {
        Self {
            stream: stream,
            wbuf: BytesMut::new(),
            written: 0,
            rbuf: BytesMut::new(),
            _in: PhantomData::default(),
            _out: PhantomData::default(),
        }
    }
}

// // 一般来说，如果我们的 Stream 是 Unpin，最好实现一下
// impl<S, Req, Res> Unpin for ProstStream<S, Req, Res> where S: Unpin {}

#[cfg(test)]
pub mod utils {
    use bytes::{BufMut, BytesMut};
    use std::task::Poll;
    use tokio::io::{AsyncRead, AsyncWrite};

    pub struct DummyStream {
        pub buf: BytesMut,
    }

    impl AsyncRead for DummyStream {
        fn poll_read(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            let len = buf.capacity();
            let data = self.get_mut().buf.split_to(len);
            buf.put_slice(&data);
            Poll::Ready(Ok(()))
        }
    }

    impl AsyncWrite for DummyStream {
        fn poll_write(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> Poll<Result<usize, std::io::Error>> {
            self.get_mut().buf.put_slice(buf);
            Poll::Ready(Ok(buf.len()))
        }

        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> Poll<Result<(), std::io::Error>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> Poll<Result<(), std::io::Error>> {
            Poll::Ready(Ok(()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{utils::DummyStream, CommandRequest};
    use anyhow::Result;
    use futures::prelude::*;

    #[allow(clippy::all)]
    #[tokio::test]
    async fn prost_stream_should_work() -> Result<()> {
        let buf = BytesMut::new();
        let stream = DummyStream { buf };

        // 创建 ProstStream
        let mut stream = ProstStream::<_, CommandRequest, CommandRequest>::new(stream);
        let cmd = CommandRequest::new_hdel("t1", "k1");

        // 使用 ProstStream 发送数据
        stream.send(&cmd).await?;

        // 使用 ProstStream 接收数据
        if let Some(Ok(s)) = stream.next().await {
            assert_eq!(s, cmd);
        } else {
            assert!(false);
        }
        Ok(())
    }
}