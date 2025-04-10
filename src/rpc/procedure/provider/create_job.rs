use serde::Deserialize;

#[derive(Deserialize, Debug)]
#[serde(tag = "tag", content = "content")]
pub enum ProviderCreateJobResponse {
	/// Provider module accepted the job and
	/// maybe even started backround processing.
	Ok,

	/// Provided `job_args` are invalid per protocol.
	InvalidJobArgs(String),

	/// Temporarily busy.
	Busy,
}
