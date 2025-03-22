use serde::de::DeserializeOwned;
use tokio::io::{AsyncRead, AsyncReadExt, BufReader};

pub async fn write_cbor<T, U>(
	writer: &mut T,
	value: &U,
) -> Result<Result<(), std::io::Error>, serde_cbor::Error>
where
	T: tokio::io::AsyncWriteExt + std::marker::Unpin,
	U: serde::Serialize,
{
	let buf = serde_cbor::to_vec(&value)?;
	writer.write_u32(buf.len() as u32).await?;
	writer.write_all(&buf).await?;
	Ok(Ok(()))
}

pub struct CborReader<R> {
	reader: BufReader<R>,
}

impl<R> CborReader<R>
where
	R: AsyncRead + Unpin,
{
	pub fn new(inner: R) -> Self {
		CborReader {
			reader: BufReader::new(inner),
		}
	}

	pub fn get_mut(&mut self) -> &mut R {
		self.reader.get_mut()
	}

	pub async fn next_cbor<T>(&mut self) -> std::io::Result<Option<T>>
	where
		T: DeserializeOwned,
	{
		// Read the length of the CBOR data (u32)
		log::trace!("Reading CBOR length...");
		let mut len_buf = [0u8; 4];
		if self.reader.read_exact(&mut len_buf).await.is_err() {
			return Ok(None); // EOF or error
		}
		let len = u32::from_be_bytes(len_buf) as usize;

		// Read the buffer of length `len`
		log::trace!("Reading CBOR buffer ({})...", len);
		let mut buffer = vec![0u8; len];
		self.reader.read_exact(&mut buffer).await?;

		// Decode the CBOR data
		log::trace!("Decoding CBOR");
		match serde_cbor::from_slice(&buffer) {
			Ok(data) => Ok(Some(data)),
			Err(e) => Err(std::io::Error::new(
				std::io::ErrorKind::InvalidData,
				format!("CBOR decoding failed: {}", e),
			)),
		}
	}
}
