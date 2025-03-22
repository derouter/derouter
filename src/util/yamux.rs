use futures::{Future, TryStreamExt};
use std::marker::PhantomData;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::compat::TokioAsyncReadCompatExt;
use yamux::{Config, Connection, ConnectionError, Mode};

pub struct YamuxServer<S> {
	_conn: PhantomData<S>,
}

// See https://github.com/caelansar/kv/blob/master/src/network/multiplex/yamux_multiplex.rs.
impl<S> YamuxServer<S>
where
	S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
	pub fn new<F, Fut>(stream: S, config: Option<Config>, f: F) -> Self
	where
		F: FnMut(yamux::Stream) -> Fut,
		F: Send + 'static,
		Fut: Future<Output = Result<(), ConnectionError>> + Send + 'static,
	{
		let config = config.unwrap_or_default();
		let mut conn = Connection::new(stream.compat(), config, Mode::Server);

		tokio::spawn(
			futures::stream::poll_fn(move |cx| conn.poll_next_inbound(cx))
				.try_for_each_concurrent(None, f),
		);

		YamuxServer { _conn: PhantomData }
	}
}
